//! Typed command and event contract between UI and core.
//! See docs/COMMANDS.md — the single source of truth for channel names.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccount {
    pub username: String,
    pub display_name: String,
    #[serde(default)]
    pub has_saved_login: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PasswordLoginParams {
    pub account: String,
    pub password: String,
    pub guard_code: Option<String>,
    #[serde(default)]
    pub remember: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamProfile {
    pub account: String,
    pub steam_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
}

pub const STEAM_LOGIN: &str = "steam_login";
pub const STEAM_LOGIN_SAVED: &str = "steam_login_saved";
pub const STEAM_SESSION: &str = "steam_session";
pub const STEAM_SIGN_OUT: &str = "steam_sign_out";
pub const REFRESH_INVENTORY: &str = "refresh_inventory";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub id: String,
    pub definition_index: u32,
    pub name: String,
    pub custom_name: Option<String>,
    pub image_url: Option<String>,
    pub quantity: u32,
    pub rarity: u32,
    pub position: u32,
    pub wear: Option<f64>,
    pub wear_name: Option<String>,
    pub paint_seed: Option<u32>,
    pub stat_trak: bool,
    pub is_storage_unit: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshot {
    pub steam_id: String,
    pub items: Vec<InventoryItem>,
}
