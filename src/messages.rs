use app_commands::{InventoryItem, InventorySnapshot, SavedAccount, SteamProfile};
use eframe::egui;
use tokio::sync::mpsc::UnboundedSender;

/// Results of core calls and delivered events, sent back from spawned tasks.
pub enum Message {
    Initial {
        accounts: Result<Vec<SavedAccount>, String>,
        session: Result<Option<SteamProfile>, String>,
    },
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
