use app_commands::{PasswordLoginParams, SteamProfile};
use settings::SettingsStore;
use std::sync::Mutex;
use steam_core::{Credentials, Profile, SessionManager, SteamClient};
use tauri::Manager;
use zeroize::Zeroizing;

pub(crate) type DesktopSession = SessionManager<SteamClient>;

fn profile_reply(profile: Profile) -> SteamProfile {
    SteamProfile {
        account: profile.account,
        steam_id: profile.steam_id,
        notice: None,
    }
}

#[tauri::command]
pub async fn steam_login(
    params: PasswordLoginParams,
    session: tauri::State<'_, DesktopSession>,
    app: tauri::AppHandle,
) -> Result<SteamProfile, String> {
    let remember = params.remember;
    let profile = session
        .login(Credentials {
            account: settings::secret::canonical_account(&params.account),
            password: params.password,
            guard_code: params.guard_code,
            refresh_token: None,
        })
        .await
        .map_err(|error| error.to_string())?;
    let login_generation = profile.generation;
    let mut reply = profile_reply(profile);
    if remember {
        let account = reply.account.clone();
        let result = session
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
                                crate::accounts::save_account_metadata(store, &account)
                            })
                            .map_err(|error| error.to_string())?
                    })
                })
                .await
                .map_err(|_| "The saved-login operation failed.".to_owned())?
            })
            .await
            .map_err(|error| error.to_string())?;
        if let Err(error) = result {
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
pub async fn steam_login_saved(
    account: String,
    session: tauri::State<'_, DesktopSession>,
) -> Result<SteamProfile, String> {
    let account = settings::secret::canonical_account(&account);
    session
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
            })
        })
        .await
        .map(profile_reply)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn steam_session(
    session: tauri::State<'_, DesktopSession>,
) -> Result<Option<SteamProfile>, String> {
    session
        .profile()
        .map(|profile| profile.map(profile_reply))
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn steam_sign_out(
    session: tauri::State<'_, DesktopSession>,
    inventory: tauri::State<'_, crate::inventory::InventoryState>,
) -> Result<(), String> {
    let generation = session
        .sign_out_generation()
        .map_err(|error| error.to_string())?;
    inventory.clear(generation).await;
    Ok(())
}
