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
    #[serde(default)]
    pub image_url: Option<String>,
}

#[derive(Deserialize, Serialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct PasswordLoginParams {
    pub account: String,
    pub password: String,
    pub guard_code: Option<String>,
    #[serde(default)]
    pub shared_secret: Option<String>,
    #[serde(default)]
    pub remember: bool,
}

#[derive(Deserialize, Serialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
#[serde(rename_all = "camelCase")]
pub struct BrowserTokenLoginParams {
    pub payload: String,
}

#[derive(Deserialize, zeroize::Zeroize, zeroize::ZeroizeOnDrop)]
pub struct BrowserTokenPayload {
    pub logged_in: bool,
    pub account_name: String,
    pub token: String,
    pub steamid: String,
}

impl PasswordLoginParams {
    pub fn encode_and_clear<Output>(&mut self, encode: impl FnOnce(&Self) -> Output) -> Output {
        let output = encode(self);
        zeroize::Zeroize::zeroize(self);
        output
    }
}

impl BrowserTokenLoginParams {
    pub fn encode_and_clear<Output>(&mut self, encode: impl FnOnce(&Self) -> Output) -> Output {
        let output = encode(self);
        zeroize::Zeroize::zeroize(self);
        output
    }
}

#[cfg(test)]
mod credential_tests {
    use super::*;

    #[test]
    fn credential_buffers_are_cleared_before_a_serialization_result_can_be_awaited() {
        for failed in [false, true] {
            let mut params = PasswordLoginParams {
                account: "synthetic-user".into(),
                password: "synthetic-password".into(),
                guard_code: None,
                shared_secret: Some("synthetic-secret".into()),
                remember: true,
            };
            let result = params.encode_and_clear(|request| {
                assert!(!request.password.is_empty());
                assert!(request.shared_secret.is_some());
                if failed { Err(()) } else { Ok(()) }
            });
            assert!(params.password.is_empty());
            assert!(params.shared_secret.is_none());
            assert!(params.guard_code.is_none());
            assert_eq!(result.is_err(), failed);
        }
    }

    #[test]
    fn browser_token_payload_is_cleared_after_request_encoding() {
        let mut params = BrowserTokenLoginParams {
            payload: r#"{"token":"synthetic-token"}"#.into(),
        };
        let encoded = params.encode_and_clear(|request| {
            serde_json::from_str::<serde_json::Value>(&request.payload).unwrap()
        });
        assert_eq!(encoded["token"], "synthetic-token");
        assert!(params.payload.is_empty());
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SteamProfile {
    pub account: String,
    pub steam_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notice: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallet: Option<WalletInfo>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WalletInfo {
    pub has_wallet: bool,
    /// Whole currency units (Steam reports cents; divided by 100).
    pub balance: f64,
    pub currency: String,
}

pub const STEAM_LOGIN: &str = "steam_login";
pub const STEAM_LOGIN_SAVED: &str = "steam_login_saved";
pub const STEAM_SESSION: &str = "steam_session";
pub const STEAM_SIGN_OUT: &str = "steam_sign_out";
pub const STEAM_LOGIN_WEBTOKEN: &str = "steam_login_webtoken";
pub const STEAM_START_QR_LOGIN: &str = "steam_start_qr_login";
pub const LOGIN_QR_CHALLENGE: &str = "login:qr_challenge";
pub const WALLET_CHANGED: &str = "wallet:changed";
pub const REFRESH_INVENTORY: &str = "refresh_inventory";
pub const STORAGE_CONTENTS: &str = "storage_contents";
pub const STORE_GET_ACCOUNTS: &str = "store_get_accounts";
pub const STORE_DELETE_ACCOUNT: &str = "store_delete_account";
pub const STORE_SET_ACCOUNT_POSITION: &str = "store_set_account_position";

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountPositionParams {
    pub username: String,
    pub index: usize,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageContentsParams {
    pub casket_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventoryItem {
    pub id: String,
    pub definition_index: u32,
    pub name: String,
    pub custom_name: Option<String>,
    pub image_url: Option<String>,
    #[serde(default)]
    pub image_path: Option<String>,
    pub quantity: u32,
    pub rarity: u32,
    pub position: u32,
    pub wear: Option<f64>,
    pub wear_name: Option<String>,
    pub paint_seed: Option<u32>,
    pub stat_trak: bool,
    pub is_storage_unit: bool,
    /// Contained-item count for storage units (casket attribute 270).
    #[serde(default)]
    pub storage_item_count: Option<u32>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub major: Option<String>,
    /// Epoch milliseconds when the tradehold lifts; only present for items
    /// still trade-locked (legacy `trade_unlock`).
    #[serde(default)]
    pub trade_unlock: Option<i64>,
    #[serde(default)]
    pub moveable: bool,
    #[serde(default)]
    pub equipped_ct: bool,
    #[serde(default)]
    pub equipped_t: bool,
    /// Selected-source price in USD for moveable items (legacy prices only
    /// moveable rows); `None` when the source has no value.
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub collection: Option<String>,
    #[serde(default)]
    pub trade_up_eligible: bool,
    #[serde(default)]
    pub stickers: Vec<InventorySticker>,
    #[serde(default)]
    pub music_kit: Option<String>,
    #[serde(default)]
    pub graffiti: Option<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySticker {
    pub slot: u32,
    pub name: String,
    pub kind: String,
    pub image_path: Option<String>,
    #[serde(default)]
    pub image_url: Option<String>,
    pub wear: Option<f64>,
    pub scale: Option<f64>,
    pub rotation: Option<f64>,
    pub offset_x: Option<f64>,
    pub offset_y: Option<f64>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InventorySnapshot {
    pub steam_id: String,
    pub items: Vec<InventoryItem>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InventoryChangeReason {
    ItemAcquired,
    ItemChanged,
    ItemRemoved,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_contents_uses_camel_case_and_preserves_large_ids_as_strings() {
        let params = StorageContentsParams {
            casket_id: "18446744073709551614".into(),
        };
        let value = serde_json::to_value(&params).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"casketId": "18446744073709551614"})
        );
        assert_eq!(
            serde_json::from_value::<StorageContentsParams>(value).unwrap(),
            params
        );
    }

    #[test]
    fn profiles_without_enrichment_fields_deserialize_like_the_old_contract() {
        let old = serde_json::json!({"account": "alice", "steamId": "76561198000000000"});
        let profile: SteamProfile = serde_json::from_value(old).unwrap();
        assert_eq!(profile.display_name, None);
        assert_eq!(profile.avatar_url, None);
        assert_eq!(profile.wallet, None);

        let enriched = SteamProfile {
            account: "alice".into(),
            steam_id: "76561198000000000".into(),
            notice: None,
            display_name: Some("Alice".into()),
            avatar_url: Some("https://avatars.steamstatic.com/a_medium.jpg".into()),
            wallet: Some(WalletInfo {
                has_wallet: true,
                balance: 12.34,
                currency: "USD".into(),
            }),
        };
        let value = serde_json::to_value(&enriched).unwrap();
        assert_eq!(value["displayName"], "Alice");
        assert_eq!(value["wallet"]["balance"], 12.34);
        assert_eq!(
            serde_json::from_value::<SteamProfile>(value).unwrap(),
            enriched
        );
    }
}
