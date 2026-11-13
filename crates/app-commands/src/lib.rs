//! Typed command and event contract between UI and core.
//! See docs/COMMANDS.md — the single source of truth for channel names.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccount {
    pub username: String,
    pub display_name: String,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordLoginParams {
    pub account: String,
    pub password: String,
    pub guard_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamProfile {
    pub account: String,
    pub steam_id: String,
}

pub const STEAM_LOGIN: &str = "steam_login";
pub const STEAM_SESSION: &str = "steam_session";
pub const STEAM_SIGN_OUT: &str = "steam_sign_out";
