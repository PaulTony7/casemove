use app_commands::{
    BrowserTokenLoginParams, BrowserTokenPayload, LOGIN_QR_CHALLENGE, PasswordLoginParams,
    SteamProfile, WALLET_CHANGED, WalletInfo,
};
use settings::SettingsStore;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use steam_core::{Credentials, Profile, SessionManager, SteamClient, WalletUpdate};
use tauri::{Emitter, Manager};
use tokio_stream::StreamExt;
use zeroize::{Zeroize, Zeroizing};

pub(crate) type DesktopSession = SessionManager<SteamClient>;

/// Enriched session view (display name, avatar, wallet) kept current across
/// UI reloads so `steam_session` can restore what login replies already had.
#[derive(Default)]
pub(crate) struct ProfileState(pub(crate) Mutex<Option<SteamProfile>>);

fn with_profile<T>(state: &Mutex<Option<SteamProfile>>, f: impl FnOnce(&mut Option<SteamProfile>) -> T) -> T {
    let mut guard: MutexGuard<'_, Option<SteamProfile>> =
        state.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard)
}

fn profile_reply(profile: Profile) -> SteamProfile {
    SteamProfile {
        account: profile.account,
        steam_id: profile.steam_id,
        notice: None,
        display_name: None,
        avatar_url: None,
        wallet: None,
    }
}

fn extract_between(haystack: &str, open: &str, close: &str) -> Option<String> {
    let start = haystack.find(open)? + open.len();
    let end = start + haystack[start..].find(close)?;
    Some(haystack[start..end].trim().to_owned())
}

fn decode_entities(value: &str) -> String {
    value
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn extract_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let value = extract_between(xml, &open, &close)?;
    if let Some(inner) = value
        .strip_prefix("<![CDATA[")
        .and_then(|rest| rest.strip_suffix("]]>"))
    {
        return Some(inner.to_owned());
    }
    Some(decode_entities(&value))
}

/// Persona name from the HTML profile page — the XML endpoint leaves
/// `steamID` empty for profiles Steam considers "not set up".
fn extract_persona_name(html: &str) -> Option<String> {
    let marker = "actual_persona_name\">";
    let start = html.find(marker)? + marker.len();
    let end = start + html[start..].find('<')?;
    let name = decode_entities(html[start..end].trim());
    (!name.is_empty()).then_some(name)
}

/// First avatar image reference in the HTML page (`srcset`/`src`), checked
/// against the Steam-host allowlist.
fn extract_avatar_url(html: &str) -> Option<String> {
    let start = html.find("https://avatars.")?;
    let rest = &html[start..];
    let end = rest.find(|c: char| c == '"' || c == '\'' || c.is_whitespace())?;
    let url = &rest[..end];
    crate::accounts::is_steam_avatar_url(url).then(|| url.to_owned())
}

async fn http_text(url: &str) -> Option<String> {
    let response = reqwest::get(url).await.ok()?.error_for_status().ok()?;
    response.text().await.ok()
}

async fn fetch_steam_identity(steam_id: &str) -> Option<(Option<String>, Option<String>)> {
    let fetch = async {
        let xml = http_text(&format!(
            "https://steamcommunity.com/profiles/{steam_id}/?xml=1"
        ))
        .await?;
        let mut display_name = extract_tag(&xml, "steamID").filter(|name| !name.is_empty());
        let mut avatar = extract_tag(&xml, "avatarMedium")
            .filter(|url| crate::accounts::is_steam_avatar_url(url));
        if display_name.is_none() || avatar.is_none() {
            if let Some(html) =
                http_text(&format!("https://steamcommunity.com/profiles/{steam_id}/")).await
            {
                display_name = display_name.or_else(|| extract_persona_name(&html));
                avatar = avatar.or_else(|| extract_avatar_url(&html));
            }
        }
        Some((display_name, avatar))
    };
    tokio::time::timeout(Duration::from_secs(8), fetch)
        .await
        .ok()
        .flatten()
}

fn wallet_info(update: &WalletUpdate) -> WalletInfo {
    WalletInfo {
        has_wallet: update.has_wallet,
        balance: update.balance,
        currency: update.currency.clone(),
    }
}

