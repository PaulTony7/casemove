//! Native command layer: the typed contract from `app-commands` implemented as
//! plain async functions over shared state. Formerly the Tauri command handlers
//! in `src-tauri/`; see docs/COMMANDS.md for the contract these names come from.

mod accounts;
mod catalog;
mod inventory;
mod prices;
mod session;

pub use accounts::initialize_store;
pub use catalog::fetch_image_catalog;
pub use inventory::InventoryState;
pub use prices::fetch_live_prices;

use app_commands::{SteamProfile, WalletInfo};
use item_schema::ItemSchema;
use settings::SettingsStore;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use steam_core::{SessionManager, SteamClient};
use tokio::sync::mpsc::UnboundedSender;

pub type DesktopSession = SessionManager<SteamClient>;

/// Core → UI pushes (the former `login:qr_challenge` and `wallet:changed`
/// Tauri events).
#[derive(Clone, Debug)]
pub enum AppEvent {
    QrChallenge(String),
    WalletChanged(WalletInfo),
}

/// Delivers core events to the UI and requests a repaint when one lands.
#[derive(Clone)]
pub struct EventSink {
    tx: UnboundedSender<AppEvent>,
    repaint: Arc<dyn Fn() + Send + Sync>,
}

impl EventSink {
    pub fn new(tx: UnboundedSender<AppEvent>, repaint: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self { tx, repaint }
    }

    pub fn emit(&self, event: AppEvent) {
        if self.tx.send(event).is_ok() {
            (self.repaint)();
        }
    }
}

/// Everything the command handlers need. Cheap to clone: every handle shares
/// the same underlying state.
#[derive(Clone)]
pub struct Core {
    pub session: Arc<DesktopSession>,
    pub profile: Arc<Mutex<Option<SteamProfile>>>,
    pub inventory: Arc<InventoryState>,
    pub store: Arc<Mutex<SettingsStore>>,
    /// Shared so the live schema refresh can swap it after startup.
    pub schema: Arc<RwLock<ItemSchema>>,
    pub events: EventSink,
}

impl Core {
    /// Opens the settings store (with the one-time legacy import) and the
    /// bundled schema, then assembles the shared command state.
    pub fn build(events: EventSink) -> Result<Self, String> {
        let store = initialize_store(
            config_dir().join("settings.json"),
            settings::legacy::legacy_store_candidates(),
        )
        .map_err(|error| format!("Could not open settings: {error}"))?;
        let schema = ItemSchema::bundled()
            .map_err(|error| format!("Could not load the bundled item schema: {error}"))?;
        Ok(Self {
            session: Arc::new(SessionManager::new(
                steam_core::SteamClient,
                std::time::Duration::from_secs(120),
            )),
            profile: Arc::new(Mutex::new(None)),
            inventory: Arc::new(InventoryState::default()),
            store: Arc::new(Mutex::new(store)),
            schema: Arc::new(RwLock::new(schema)),
            events,
        })
    }

    /// Replaces the bundled schema with the live skinledger dump (legacy's
    /// `updateItems`/`getTranslations`). Any failure keeps the bundled
    /// schema — the fallback is the behavior.
    pub async fn refresh_schema(&self) {
        let Some((items, english)) = fetch_schema_files().await else {
            return;
        };
        let parsed = tokio::task::spawn_blocking(move || ItemSchema::from_vdf(&items, &english))
            .await
            .ok()
            .and_then(Result::ok);
        if let Some(schema) = parsed
            && let Ok(mut slot) = self.schema.write()
        {
            *slot = schema;
        }
    }

    /// Generic dot-path setting read (e.g. `overview.by`).
    pub fn store_get<T: serde::de::DeserializeOwned>(&self, key: &str) -> Option<T> {
        self.store.lock().ok().and_then(|store| store.get(key))
    }

    /// Generic dot-path setting write, persisted like the legacy store.
    pub fn store_set<T: serde::Serialize>(&self, key: &str, value: T) -> Result<(), String> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| "The settings store is busy.".to_owned())?;
        store
            .set(key, value)
            .map_err(|error| format!("Could not save the setting: {error}"))
    }
}

/// Legacy's live schema sources (`itemsLink`/`translationsLink`).
const SCHEMA_ITEMS_URL: &str = "https://files.skinledger.com/counterstrike/items_game.txt";
const SCHEMA_TRANSLATIONS_URL: &str = "https://files.skinledger.com/counterstrike/csgo_english.txt";

async fn fetch_schema_files() -> Option<(String, String)> {
    let items = fetch_text(SCHEMA_ITEMS_URL).await?;
    let english = fetch_text(SCHEMA_TRANSLATIONS_URL).await?;
    Some((items, english))
}

async fn fetch_text(url: &str) -> Option<String> {
    let request = async {
        reqwest::get(url)
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .text()
            .await
            .ok()
    };
    tokio::time::timeout(std::time::Duration::from_secs(60), request)
        .await
        .ok()
        .flatten()
}

fn home_dir() -> PathBuf {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(PathBuf::from);
    home.unwrap_or_else(|| PathBuf::from("."))
}

/// Config directory matching Tauri's `app_config_dir` for the
/// `com.nombers.casemove` identifier, so settings written by earlier builds
/// stay where they are.
pub fn config_dir() -> PathBuf {
    #[cfg(windows)]
    {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join("AppData").join("Roaming"));
        base.join("com.nombers.casemove")
    }
    #[cfg(target_os = "macos")]
    {
        home_dir()
            .join("Library")
            .join("Application Support")
            .join("com.nombers.casemove")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".config"));
        base.join("com.nombers.casemove")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "downloads the live schema (~10 MB)"]
    async fn live_skinledger_files_parse_into_a_usable_schema() {
        let (items, english) = fetch_schema_files().await.expect("skinledger reachable");
        let schema = ItemSchema::from_vdf(&items, &english).expect("live VDF parses");
        assert_eq!(schema.describe(7, None).name, "AK-47");
        assert!(schema.describe(7, None).image_path.is_some());
        assert!(schema.music_image(1).is_some() || schema.music_image(3).is_some());
    }
}
