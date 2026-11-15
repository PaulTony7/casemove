use app_commands::{InventoryItem, InventorySnapshot, SavedAccount, SteamProfile};
use eframe::egui;
use tokio::sync::mpsc::UnboundedSender;

/// Results of core calls and delivered events, sent back from spawned tasks.
pub enum Message {
    Initial {
        accounts: Result<Vec<SavedAccount>, String>,
        session: Result<Option<SteamProfile>, String>,
    },
    SchemaRefresh(Result<app_core::SchemaRefreshReport, String>),
    InventoryMetadata(u64, Result<Option<InventorySnapshot>, String>),
    Accounts(Result<Vec<SavedAccount>, String>),
    Deleted(Result<Vec<SavedAccount>, String>),
    Moved(Result<Vec<SavedAccount>, String>),
    Login(u64, Result<SteamProfile, String>),
    SignOut(Result<(), String>),
    Inventory(u64, Result<InventorySnapshot, String>),
    Storage {
        attempt: u64,
        session_attempt: u64,
        casket: String,
        result: Result<Vec<InventoryItem>, String>,
    },
    Image {
        url: String,
        result: Result<egui::ColorImage, ()>,
    },
    /// Fetched live Steam Market quotes as `(market_hash_name, price)` pairs.
    LivePrices(Vec<(String, f64)>),
    /// All storage units' contents as `(casket id, items)` pairs, `(attempt,
    /// result)`.
    StorageAll(u64, Result<Vec<(String, Vec<InventoryItem>)>, String>),
    /// Progress of the running move batch `(attempt, done, total)`.
    StorageMoveProgress(u64, usize, usize),
    /// Result of moving items into a storage unit.
    StorageMove {
        attempt: u64,
        moved: usize,
        total: usize,
        result: Result<(), String>,
    },
    /// Result of taking one item out of a storage unit.
    StorageWithdraw {
        attempt: u64,
        session_attempt: u64,
        casket: String,
        item_id: String,
        result: Result<(), String>,
    },
    /// Result of submitting a trade-up contract.
    TradeUpResult {
        attempt: u64,
        result: Result<(), String>,
    },
    /// Result of renaming a storage unit.
    StorageRename {
        attempt: u64,
        session_attempt: u64,
        casket: String,
        name: String,
        result: Result<(), String>,
    },
}

/// Clonable handle for sending a `Message` and waking the UI.
#[derive(Clone)]
pub struct Reply {
    tx: UnboundedSender<Message>,
    ctx: egui::Context,
}

impl Reply {
    pub fn new(tx: UnboundedSender<Message>, ctx: egui::Context) -> Self {
        Self { tx, ctx }
    }

    pub fn send(&self, message: Message) {
        if self.tx.send(message).is_ok() {
            self.ctx.request_repaint();
        }
    }
}
