use app_commands::{PasswordLoginParams, SteamProfile};
use steam_core::{Credentials, Profile, SessionManager, SteamClient};

type DesktopSession = SessionManager<SteamClient>;

fn profile_reply(profile: Profile) -> SteamProfile {
    SteamProfile {
        account: profile.account,
        steam_id: profile.steam_id,
    }
}

#[tauri::command]
pub async fn steam_login(
    params: PasswordLoginParams,
    session: tauri::State<'_, DesktopSession>,
) -> Result<SteamProfile, String> {
    session
        .login(Credentials {
            account: params.account,
            password: params.password,
            guard_code: params.guard_code,
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
pub fn steam_sign_out(session: tauri::State<'_, DesktopSession>) -> Result<(), String> {
    session.sign_out().map_err(|error| error.to_string())
}
