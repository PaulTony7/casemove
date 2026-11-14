use crate::{AppEvent, Core};
use app_commands::{
    BrowserTokenLoginParams, BrowserTokenPayload, PasswordLoginParams, SteamProfile, WalletInfo,
};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use steam_core::{Credentials, Profile, WalletUpdate};
use tokio_stream::StreamExt;
use zeroize::{Zeroize, Zeroizing};

fn with_profile<T>(
    state: &Mutex<Option<SteamProfile>>,
    f: impl FnOnce(&mut Option<SteamProfile>) -> T,
) -> T {
    let mut guard: MutexGuard<'_, Option<SteamProfile>> = state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
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
    // One retry: steamcommunity edges intermittently drop rapid requests.
    for attempt in 0..2 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(400)).await;
        }
        if let Ok(response) = reqwest::get(url).await
            && let Ok(response) = response.error_for_status()
            && let Ok(text) = response.text().await
        {
            return Some(text);
        }
    }
    None
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
        if (display_name.is_none() || avatar.is_none())
            && let Some(html) =
                http_text(&format!("https://steamcommunity.com/profiles/{steam_id}/")).await
        {
            display_name = display_name.or_else(|| extract_persona_name(&html));
            avatar = avatar.or_else(|| extract_avatar_url(&html));
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
    core: Core,
    generation: u64,
    mut stream: std::pin::Pin<Box<dyn tokio_stream::Stream<Item = WalletUpdate> + Send>>,
) {
    tokio::spawn(async move {
        while let Some(update) = stream.next().await {
            let still_current = core.session.in_generation(generation, || ()).is_ok();
            if !still_current {
                break;
            }
            let info = wallet_info(&update);
            with_profile(&core.profile, |profile| {
                if let Some(profile) = profile {
                    profile.wallet = Some(info.clone());
                }
            });
            core.events.emit(AppEvent::WalletChanged(info));
        }
    });
}

impl Core {
    /// Best-effort enrichment: wallet balance + Steam community identity.
    /// Never fails a login; missing data stays `None` in the reply.
    async fn enrich_reply(&self, generation: u64, reply: &mut SteamProfile) {
        let session = &*self.session;
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

        with_profile(&self.profile, |profile| {
            *profile = Some(reply.clone());
        });

        if let Some(stream) = stream {
            spawn_wallet_forwarder(self.clone(), generation, stream);
        }
    }