fn spawn_wallet_forwarder(
    app: tauri::AppHandle,
    generation: u64,
    mut stream: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = WalletUpdate> + Send>>,
) {
    tauri::async_runtime::spawn(async move {
        while let Some(update) = stream.next().await {
            let still_current = app
                .state::<DesktopSession>()
                .in_generation(generation, || ())
                .is_ok();
            if !still_current {
                break;
            }
            let info = wallet_info(&update);
            with_profile(&app.state::<ProfileState>().0, |profile| {
                if let Some(profile) = profile {
                    profile.wallet = Some(info.clone());
                }
            });
            let _ = app.emit(WALLET_CHANGED, &info);
        }
    });
}

/// Best-effort enrichment: wallet balance + Steam community identity.
/// Never fails a login; missing data stays `None` in the reply.
async fn enrich_reply(
    app: tauri::AppHandle,
    session: &DesktopSession,
    generation: u64,
    reply: &mut SteamProfile,
) {
    let wallet_work = async {
        let stream = session
            .with_connection({
                let expected = generation;
                move |connection, current| async move {
                    if current != expected {
                        None
                    } else {
                        connection.wallet_stream()
                    }
                }
            })
            .await
            .ok()
            .flatten();
        let Some(mut stream) = stream else {
            return (None, None);
        };
        let first = tokio::time::timeout(Duration::from_secs(3), stream.next())
            .await
            .ok()
            .flatten();
        (Some(stream), first)
    };

    let ((stream, first_wallet), identity) =
        tokio::join!(wallet_work, fetch_steam_identity(&reply.steam_id));

    if let Some(update) = first_wallet {
        reply.wallet = Some(wallet_info(&update));
    }
    if let Some((display_name, avatar_url)) = identity {
        reply.display_name = display_name;
        reply.avatar_url = avatar_url;
    }

    eprintln!(
        "casemove enrich: wallet_stream={} wallet_first={} display_name={} avatar={}",
        stream.is_some(),
        reply.wallet.is_some(),
        reply.display_name.is_some(),
        reply.avatar_url.is_some(),
    );

    with_profile(&app.state::<ProfileState>().0, |profile| {
        *profile = Some(reply.clone());
    });

    if let Some(stream) = stream {
        spawn_wallet_forwarder(app, generation, stream);
    }
}

