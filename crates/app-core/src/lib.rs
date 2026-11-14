//! Native command layer: the typed contract from `app-commands` implemented as
//! plain async functions over shared state. Formerly the Tauri command handlers
//! in `src-tauri/`; see docs/COMMANDS.md for the contract these names come from.

mod accounts;
mod inventory;
mod prices;
mod schema;
mod session;

pub use accounts::initialize_store;
pub use inventory::InventoryState;
pub use prices::fetch_live_prices;
pub use schema::SchemaRefreshReport;

use app_commands::{SteamProfile, WalletInfo};
use item_schema::ItemSchema;
use settings::SettingsStore;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
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
    InventoryChanged {
        steam_id: String,
        reason: app_commands::InventoryChangeReason,
        item_id: String,
        item: Option<Box<app_commands::InventoryItem>>,
    },
    InventoryError(String),
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
    pub schema: Arc<RwLock<Arc<ItemSchema>>>,
    schema_cache_path: Arc<PathBuf>,
    schema_cache_valid: Arc<AtomicBool>,
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
        let schema_path = config_dir().join("item-schema-cache.json");
        let (schema, schema_cache_valid) = match ItemSchema::load_cache(&schema_path) {
            Ok(Some(schema)) => (schema, true),
            Ok(None) => (
                ItemSchema::bundled().map_err(|error| {
                    format!("Could not load the bundled item schema: {error}")
                })?,
                false,
            ),
            Err(error) => {
                eprintln!("{error}; using the bundled item schema.");
                (
                    ItemSchema::bundled().map_err(|error| {
                        format!("Could not load the bundled item schema: {error}")
                    })?,
                    false,
                )
            }
        };
        Ok(Self {
            session: Arc::new(SessionManager::new(
                steam_core::SteamClient,
                std::time::Duration::from_secs(120),
            )),
            profile: Arc::new(Mutex::new(None)),
            inventory: Arc::new(InventoryState::default()),
            store: Arc::new(Mutex::new(store)),
            schema: Arc::new(RwLock::new(Arc::new(schema))),
            schema_cache_path: Arc::new(schema_path),
            schema_cache_valid: Arc::new(AtomicBool::new(schema_cache_valid)),
            events,
        })
    }

    fn schema_snapshot(&self) -> Result<Arc<ItemSchema>, String> {
        self.schema
            .read()
            .map(|schema| schema.clone())
            .map_err(|_| "The item schema is unavailable.".to_owned())
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