    async fn remember_session(
        &self,
        reply: &SteamProfile,
        login_generation: u64,
    ) -> Result<(), String> {
        let account = reply.account.clone();
        let display_name = reply.display_name.clone();
        let avatar_url = reply.avatar_url.clone();
        let core = self.clone();
        self.session
            .with_connection(move |connection, generation| async move {
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
                let blocking_core = core.clone();
                tokio::task::spawn_blocking(move || {
                    let mut store = blocking_core
                        .store
                        .lock()
                        .map_err(|_| "Saved-account storage is unavailable.".to_owned())?;
                    crate::accounts::remember_login(&mut store, &account, &token, |store| {
                        blocking_core
                            .session
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

    pub async fn steam_login(
        &self,
        mut params: PasswordLoginParams,
    ) -> Result<SteamProfile, String> {
        let remember = params.remember;
        let profile = self
            .session
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
        self.enrich_reply(login_generation, &mut reply).await;
        if remember && let Err(error) = self.remember_session(&reply, login_generation).await {
            reply.notice = Some(format!(
                "Signed in, but this login was not remembered. {error}"
            ));
        }
        self.session
            .in_generation(login_generation, || ())
            .map_err(|error| error.to_string())?;
        Ok(reply)
    }

    pub async fn steam_start_qr_login(&self, remember: bool) -> Result<SteamProfile, String> {
        let (challenge_tx, mut challenge_rx) = tokio::sync::mpsc::unbounded_channel();
        let relay_core = self.clone();
        let relay = tokio::spawn(async move {
            match challenge_rx.recv().await {
                Some(challenge) => {
                    relay_core.events.emit(AppEvent::QrChallenge(challenge));
                    Ok(())
                }
                None => Ok(()),
            }
        });
        let result = self
            .session
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
        // A panicked relay task means the challenge never reached the UI: treat
        // it like a delivery failure so a successful login cannot strand a
        // session the UI never saw start.
        let relay_result = relay
            .await
            .unwrap_or_else(|_| Err("Could not deliver the Steam QR challenge.".to_owned()));
        let profile = result.map_err(|error| error.to_string())?;
        if let Err(error) = relay_result {
            let _ = self.session.sign_out_generation();
            return Err(error);
        }

        let login_generation = profile.generation;
        let mut reply = profile_reply(profile);
        self.enrich_reply(login_generation, &mut reply).await;
        if remember && let Err(error) = self.remember_session(&reply, login_generation).await {
            reply.notice = Some(format!(
                "Signed in, but this login was not remembered. {error}"
            ));
        }
        self.session
            .in_generation(login_generation, || ())
            .map_err(|error| error.to_string())?;
        Ok(reply)
    }

    pub async fn steam_login_webtoken(
        &self,
        mut params: BrowserTokenLoginParams,
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
        let profile = self
            .session
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
        self.enrich_reply(generation, &mut reply).await;
        self.session
            .in_generation(generation, || ())
            .map_err(|error| error.to_string())?;
        Ok(reply)
    }

    pub async fn steam_login_saved(&self, account: String) -> Result<SteamProfile, String> {
        let account = settings::secret::canonical_account(&account);
        let profile = self
            .session
            .login_with(async move {
                let lookup = account.clone();
                let token = tokio::task::spawn_blocking(move || {
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
        self.enrich_reply(generation, &mut reply).await;
        self.session
            .in_generation(generation, || ())
            .map_err(|error| error.to_string())?;
        Ok(reply)
    }

    pub fn steam_session(&self) -> Result<Option<SteamProfile>, String> {
        let snapshot = with_profile(&self.profile, |profile| profile.clone());
        if snapshot.is_some() {
            return Ok(snapshot);
        }
        self.session
            .profile()
            .map(|profile| profile.map(profile_reply))
            .map_err(|error| error.to_string())
    }

    pub async fn steam_sign_out(&self) -> Result<(), String> {
        let generation = self
            .session
            .sign_out_generation()
            .map_err(|error| error.to_string())?;
        self.inventory.clear(generation).await;
        with_profile(&self.profile, |profile| {
            *profile = None;
        });
        Ok(())
    }
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
        assert_eq!(
            extract_tag("<steamID>Bob &amp; Co</steamID>", "steamID").unwrap(),
            "Bob & Co"
        );
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
            Some(
                "https://avatars.fastly.steamstatic.com/fef49e7fa7e1997310d705b2a6158ff8dc1cdfeb_full.jpg"
            )
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
        assert_eq!(
            extract_avatar_url(r#"<img src="https://evil.example/x.jpg">"#),
            None
        );
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

#[cfg(test)]
mod live_tests {
    use super::*;

    #[tokio::test]
    #[ignore = "network: live steamcommunity fetch"]
    async fn fetches_live_identity_for_the_reference_profile() {
        let html_url = "https://steamcommunity.com/profiles/76561198048391869/";
        match reqwest::get(html_url).await {
            Ok(response) => {
                println!("html status: {}", response.status());
                match response.text().await {
                    Ok(text) => println!("html text: {} bytes", text.len()),
                    Err(error) => println!("html text error: {error:?}"),
                }
            }
            Err(error) => println!("html get error: {error:?}"),
        }

        let identity = fetch_steam_identity("76561198048391869").await;
        println!("fetch_steam_identity: {identity:?}");
        let (display, avatar) = identity.expect("identity fetch should succeed");
        assert!(display.is_some(), "expected a display name");
        assert!(avatar.is_some(), "expected an avatar URL");
    }
}