async fn remember_session(
    app: tauri::AppHandle,
    session: &DesktopSession,
    reply: &SteamProfile,
    login_generation: u64,
) -> Result<(), String> {
    let account = reply.account.clone();
    let display_name = reply.display_name.clone();
    let avatar_url = reply.avatar_url.clone();
    session
        .with_connection(|connection, generation| async move {
            if generation != login_generation {
                return Err(
                    "The Steam session changed before this login could be remembered.".into(),
                );
            }
            let token = connection
                .access_token()
                .filter(|token| !token.is_empty())
                .map(|token| Zeroizing::new(token.to_owned()))
                .ok_or_else(|| "Steam did not provide a reusable login token.".to_owned())?;
            tauri::async_runtime::spawn_blocking(move || {
                let session = app.state::<DesktopSession>();
                let settings = app.state::<Mutex<SettingsStore>>();
                let mut store = settings
                    .lock()
                    .map_err(|_| "Saved-account storage is unavailable.".to_owned())?;
                crate::accounts::remember_login(&mut store, &account, &token, |store| {
                    session
                        .in_generation(generation, || {
                            crate::accounts::save_account_identity(
                                store,
                                &account,
                                display_name.as_deref(),
                                avatar_url.as_deref(),
                            )
                        })
                        .map_err(|error| error.to_string())?
                })
            })
            .await
            .map_err(|_| "The saved-login operation failed.".to_owned())?
        })
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn steam_login(
    mut params: PasswordLoginParams,
    session: tauri::State<'_, DesktopSession>,
    app: tauri::AppHandle,
) -> Result<SteamProfile, String> {
    let remember = params.remember;
    let profile = session
        .login(Credentials {
            account: settings::secret::canonical_account(&params.account),
            password: std::mem::take(&mut params.password),
            guard_code: params.guard_code.take(),
            refresh_token: None,
            shared_secret: params.shared_secret.take(),
            web_logon_token: None,
            steam_id: None,
            qr_challenge_tx: None,
        })
        .await
        .map_err(|error| error.to_string())?;
    let login_generation = profile.generation;
    let mut reply = profile_reply(profile);
    enrich_reply(app.clone(), &session, login_generation, &mut reply).await;
    if remember {
        if let Err(error) = remember_session(app.clone(), &session, &reply, login_generation).await
        {
            reply.notice = Some(format!(
                "Signed in, but this login was not remembered. {error}"
            ));
        }
    }
    session
        .in_generation(login_generation, || ())
        .map_err(|error| error.to_string())?;
    Ok(reply)
}

#[tauri::command]
pub async fn steam_start_qr_login(
    remember: bool,
    session: tauri::State<'_, DesktopSession>,
    app: tauri::AppHandle,
) -> Result<SteamProfile, String> {
    let (challenge_tx, mut challenge_rx) = tokio::sync::mpsc::unbounded_channel();
    let event_app = app.clone();
    let relay = tauri::async_runtime::spawn(async move {
        match challenge_rx.recv().await {
            Some(challenge) => event_app
                .emit(LOGIN_QR_CHALLENGE, challenge)
                .map_err(|error| error.to_string()),
            None => Ok(()),
        }
    });
    let result = session
        .login(Credentials {
            account: String::new(),
            password: String::new(),
            guard_code: None,
            refresh_token: None,
            shared_secret: None,
            web_logon_token: None,
            steam_id: None,
            qr_challenge_tx: Some(challenge_tx),
        })
        .await;
    // A panicked relay task means the challenge never reached the UI: treat it
    // like an emit failure so a successful login cannot strand a session the
    // UI never saw start.
    let relay_result = relay
        .await
        .unwrap_or_else(|_| Err("Could not deliver the Steam QR challenge.".to_owned()));
    let profile = result.map_err(|error| error.to_string())?;
    if let Err(error) = relay_result {
        let _ = session.sign_out_generation();
        return Err(error);
    }

    let login_generation = profile.generation;
    let mut reply = profile_reply(profile);
    enrich_reply(app.clone(), &session, login_generation, &mut reply).await;
    if remember
        && let Err(error) =
            remember_session(app, &session, &reply, login_generation).await
    {
        reply.notice = Some(format!(
            "Signed in, but this login was not remembered. {error}"
        ));
    }
    session
        .in_generation(login_generation, || ())
        .map_err(|error| error.to_string())?;
    Ok(reply)
}

#[tauri::command]
pub async fn steam_login_webtoken(
    mut params: BrowserTokenLoginParams,
    session: tauri::State<'_, DesktopSession>,
    app: tauri::AppHandle,
) -> Result<SteamProfile, String> {
    let mut payload: BrowserTokenPayload = serde_json::from_str(&params.payload)
        .map_err(|_| "The browser login data must be valid JSON.".to_owned())?;
    params.payload.zeroize();
    if !payload.logged_in {
        return Err("Sign in to Steam in your browser and try again.".into());
    }
    if payload.account_name.trim().is_empty()
        || payload.token.trim().is_empty()
        || payload.steamid.trim().is_empty()
    {
        return Err("The browser login data is incomplete.".into());
    }
    let profile = session
        .login(Credentials {
            account: settings::secret::canonical_account(&payload.account_name),
            password: String::new(),
            guard_code: None,
            refresh_token: None,
            shared_secret: None,
            web_logon_token: Some(std::mem::take(&mut payload.token)),
            steam_id: Some(std::mem::take(&mut payload.steamid)),
            qr_challenge_tx: None,
        })
        .await
        .map_err(|error| error.to_string())?;
    let generation = profile.generation;
    let mut reply = profile_reply(profile);
    enrich_reply(app, &session, generation, &mut reply).await;
    session
        .in_generation(generation, || ())
        .map_err(|error| error.to_string())?;
    Ok(reply)
}

#[tauri::command]
pub async fn steam_login_saved(
    account: String,
    session: tauri::State<'_, DesktopSession>,
    app: tauri::AppHandle,
) -> Result<SteamProfile, String> {
    let account = settings::secret::canonical_account(&account);
    let profile = session
        .login_with(async move {
            let lookup = account.clone();
            let token = tauri::async_runtime::spawn_blocking(move || {
                crate::accounts::login_vault()
                    .get_optional(&lookup)
                    .map_err(|_| steam_core::LoginError::SecureStorageUnavailable)?
                    .filter(|token| !token.is_empty())
                    .map(Zeroizing::new)
                    .ok_or(steam_core::LoginError::SavedLoginMissing)
            })
            .await
            .map_err(|_| steam_core::LoginError::SecureStorageUnavailable)??;
            Ok(Credentials {
                account,
                password: String::new(),
                guard_code: None,
                refresh_token: Some(token.to_string()),
                shared_secret: None,
                web_logon_token: None,
                steam_id: None,
                qr_challenge_tx: None,
            })
        })
        .await
        .map_err(|error| error.to_string())?;
    let generation = profile.generation;
    let mut reply = profile_reply(profile);
    enrich_reply(app, &session, generation, &mut reply).await;
    session
        .in_generation(generation, || ())
        .map_err(|error| error.to_string())?;
    Ok(reply)
}

#[tauri::command]
pub fn steam_session(
    session: tauri::State<'_, DesktopSession>,
    profile_state: tauri::State<'_, ProfileState>,
) -> Result<Option<SteamProfile>, String> {
    let snapshot = with_profile(&profile_state.0, |profile| profile.clone());
    if snapshot.is_some() {
        return Ok(snapshot);
    }
    session
        .profile()
        .map(|profile| profile.map(profile_reply))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn steam_sign_out(
    session: tauri::State<'_, DesktopSession>,
    inventory: tauri::State<'_, crate::inventory::InventoryState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let generation = session
        .sign_out_generation()
        .map_err(|error| error.to_string())?;
    inventory.clear(generation).await;
    with_profile(&app.state::<ProfileState>().0, |profile| {
        *profile = None;
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_profile_identity_without_confusing_steam_id64() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?><profile>
            <steamID64>76561198000000000</steamID64>
            <steamID><![CDATA[Real Name & Co]]></steamID>
            <avatarMedium><![CDATA[https://avatars.fastly.steamstatic.com/x_medium.jpg]]></avatarMedium>
        </profile>"#;
        assert_eq!(
            extract_tag(xml, "steamID").as_deref(),
            Some("Real Name & Co")
        );
        assert_eq!(
            extract_tag(xml, "steamID64").as_deref(),
            Some("76561198000000000")
        );
        assert_eq!(
            extract_tag(xml, "avatarMedium").as_deref(),
            Some("https://avatars.fastly.steamstatic.com/x_medium.jpg")
        );
        assert_eq!(extract_tag(xml, "missing"), None);
    }

    #[test]
    fn unescapes_plain_xml_entities() {
        assert_eq!(extract_tag("<steamID>Bob &amp; Co</steamID>", "steamID").unwrap(), "Bob & Co");
        assert_eq!(
            extract_tag("<steamID>&quot;Nick&quot;</steamID>", "steamID").unwrap(),
            "\"Nick\""
        );
        // The limited-profile response: empty name, no avatar — both None.
        let xml = "<profile><steamID64>1</steamID64><steamID><![CDATA[]]></steamID></profile>";
        assert_eq!(extract_tag(xml, "steamID").as_deref(), Some(""));
        assert_eq!(extract_tag(xml, "avatarMedium"), None);
    }

    #[test]
    fn html_fallback_extracts_persona_and_avatar_for_unset_profiles() {
        let html = r#"<html><body>
            <img srcset="https://avatars.fastly.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_full.jpg 1.5x, https://avatars.fastly.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_full.jpg 2x" src="https://avatars.fastly.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_medium.jpg">
            <span class="actual_persona_name">spiderman_is_me_adam</span>
        </body></html>"#;
        assert_eq!(
            extract_persona_name(html).as_deref(),
            Some("spiderman_is_me_adam")
        );
        assert_eq!(
            extract_avatar_url(html).as_deref(),
            Some("https://avatars.fastly.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_full.jpg")
        );

        assert_eq!(
            extract_persona_name(r#"<span class="actual_persona_name">Bob &amp; Co</span>"#)
                .as_deref(),
            Some("Bob & Co")
        );
        assert_eq!(
            extract_persona_name(r#"<span class="actual_persona_name"></span>"#),
            None
        );
        // Only allowlisted Steam hosts survive.
        assert_eq!(
            extract_avatar_url(r#"<img src="https://avatars.evil.example/x.jpg">"#),
            None
        );
        assert_eq!(extract_avatar_url(r#"<img src="https://evil.example/x.jpg">"#), None);
    }

    #[test]
    fn wallet_info_copies_the_update_verbatim() {
        let update = WalletUpdate {
            has_wallet: true,
            balance: 12.34,
            currency: "EUR".into(),
        };
        assert_eq!(
            wallet_info(&update),
            WalletInfo {
                has_wallet: true,
                balance: 12.34,
                currency: "EUR".into(),
            }
        );
    }
}
