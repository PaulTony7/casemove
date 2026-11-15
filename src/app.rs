use crate::images::{self, ImageCache};
use crate::messages::{Message, Reply};
use crate::{
    icons, inventory_view, login, overview_view, settings_view, storage_view, theme, tradeup_view,
};
use app_commands::{
    BrowserTokenLoginParams, InventoryItem, InventorySnapshot, PasswordLoginParams, SavedAccount,
    SteamProfile, WalletInfo,
};
use app_core::{AppEvent, Core};
use eframe::egui;
use pricing::format_currency;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Clone, Copy, PartialEq)]
pub enum LoginMethod {
    Regular,
    Qr,
    WebToken,
}

#[derive(Default)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
    pub guard_code: String,
    pub shared_secret: String,
    pub browser_token: String,
    pub remember: bool,
    pub show_secret: bool,
    pub qr_remember: bool,
}

/// Build version shown in the sidebar and compared against GitHub releases.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Column sets for both tables plus the inventory sort, from the store.
/// Saved data from the pre-overhaul layout (`ui.columns` as a flag struct)
/// no longer deserializes, so it falls back to the defaults.
fn load_columns(
    core: &app_core::Core,
) -> (
    inventory_view::ColumnSet,
    inventory_view::ColumnSet,
    inventory_view::SortKey,
    bool,
) {
    let inventory = core
        .store_get::<inventory_view::ColumnSet>("ui.columns.inventory")
        .unwrap_or_default();
    let storage = core
        .store_get::<inventory_view::ColumnSet>("ui.columns.storage")
        .unwrap_or_default();
    let sort = core
        .store_get::<inventory_view::SortKey>("ui.sort")
        .unwrap_or_default();
    let sort_desc = core.store_get::<bool>("ui.sortDesc").unwrap_or(false);
    (inventory, storage, sort, sort_desc)
}

/// Bounds for the persisted UI scale.
const MIN_ZOOM: f32 = 0.6;
const MAX_ZOOM: f32 = 3.0;

#[derive(Default, Clone)]
pub struct MoveLine {
    pub name: String,
    pub ids: Vec<String>,
    pub quantity: usize,
}

#[derive(Default, Clone)]
pub struct MoveDialog {
    pub lines: Vec<MoveLine>,
    pub query: String,
    pub busy: bool,
    pub done: usize,
    pub total: usize,
    pub error: String,
}

#[derive(Default, Clone)]
pub struct RenameDialog {
    pub casket_id: String,
    pub current_name: String,
    pub name: String,
    pub busy: bool,
    pub error: String,
}

/// Trade-up page state: the contract inputs, picker filters, and the latest
/// outcomes for the current selection (legacy `tradeUpReducer`).
pub struct TradeUpState {
    pub contract: domain::Contract,
    /// Custom display names for candidate ids (search only, as legacy).
    pub custom_names: std::collections::HashMap<String, String>,
    pub outcomes: Vec<domain::Outcome>,
    pub query: String,
    pub min_float: f64,
    pub max_float: f64,
    pub hide_equipped: bool,
    pub selected_collections: Vec<String>,
    /// Snapshot of the filters the outcomes were computed with.
    pub filter: domain::ContractFilter,
    pub busy: bool,
    pub error: String,
    pub notice: String,
    /// Eligible inputs; refreshed each frame from the inventory and loaded
    /// storage units.
    pub candidates: Vec<domain::CandidateItem>,
}

impl Default for TradeUpState {
    fn default() -> Self {
        Self {
            contract: domain::Contract::default(),
            custom_names: std::collections::HashMap::new(),
            outcomes: Vec::new(),
            query: String::new(),
            min_float: 0.0,
            max_float: 1.0,
            hide_equipped: false,
            selected_collections: Vec::new(),
            filter: domain::ContractFilter::default(),
            busy: false,
            error: String::new(),
            notice: String::new(),
            candidates: Vec::new(),
        }
    }
}

/// Container-opening dialog: the chosen container and its key.
#[derive(Default, Clone)]
pub struct OpenContainerDialog {
    pub container_id: String,
    pub container_name: String,
    pub key_id: String,
    pub busy: bool,
    pub error: String,
}

/// State of the Steam-close prompt (legacy `closeSteamModal`).
#[derive(Default, Clone)]
pub struct SteamPrompt {
    /// Steam's PID, when it could be read and can be closed.
    pub pid: Option<u32>,
    pub error: String,
    pub dont_show: bool,
    pub closing: bool,
}

pub struct CasemoveApp {
    pub core: Core,
    pub rt: tokio::runtime::Runtime,
    pub reply: Reply,
    pub images: ImageCache,
    pub brand: Option<egui::TextureHandle>,
    rx: UnboundedReceiver<Message>,
    events_rx: UnboundedReceiver<AppEvent>,

    pub loading: bool,
    pub profile: Option<SteamProfile>,
    pub wallet: Option<WalletInfo>,

    pub method: LoginMethod,
    pub form: LoginForm,
    pub login_error: String,
    pub login_busy: bool,
    pub session_busy: bool,
    pub attempt: u64,
    pub qr_challenge: Option<String>,
    pub qr_texture: Option<(String, egui::TextureHandle)>,
    pub qr_error: String,

    pub accounts: Vec<SavedAccount>,
    pub account_error: String,
    pub account_order_busy: bool,
    pub pending_removal: Option<String>,
    pub removing: bool,

    pub inventory_open: bool,
    pub overview_open: bool,
    pub storage_open: bool,
    pub settings_open: bool,
    pub trade_up_open: bool,
    pub trade_up: TradeUpState,
    pub trade_up_review: bool,
    /// Latest release compared with this build (legacy `needUpdate`).
    pub update: app_core::UpdateStatus,
    /// Legacy "Steam is currently running" prompt, shown before a login only.
    pub steam_prompt: Option<SteamPrompt>,
    /// Whether this session already offered to close Steam (legacy
    /// `hasAskedCloseSteam`).
    pub steam_asked: bool,
    pub steam_checking: bool,
    /// Account whose saved login waits for the Steam check to finish.
    pending_saved_login: Option<String>,
    /// CM connection state from the core monitor.
    pub conn_state: app_core::ConnState,
    pub reconnecting: bool,
    /// Container-opening dialog (legacy `OpenContainer`).
    pub open_container: Option<OpenContainerDialog>,
    /// Message from the last failed update check (legacy logged it).
    pub update_error: String,
    /// Error to show inside the open review dialog.
    pub trade_up_dialog_error: Option<String>,
    pub overview_by: overview_view::OverviewBy,
    /// Display currency (legacy `pricing.currency`, default `USD`).
    pub currency_code: String,
    pub currency: pricing::Currency,
    /// Legacy `steamLogin` preference: prompt to close Steam when it is open.
    pub show_steam_login: bool,
    /// Legacy `devmode.value`: persisted only; legacy had no consumer either.
    pub dev_mode: bool,
    /// Persisted sidebar width and the storage-contents batch size.
    pub sidebar_width: f32,
    /// Last persisted UI scale, so zooming is written once it settles.
    pub saved_zoom: f32,
    /// Account row currently being dragged to reorder the saved list.
    pub drag_account: Option<String>,

    pub inventory: Option<InventorySnapshot>,
    pending_inventory_events: Vec<AppEvent>,
    pub grouped: Vec<crate::inventory::GroupedItem>,
    pub majors: Vec<String>,
    pub inventory_busy: bool,
    pub inventory_error: String,
    #[allow(dead_code)]
    pub schema_notice: String,
    pub storage_selection: Option<(String, String)>,
    pub storage_contents: Option<Vec<InventoryItem>>,
    pub storage_busy: bool,
    pub storage_error: String,
    pub storage_attempt: u64,
    /// Contents of every storage unit after "Load storage units" (legacy
    /// `storageInventory`).
    pub storage_aggregate: Option<Vec<InventoryItem>>,
    pub storage_aggregate_busy: bool,
    pub storage_aggregate_error: String,
    /// Contents per storage unit (keyed by unit id) after "Load storage
    /// units", used by the management view for per-unit values.
    pub storage_unit_contents: std::collections::BTreeMap<String, Vec<InventoryItem>>,
    pub storage_query: String,
    pub move_dialog: Option<MoveDialog>,
    pub rename_dialog: Option<RenameDialog>,
    /// Item id currently being withdrawn from the open storage unit.
    pub withdraw_busy: Option<String>,
    pub withdraw_error: String,
    /// Result of the last move/rename batch shown above the table.
    pub storage_notice: String,
    pub price_cache: pricing::LiveCache,
    pub prices_busy: bool,
    pub view: inventory_view::InventoryViewState,
    /// Filters/sort for the open storage unit's contents, kept apart from
    /// the inventory view so the two do not overwrite each other.
    pub storage_view: inventory_view::InventoryViewState,
    pub storage_grouped: Vec<crate::inventory::GroupedItem>,
    pub storage_majors: Vec<String>,
}

impl CasemoveApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        core: Core,
        rt: tokio::runtime::Runtime,
        tx: tokio::sync::mpsc::UnboundedSender<Message>,
        rx: UnboundedReceiver<Message>,
        events_rx: UnboundedReceiver<AppEvent>,
    ) -> Self {
        let reply = Reply::new(tx, cc.egui_ctx.clone());
        let images = ImageCache::new(rt.handle().clone(), reply.clone());
        let brand = icons::load_brand(&cc.egui_ctx);
        let overview_by = core
            .store_get::<String>("overview.by")
            .and_then(|value| overview_view::OverviewBy::from_store(&value))
            .unwrap_or_default();
        let sidebar_width = core
            .store_get::<f32>("ui.sidebar")
            .unwrap_or(216.0)
            .clamp(184.0, 320.0);
        // Reopen the last page, defaulting to the dashboard (`/stats`).
        let saved_view = core
            .store_get::<String>("ui.view")
            .unwrap_or_else(|| "overview".to_owned());
        let (inventory_open, overview_open, storage_open, settings_open, trade_up_open) =
            match saved_view.as_str() {
                "inventory" => (true, false, false, false, false),
                "storage" => (false, false, true, false, false),
                "settings" => (false, false, false, true, false),
                "tradeup" => (false, false, false, false, true),
                _ => (false, true, false, false, false),
            };
        let saved_storage_page = core.store_get::<usize>("ui.storage.page").unwrap_or(0);
        let currency_code = core
            .store_get::<String>("pricing.currency")
            .filter(|code| !code.trim().is_empty())
            .unwrap_or_else(|| "USD".to_owned());
        let show_steam_login = core.store_get::<bool>("steamLogin").unwrap_or(true);
        let dev_mode = core.store_get::<bool>("devmode.value").unwrap_or(false);
        let zoom = core
            .store_get::<f32>("ui.zoom")
            .filter(|zoom| zoom.is_finite())
            .unwrap_or(1.0)
            .clamp(MIN_ZOOM, MAX_ZOOM);
        cc.egui_ctx.options_mut(|options| options.zoom_factor = zoom);
        let (inventory_columns, storage_columns, sort, sort_desc) = load_columns(&core);
        let mut app = Self {
            core,
            rt,
            reply,
            images,
            brand,
            rx,
            events_rx,
            loading: true,
            profile: None,
            wallet: None,
            method: LoginMethod::Regular,
            form: LoginForm::default(),
            login_error: String::new(),
            login_busy: false,
            session_busy: false,
            attempt: 0,
            qr_challenge: None,
            qr_texture: None,
            qr_error: String::new(),
            accounts: Vec::new(),
            account_error: String::new(),
            account_order_busy: false,
            pending_removal: None,
            removing: false,
            inventory_open,
            overview_open,
            storage_open,
            settings_open,
            trade_up_open,
            trade_up: TradeUpState::default(),
            trade_up_review: false,
            trade_up_dialog_error: None,
            update: app_core::UpdateStatus::unchecked(APP_VERSION),
            steam_prompt: None,
            steam_asked: false,
            steam_checking: false,
            pending_saved_login: None,
            conn_state: app_core::ConnState::Connected,
            reconnecting: false,
            update_error: String::new(),
            open_container: None,
            overview_by,
            currency: pricing::Currency::new(&currency_code),
            currency_code,
            show_steam_login,
            dev_mode,
            sidebar_width,
            saved_zoom: zoom,
            drag_account: None,
            inventory: None,
            pending_inventory_events: Vec::new(),
            grouped: Vec::new(),
            majors: Vec::new(),
            inventory_busy: false,
            inventory_error: String::new(),
            schema_notice: "Refreshing item and sticker data; cached or bundled data remains available."
                .into(),
            storage_selection: None,
            storage_contents: None,
            storage_busy: false,
            storage_error: String::new(),
            storage_attempt: 0,
            storage_aggregate: None,
            storage_aggregate_busy: false,
            storage_aggregate_error: String::new(),
            storage_unit_contents: std::collections::BTreeMap::new(),
            storage_query: String::new(),
            move_dialog: None,
            rename_dialog: None,
            withdraw_busy: None,
            withdraw_error: String::new(),
            storage_notice: String::new(),
            price_cache: pricing::LiveCache::load(&prices_cache_path()),
            prices_busy: false,
            view: inventory_view::InventoryViewState {
                columns: inventory_columns,
                sort,
                sort_desc,
                ..Default::default()
            },
            storage_view: inventory_view::InventoryViewState {
                columns: storage_columns,
                page: saved_storage_page,
                ..Default::default()
            },
            storage_grouped: Vec::new(),
            storage_majors: Vec::new(),
        };
        app.initial_load();
        app.check_for_update();
        app
    }

    /// Legacy `needUpdate` on startup, with a manual re-check from the
    /// status strip.
    pub fn check_for_update(&mut self) {
        self.update_error.clear();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let result = app_core::check_update(APP_VERSION).await;
            reply.send(Message::Update(result));
        });
    }

    fn initial_load(&mut self) {
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let accounts = core.store_get_accounts();
            let session = core.steam_session();
            reply.send(Message::Initial { accounts, session });
        });
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::SchemaRefresh(core.refresh_item_schema().await));
        });
    }

    // --- message handling -------------------------------------------------

    fn handle_event(&mut self, event: AppEvent) {
        match event {
            AppEvent::QrChallenge(challenge) => {
                self.qr_challenge = Some(challenge);
                self.qr_error.clear();
            }
            AppEvent::WalletChanged(info) => {
                self.wallet = Some(info);
            }
            AppEvent::InventoryChanged {
                steam_id,
                reason,
                item_id,
                item,
            } => {
                if self
                    .profile
                    .as_ref()
                    .is_some_and(|profile| profile.steam_id == steam_id)
                    && !self.apply_inventory_delta(&steam_id, &item_id, item.as_deref().cloned())
                {
                    self.pending_inventory_events
                        .push(AppEvent::InventoryChanged {
                            steam_id,
                            reason,
                            item_id,
                            item,
                        });
                }
            }
            AppEvent::ConnectionChanged(state) => {
                // The core monitor reports the CM's logon state; a fresh
                // attach reloads the inventory once it succeeds.
                let recovered = matches!(state, app_core::ConnState::Connected)
                    && !matches!(self.conn_state, app_core::ConnState::Connected);
                self.conn_state = state;
                if recovered {
                    self.refresh_inventory();
                }
            }
            AppEvent::InventoryError(error) => {
                self.inventory_error = error;
            }
        }
    }

    fn handle(&mut self, ctx: &egui::Context, message: Message) {
        match message {
            Message::Initial { accounts, session } => {
                match accounts {
                    Ok(list) => self.accounts = list,
                    Err(error) => self.account_error = error,
                }
                match session {
                    Ok(value) => {
                        self.wallet = value.as_ref().and_then(|profile| profile.wallet.clone());
                        self.profile = value;
                    }
                    Err(error) => self.login_error = error,
                }
                self.loading = false;
                // Connecting is the normal path: load the inventory as soon
                // as a session exists instead of waiting for a button.
                if self.profile.is_some() {
                    self.refresh_inventory();
                } else if let Some(account) = self
                    .accounts
                    .iter()
                    .rev()
                    .find(|account| account.has_saved_login)
                    .map(|account| account.username.clone())
                {
                    // No live session: resume the most recently used saved
                    // login, so opening the app lands signed in. It still
                    // answers the Steam-close prompt first.
                    self.login_saved(account);
                }
            }
            Message::SchemaRefresh(result) => match result {
                Ok(_report) => {
                    if self.inventory.is_some() && self.storage_selection.is_none() {
                        let core = self.core.clone();
                        let reply = self.reply.clone();
                        let attempt = self.attempt;
                        self.rt.spawn(async move {
                            reply.send(Message::InventoryMetadata(
                                attempt,
                                core.refresh_inventory_metadata().await,
                            ));
                        });
                    }
                }
                Err(error) => {
                    self.inventory_error = format!(
                        "Could not refresh item data; cached or bundled data remains active. {error}"
                    );
                }
            },
            Message::InventoryMetadata(attempt, result) => {
                if attempt == self.attempt && !self.inventory_busy {
                    match result {
                        Ok(Some(snapshot))
                            if self
                                .profile
                                .as_ref()
                                .is_some_and(|profile| profile.steam_id == snapshot.steam_id) =>
                        {
                            self.set_inventory(Some(snapshot));
                            self.refresh_live_prices(false);
                        }
                        Ok(Some(_)) => {
                            self.inventory_error =
                                "The Steam session changed. Load inventory again.".into();
                        }
                        Ok(None) => {}
                        Err(error) => self.inventory_error = error,
                    }
                }
            }
            Message::Accounts(result) => match result {
                Ok(list) => {
                    self.accounts = list;
                    self.account_error.clear();
                }
                Err(error) => self.account_error = error,
            },
            Message::Deleted(result) => {
                self.removing = false;
                self.pending_removal = None;
                match result {
                    Ok(list) => {
                        self.accounts = list;
                        self.account_error.clear();
                        self.form.username.clear();
                    }
                    Err(error) => self.account_error = error,
                }
            }
            Message::Moved(result) => {
                self.account_order_busy = false;
                match result {
                    Ok(list) => {
                        self.accounts = list;
                        self.account_error.clear();
                    }
                    Err(error) => self.account_error = error,
                }
            }
            Message::Login(attempt, result) => {
                if attempt == self.attempt {
                    match result {
                        Ok(profile) => self.complete_login(profile),
                        Err(error) => self.login_error = error,
                    }
                    self.login_busy = false;
                }
            }
            Message::SignOut(result) => {
                match result {
                    Ok(()) => {
                        self.profile = None;
                        self.wallet = None;
                        self.login_error.clear();
                    }
                    Err(error) => self.login_error = error,
                }
                self.login_busy = false;
                self.session_busy = false;
            }
            Message::Inventory(attempt, result) => {
                if attempt == self.attempt {
                    match result {
                        Ok(snapshot) => {
                            if self
                                .profile
                                .as_ref()
                                .is_some_and(|profile| profile.steam_id == snapshot.steam_id)
                            {
                                self.set_inventory(Some(snapshot));
                                let pending = std::mem::take(&mut self.pending_inventory_events);
                                for event in pending {
                                    self.handle_event(event);
                                }
                                self.refresh_live_prices(false);
                            } else {
                                self.inventory_error =
                                    "The Steam session changed. Load inventory again.".into();
                            }
                        }
                        Err(error) => self.inventory_error = error,
                    }
                    self.inventory_busy = false;
                }
            }
            Message::Storage {
                attempt,
                session_attempt,
                casket,
                result,
            } => {
                if attempt == self.storage_attempt
                    && session_attempt == self.attempt
                    && self
                        .storage_selection
                        .as_ref()
                        .is_some_and(|(selected, _)| *selected == casket)
                {
                    match result {
                        Ok(items) => {
                            self.storage_grouped =
                                crate::inventory::group_items(items.clone());
                            let mut majors: Vec<String> = self
                                .storage_grouped
                                .iter()
                                .filter_map(|row| row.representative.major.clone())
                                .collect();
                            majors.sort();
                            majors.dedup();
                            self.storage_majors = majors;
                            self.storage_contents = Some(items.clone());
                            self.storage_unit_contents.insert(casket, items);
                            self.storage_view = inventory_view::InventoryViewState::default();
                            self.refresh_live_prices(false);
                        }
                        Err(error) => self.storage_error = error,
                    }
                    self.storage_busy = false;
                }
            }
            Message::LivePrices(prices) => {
                self.prices_busy = false;
                if !prices.is_empty() {
                    let now = now_ms();
                    for (key, price) in prices {
                        self.price_cache.insert(&key, price, now);
                    }
                    let _ = self.price_cache.save(&prices_cache_path());
                    self.apply_live_prices();
                }
            }
            Message::Update(result) => match result {
                Ok(status) => self.update = status,
                Err(error) => self.update_error = error,
            },
            Message::SteamRunning(result) => {
                self.steam_checking = false;
                let waiting = self.pending_saved_login.take();
                match result {
                    Ok(pid) => {
                        self.steam_prompt = Some(SteamPrompt {
                            pid,
                            ..Default::default()
                        });
                    }
                    Err(error) => self.login_error = error,
                }
                // Steam is not running, so the waiting sign-in skips the
                // prompt (it was only opened because the check was pending).
                if self.steam_prompt.as_ref().is_some_and(|prompt| prompt.pid.is_none())
                    && let Some(account) = waiting
                {
                    self.steam_prompt = None;
                    self.continue_saved_login(account);
                }
            }
            Message::SteamClosed(result) => {
                if self.steam_prompt.is_none() {
                    return;
                }
                match result {
                    Ok(()) => {
                        // Legacy asked once whether to close Steam, then
                        // logged in regardless of the result.
                        self.steam_prompt = None;
                        self.submit_login();
                    }
                    Err(error) => self.report_steam_still_running(error),
                }
            }
            Message::StorageAll(attempt, result) => {
                self.storage_aggregate_busy = false;
                if attempt == self.attempt {
                    match result {
                        Ok(units) => {
                            self.storage_aggregate = Some(
                                units
                                    .iter()
                                    .flat_map(|(_, items)| items.iter().cloned())
                                    .collect(),
                            );
                            self.storage_unit_contents = units.into_iter().collect();
                            self.storage_aggregate_error.clear();
                            self.apply_live_prices();
                            self.refresh_live_prices(false);
                        }
                        Err(error) => self.storage_aggregate_error = error,
                    }
                }
            }
            Message::TradeUpResult { attempt, result } => {
                if attempt != self.attempt {
                    return;
                }
                self.trade_up.busy = false;
                match result {
                    Ok(()) => {
                        self.trade_up_review = false;
                        self.trade_up.error.clear();
                        self.trade_up.notice =
                            "Trade-up submitted. The new item appears when the GC answers."
                                .to_owned();
                        self.trade_up.contract = domain::Contract::default();
                        self.trade_up.outcomes.clear();
                        self.trade_up.selected_collections.clear();
                    }
                    Err(error) => {
                        self.trade_up.error = error.clone();
                        if let Some(dialog) = &mut self.trade_up_dialog_error {
                            dialog.clone_from(&error);
                        }
                    }
                }
            }
            Message::StorageMove {
                attempt,
                moved,
                total,
                result,
            } => {
                if attempt != self.attempt {
                    return;
                }
                let lines = self
                    .move_dialog
                    .as_ref()
                    .map(|dialog| dialog.lines.clone())
                    .unwrap_or_default();
                match result {
                    Ok(()) => {
                        self.move_dialog = None;
                        self.withdraw_error.clear();
                        self.storage_notice = format!(
                            "Moved {moved} {} to storage.",
                            if moved == 1 { "item" } else { "items" }
                        );
                        self.remove_selected(&lines);
                        // Counts and contents changed; load them on demand.
                        self.storage_aggregate = None;
                        self.storage_unit_contents.clear();
                        self.refresh_inventory();
                    }
                    Err(error) => {
                        if total == 1 {
                            if let Some(dialog) = &mut self.move_dialog {
                                dialog.busy = false;
                                dialog.error = error;
                            }
                        } else {
                            self.move_dialog = None;
                            self.remove_selected(&lines);
                            self.storage_notice = format!(
                                "Moved {moved} of {total} items. {error}"
                            );
                            self.storage_aggregate = None;
                            self.storage_unit_contents.clear();
                            self.refresh_inventory();
                        }
                    }
                }
            }
            Message::StorageMoveProgress(attempt, done, total) => {
                if attempt == self.attempt
                    && let Some(dialog) = &mut self.move_dialog
                {
                    dialog.done = done;
                    dialog.total = total;
                }
            }
            Message::StorageWithdraw {
                attempt,
                session_attempt,
                casket,
                item_id,
                result,
            } => {
                if attempt != self.attempt || session_attempt != self.attempt {
                    return;
                }
                self.withdraw_busy = None;
                if self
                    .storage_selection
                    .as_ref()
                    .is_some_and(|(selected, _)| *selected == casket)
                {
                    match result {
                        Ok(()) => {
                            self.withdraw_error.clear();
                            self.storage_notice = "Moved item back to inventory.".into();
                            self.remove_storage_item(&casket, &item_id);
                            self.refresh_inventory();
                        }
                        Err(error) => self.withdraw_error = error,
                    }
                }
            }
            Message::Reconnect(result) => {
                self.reconnecting = false;
                match result {
                    Ok(()) => {
                        self.storage_notice = "Reconnected to the CS2 Game Coordinator.".to_owned();
                        self.refresh_inventory();
                    }
                    Err(error) => self.login_error = error,
                }
            }
            Message::ContainerOpened {
                attempt,
                container,
                result,
            } => {
                if attempt != self.attempt {
                    return;
                }
                match result {
                    Ok(()) => {
                        self.open_container = None;
                        self.storage_notice = format!(
                            "Opening {container}. The new item appears once the Game Coordinator answers."
                        );
                        self.refresh_inventory();
                    }
                    Err(error) => {
                        if let Some(dialog) = &mut self.open_container {
                            dialog.busy = false;
                            dialog.error = error;
                        }
                    }
                }
            }
            Message::StorageRename {
                attempt,
                session_attempt,
                casket,
                name,
                result,
            } => {
                if attempt != self.attempt || session_attempt != self.attempt {
                    return;
                }
                match result {
                    Ok(()) => {
                        self.rename_dialog = None;
                        self.storage_notice = format!("Renamed storage unit to {name}.");
                        if let Some(item) = self
                            .inventory
                            .as_mut()
                            .and_then(|snapshot| {
                                snapshot
                                    .items
                                    .iter_mut()
                                    .find(|item| item.id == casket)
                            })
                        {
                            item.custom_name = Some(name.clone());
                        }
                        if let Some((selected, current)) = &mut self.storage_selection
                            && *selected == casket
                        {
                            *current = name;
                        }
                        self.refresh_inventory();
                    }
                    Err(error) => {
                        if let Some(dialog) = &mut self.rename_dialog {
                            dialog.busy = false;
                            dialog.error = error;
                        }
                    }
                }
            }
            Message::Image { url, result } => self.images.insert(ctx, url, result),
        }
    }

    fn remove_selected(&mut self, lines: &[MoveLine]) {
        for line in lines {
            self.view.selected.remove(&line.ids[0]);
        }
    }

    /// Drops an item from the open storage unit's local lists so the table
    /// reflects the withdrawal before the refreshed inventory arrives.
    fn remove_storage_item(&mut self, casket: &str, item_id: &str) {
        if let Some(items) = &mut self.storage_contents {
            items.retain(|item| item.id != item_id);
            self.storage_grouped = crate::inventory::group_items(items.clone());
            self.storage_majors = self
                .storage_grouped
                .iter()
                .filter_map(|row| row.representative.major.clone())
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect();
        }
        if let Some(items) = self.storage_unit_contents.get_mut(casket) {
            items.retain(|item| item.id != item_id);
        }
        if let Some(items) = &mut self.storage_aggregate {
            items.retain(|item| item.id != item_id);
        }
        if let Some(unit) = self
            .inventory
            .as_mut()
            .and_then(|snapshot| snapshot.items.iter_mut().find(|item| item.id == casket))
            && let Some(count) = unit.storage_item_count
        {
            unit.storage_item_count = Some(count.saturating_sub(1));
        }
    }

    fn set_inventory(&mut self, snapshot: Option<InventorySnapshot>) {
        self.grouped = snapshot
            .as_ref()
            .map(|snapshot| crate::inventory::group_items(snapshot.items.clone()))
            .unwrap_or_default();
        let mut majors: Vec<String> = self
            .grouped
            .iter()
            .filter_map(|row| row.representative.major.clone())
            .collect();
        majors.sort();
        majors.dedup();
        self.majors = majors;
        let present: std::collections::HashSet<String> = self
            .grouped
            .iter()
            .map(|row| row.representative.id.clone())
            .collect();
        self.view.selected.retain(|id, _| present.contains(id));
        self.inventory = snapshot;
        self.sync_trade_up_selection();
        // Contents may have changed with the new snapshot; reload on demand.
        self.storage_aggregate = None;
        self.storage_unit_contents.clear();
        self.storage_aggregate_error.clear();
    }

    /// Rebuilds the eligible input list (legacy `tradeUpConfirmed` rows).
    pub fn refresh_trade_up_candidates(&mut self) {
        let mut candidates = Vec::new();
        if let Some(snapshot) = &self.inventory {
            candidates.extend(
                snapshot
                    .items
                    .iter()
                    .filter_map(tradeup_view::candidate_from),
            );
        }
        for items in self.storage_unit_contents.values() {
            candidates.extend(items.iter().filter_map(tradeup_view::candidate_from));
        }
        self.trade_up.candidates = candidates;
    }

    /// Keeps the contract in step with the inventory: inputs that vanish
    /// (traded up, moved, sold) leave the contract and the outcomes refresh.
    fn sync_trade_up_selection(&mut self) {
        self.refresh_trade_up_candidates();
        let Some(snapshot) = &self.inventory else {
            return;
        };
        let present: std::collections::HashSet<&str> =
            snapshot.items.iter().map(|item| item.id.as_str()).collect();
        let before = self.trade_up.contract.items.len();
        self.trade_up
            .contract
            .items
            .retain(|item| present.contains(item.id.as_str()));
        if self.trade_up.contract.items.len() != before || self.trade_up.outcomes.is_empty() {
            self.recompute_trade_up();
        }
    }

    fn apply_inventory_delta(
        &mut self,
        steam_id: &str,
        item_id: &str,
        item: Option<InventoryItem>,
    ) -> bool {
        let Some(snapshot) = &mut self.inventory else {
            return false;
        };
        if snapshot.steam_id != steam_id {
            return false;
        }
        let previous = snapshot
            .items
            .iter()
            .find(|current| current.id == item_id)
            .cloned();
        if item.as_ref() == previous.as_ref() {
            return true;
        }
        snapshot.items.retain(|current| current.id != item_id);
        if let Some(item) = item {
            snapshot.items.push(item);
        }
        snapshot
            .items
            .sort_by_key(|item| (item.position, item.id.clone()));
        let snapshot = snapshot.clone();
        self.set_inventory(Some(snapshot));
        self.sync_trade_up_selection();
        self.refresh_live_prices(false);
        true
    }

    /// Fetches live Steam Market quotes for every distinct moveable item
    /// name (inventory plus open storage contents), skipping keys the cache
    /// still holds fresh unless `force` — the Refresh prices button. With
    /// nothing to fetch the cached quotes are re-applied straight away.
    pub fn refresh_live_prices(&mut self, force: bool) {
        if self.prices_busy || self.inventory.is_none() {
            return;
        }
        let now = now_ms();
        let mut keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for item in self
            .inventory
            .iter()
            .flat_map(|snapshot| &snapshot.items)
            .chain(self.storage_contents.iter().flatten())
            .chain(self.storage_aggregate.iter().flatten())
        {
            if item.moveable {
                keys.insert(pricing::live_key(&item.name, item.wear_name.as_deref()));
            }
        }
        let keys: Vec<String> = keys
            .into_iter()
            .filter(|key| force || !self.price_cache.fresh(key, now))
            .collect();
        if keys.is_empty() {
            self.apply_live_prices();
            return;
        }
        self.prices_busy = true;
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::LivePrices(app_core::fetch_live_prices(keys).await));
        });
    }

    /// Re-resolves every visible price from the snapshot plus the live cache.
    fn apply_live_prices(&mut self) {
        if let Some(snapshot) = &mut self.inventory {
            for item in &mut snapshot.items {
                reprice(item, &self.price_cache);
            }
        }
        for row in &mut self.grouped {
            reprice(&mut row.representative, &self.price_cache);
        }
        if let Some(items) = &mut self.storage_contents {
            for item in items {
                reprice(item, &self.price_cache);
            }
        }
        if let Some(items) = &mut self.storage_aggregate {
            for item in items {
                reprice(item, &self.price_cache);
            }
        }
        for items in self.storage_unit_contents.values_mut() {
            for item in items {
                reprice(item, &self.price_cache);
            }
        }
        for row in &mut self.storage_grouped {
            reprice(&mut row.representative, &self.price_cache);
        }
    }

    /// Fetches every storage unit's contents (legacy's overview Load button).
    pub fn load_all_storage(&mut self) {
        if self.storage_aggregate_busy || self.session_busy || self.inventory.is_none() {
            return;
        }
        let ids: Vec<String> = self
            .inventory
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .items
                    .iter()
                    .filter(|item| item.is_storage_unit)
                    .map(|item| item.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        if ids.is_empty() {
            self.storage_aggregate = Some(Vec::new());
            self.storage_unit_contents.clear();
            self.storage_aggregate_error.clear();
            return;
        }
        self.storage_aggregate_busy = true;
        self.storage_aggregate_error.clear();
        let attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::StorageAll(
                attempt,
                core.all_storage_contents(ids).await,
            ));
        });
    }

    /// Persists the overview's price/volume grouping choice.
    pub fn save_overview_by(&self) {
        let core = self.core.clone();
        let value = self.overview_by.as_str().to_owned();
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("overview.by", value);
        }));
    }

    /// Persists the legacy dev-mode toggle (`devmode.value`).
    pub fn set_dev_mode(&mut self, value: bool) {
        self.dev_mode = value;
        let core = self.core.clone();
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("devmode.value", value);
        }));
    }

    /// Persists the Steam-close popup preference.
    pub fn save_steam_login(&self) {
        let core = self.core.clone();
        let value = self.show_steam_login;
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("steamLogin", value);
        }));
    }

    /// Changes the display currency: prices re-render with the new rate.
    pub fn set_currency(&mut self, code: &str) {
        self.currency_code = code.to_owned();
        self.currency = pricing::Currency::new(code);
        let core = self.core.clone();
        let value = self.currency_code.clone();
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("pricing.currency", value);
        }));
    }

    /// Formats a USD amount in the selected display currency.
    pub fn money(&self, amount: f64) -> String {
        format_currency(&self.currency, amount, 2)
    }

    /// Writes the legacy `casemove.csv` report (inventory plus every loaded
    /// storage unit) to the Downloads folder and reveals it.
    pub fn export_csv(&mut self) {
        let mut rows: Vec<crate::export::Row> = Vec::new();
        for row in &self.grouped {
            rows.push(crate::export::Row {
                grouped: row,
                storage_name: None,
                trade_locked: row.representative.trade_unlock.is_some(),
            });
        }
        let mut reports: Vec<(String, Vec<crate::inventory::GroupedItem>)> = Vec::new();
        if let Some(units) = &self.inventory {
            for unit in units.items.iter().filter(|item| item.is_storage_unit) {
                let Some(contents) = self.storage_unit_contents.get(&unit.id) else {
                    continue;
                };
                let name = unit
                    .custom_name
                    .clone()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| unit.name.clone());
                reports.push((name, crate::inventory::group_items(contents.clone())));
            }
        }
        for (name, grouped) in &reports {
            for row in grouped {
                rows.push(crate::export::Row {
                    grouped: row,
                    storage_name: Some(name),
                    trade_locked: row.representative.trade_unlock.is_some(),
                });
            }
        }
        let csv = crate::export::export(&rows, &self.currency);
        let path = downloads_dir().join("casemove.csv");
        self.storage_notice = match write_report(&path, &csv) {
            Ok(()) => format!("Saved {}", path.display()),
            Err(error) => format!("Could not save the report: {error}"),
        };
    }

    /// Persists the visible columns of one table so they survive restarts.
    pub fn save_columns(&self, table: inventory_view::TableId, columns: &inventory_view::ColumnSet) {
        let core = self.core.clone();
        let key = format!("ui.columns.{}", table.store_key());
        let columns = columns.clone();
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set(&key, columns);
        }));
    }

    /// Persists the UI scale so zoom survives a restart.
    /// Persists the UI scale so zoom survives a restart.
    pub fn save_zoom(&self, zoom: f32) {
        let core = self.core.clone();
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("ui.zoom", zoom);
        }));
    }

    /// Persists which page of the storage contents is on screen.
    pub fn save_storage_page(&self) {
        let core = self.core.clone();
        let page = self.storage_view.page;
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("ui.storage.page", page);
        }));
    }

    /// Persists the sidebar width once the user stops dragging it.
    pub fn persist_sidebar_width(&mut self, width: f32, ctx: &egui::Context) {
        if width < 50.0 || (width - self.sidebar_width).abs() < 0.5 {
            return;
        }
        if ctx.input(|input| input.pointer.any_down()) {
            return;
        }
        self.sidebar_width = width;
        let core = self.core.clone();
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("ui.sidebar", width);
        }));
    }

    // --- actions ----------------------------------------------------------

    pub fn switch_method(&mut self, method: LoginMethod) {
        if self.login_busy || self.session_busy || self.method == method {
            return;
        }
        self.form.password.clear();
        self.form.guard_code.clear();
        self.form.shared_secret.clear();
        self.form.browser_token.clear();
        self.qr_challenge = None;
        self.qr_texture = None;
        self.qr_error.clear();
        self.form.show_secret = false;
        self.steam_prompt = None;
        self.steam_asked = false;
        self.login_error.clear();
        self.method = method;
    }

    pub fn select_account(&mut self, username: String) {
        if self.login_busy || self.session_busy || self.profile.is_some() {
            return;
        }
        self.form.username = username;
        self.method = LoginMethod::Regular;
    }

    pub fn login_saved(&mut self, account: String) {
        if self.login_busy || self.session_busy || self.profile.is_some() {
            return;
        }
        self.form.username = account.clone();
        self.method = LoginMethod::Regular;
        self.login_error.clear();
        self.qr_challenge = None;
        self.qr_texture = None;
        self.qr_error.clear();
        self.form.password.clear();
        self.form.guard_code.clear();
        self.form.shared_secret.clear();
        self.form.show_secret = false;
        // A saved-account sign-in asks about Steam exactly like a typed one.
        if self.steam_gate() {
            self.pending_saved_login = Some(account);
            return;
        }
        self.begin_saved_login(account);
    }

    fn begin_saved_login(&mut self, account: String) {
        self.login_busy = true;
        self.attempt += 1;
        let attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::Login(
                attempt,
                core.steam_login_saved(account).await,
            ));
        });
    }

    /// The Steam-close prompt every regular login answers first (legacy
    /// `loginForm.onSubmit`). Returns `true` when it opened; the login resumes
    /// from the prompt.
    fn steam_gate(&mut self) -> bool {
        if self.steam_asked || !self.show_steam_login || self.steam_prompt.is_some() {
            return false;
        }
        self.steam_asked = true;
        self.check_steam();
        true
    }

    pub fn submit_login(&mut self) {
        if self.loading || self.login_busy || self.session_busy || self.profile.is_some() {
            return;
        }
        // Legacy `loginForm.onSubmit`: before the first regular login of the
        // session, offer to close a running Steam client when the
        // `steamLogin` preference is on. QR/web-token logins skip the check.
        if self.method == LoginMethod::Regular && self.steam_gate() {
            return;
        }
        match self.method {
            LoginMethod::Qr => {
                self.qr_challenge = None;
                self.qr_texture = None;
                self.qr_error.clear();
                self.login_error.clear();
                self.login_busy = true;
                self.attempt += 1;
                let attempt = self.attempt;
                let remember = self.form.qr_remember;
                let core = self.core.clone();
                let reply = self.reply.clone();
                self.rt.spawn(async move {
                    reply.send(Message::Login(
                        attempt,
                        core.steam_start_qr_login(remember).await,
                    ));
                });
            }
            LoginMethod::WebToken => {
                let params = BrowserTokenLoginParams {
                    payload: std::mem::take(&mut self.form.browser_token),
                };
                self.qr_challenge = None;
                self.qr_texture = None;
                self.login_error.clear();
                self.login_busy = true;
                self.attempt += 1;
                let attempt = self.attempt;
                let core = self.core.clone();
                let reply = self.reply.clone();
                self.rt.spawn(async move {
                    reply.send(Message::Login(
                        attempt,
                        core.steam_login_webtoken(params).await,
                    ));
                });
            }
            LoginMethod::Regular => {
                // The legacy form relied on HTML `required` validation.
                if self.form.username.trim().is_empty() || self.form.password.is_empty() {
                    self.login_error = "Enter your Steam username and password.".into();
                    return;
                }
                if self.form.show_secret && self.form.shared_secret.is_empty() {
                    self.login_error =
                        "Enter a Base64 shared secret or turn off the secret field.".into();
                    return;
                }
                let guard_code = {
                    let code = std::mem::take(&mut self.form.guard_code);
                    (!code.trim().is_empty()).then_some(code)
                };
                let shared_secret = self
                    .form
                    .show_secret
                    .then(|| std::mem::take(&mut self.form.shared_secret));
                let params = PasswordLoginParams {
                    account: self.form.username.trim().to_owned(),
                    password: std::mem::take(&mut self.form.password),
                    guard_code,
                    shared_secret,
                    remember: self.form.remember,
                };
                self.qr_challenge = None;
                self.qr_texture = None;
                self.login_error.clear();
                self.login_busy = true;
                self.attempt += 1;
                let attempt = self.attempt;
                let core = self.core.clone();
                let reply = self.reply.clone();
                self.rt.spawn(async move {
                    reply.send(Message::Login(attempt, core.steam_login(params).await));
                });
            }
        }
    }

    fn complete_login(&mut self, value: SteamProfile) {
        self.login_error = value.notice.clone().unwrap_or_default();
        self.qr_challenge = None;
        self.qr_texture = None;
        self.qr_error.clear();
        self.wallet = value.wallet.clone();
        self.profile = Some(value);
        // The app always wants a connected inventory, so start loading it
        // right after sign-in rather than asking for another click.
        self.refresh_inventory();
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::Accounts(core.store_get_accounts()));
        });
    }

    pub fn sign_out(&mut self) {
        if self.session_busy {
            return;
        }
        self.attempt += 1;
        self.storage_attempt += 1;
        self.form.shared_secret.clear();
        self.form.show_secret = false;
        self.qr_challenge = None;
        self.qr_texture = None;
        self.qr_error.clear();
        self.pending_inventory_events.clear();
        self.set_inventory(None);
        self.inventory_open = false;
        self.overview_open = false;
        self.storage_open = false;
        self.inventory_error.clear();
        self.inventory_busy = false;
        self.storage_selection = None;
        self.storage_contents = None;
        self.storage_grouped.clear();
        self.storage_majors.clear();
        self.storage_busy = false;
        self.storage_error.clear();
        self.storage_unit_contents.clear();
        self.move_dialog = None;
        self.rename_dialog = None;
        self.withdraw_busy = None;
        self.withdraw_error.clear();
        self.storage_notice.clear();
        self.session_busy = true;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::SignOut(core.steam_sign_out().await));
        });
    }

    pub fn refresh_inventory(&mut self) {
        if self.inventory_busy || self.session_busy || self.profile.is_none() {
            return;
        }
        let attempt = self.attempt;
        self.inventory_busy = true;
        self.inventory_error.clear();
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::Inventory(attempt, core.refresh_inventory().await));
        });
    }

    pub fn begin_move_to_storage(&mut self, item_id: String, item_name: String) {
        if self.session_busy || self.profile.is_none() {
            return;
        }
        self.storage_notice.clear();
        self.move_dialog = Some(MoveDialog {
            lines: vec![MoveLine {
                name: item_name,
                ids: vec![item_id],
                quantity: 1,
            }],
            ..Default::default()
        });
    }

    pub fn begin_bulk_move(&mut self, lines: Vec<MoveLine>) {
        if self.session_busy || self.profile.is_none() || lines.is_empty() {
            return;
        }
        self.storage_notice.clear();
        self.move_dialog = Some(MoveDialog {
            lines,
            ..Default::default()
        });
    }

    pub fn move_item_to_storage(&mut self, casket_id: String) {
        let Some(dialog) = &mut self.move_dialog else {
            return;
        };
        if dialog.busy {
            return;
        }
        dialog.busy = true;
        dialog.error.clear();
        let mut ids = Vec::new();
        for line in &dialog.lines {
            ids.extend(line.ids.iter().take(line.quantity).cloned());
        }
        if ids.is_empty() {
            dialog.busy = false;
            dialog.error = "Select at least one item to move.".into();
            return;
        }
        dialog.done = 0;
        dialog.total = ids.len();
        let total = ids.len();
        let attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let mut moved = 0;
            let mut first_error = None;
            for item_id in ids {
                match core.move_to_storage(casket_id.clone(), item_id).await {
                    Ok(()) => moved += 1,
                    Err(error) => {
                        if first_error.is_none() {
                            first_error = Some(error);
                        }
                    }
                }
                reply.send(Message::StorageMoveProgress(attempt, moved, total));
            }
            reply.send(Message::StorageMove {
                attempt,
                moved,
                total,
                result: match first_error {
                    Some(error) => Err(error),
                    None => Ok(()),
                },
            });
        });
    }

    /// Takes one item out of the open storage unit.
    pub fn withdraw_item(&mut self, item_id: String) {
        let Some((casket_id, _)) = self.storage_selection.clone() else {
            return;
        };
        if self.session_busy || self.profile.is_none() || self.withdraw_busy.is_some() {
            return;
        }
        self.withdraw_busy = Some(item_id.clone());
        self.withdraw_error.clear();
        self.storage_notice.clear();
        let attempt = self.attempt;
        let session_attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let result = core.move_from_storage(casket_id.clone(), item_id.clone()).await;
            reply.send(Message::StorageWithdraw {
                attempt,
                session_attempt,
                casket: casket_id,
                item_id,
                result,
            });
        });
    }

    pub fn begin_rename_storage(&mut self, casket_id: String, current_name: String) {
        if self.session_busy || self.profile.is_none() {
            return;
        }
        self.rename_dialog = Some(RenameDialog {
            casket_id,
            name: current_name.clone(),
            current_name,
            ..Default::default()
        });
        self.storage_notice.clear();
    }

    /// Adds one picker row to the contract (legacy `tradeUpAddRemove`).
    pub fn add_trade_up_candidate(
        &mut self,
        item: domain::CandidateItem,
        custom_name: Option<String>,
    ) {
        let id = item.id.clone();
        if self.trade_up.contract.contains(&id) {
            return;
        }
        if let Some(name) = custom_name.filter(|name| !name.is_empty()) {
            self.trade_up.custom_names.insert(id.clone(), name);
        }
        self.trade_up.contract.toggle(item);
        self.recompute_trade_up();
    }

    pub fn remove_trade_up_item(&mut self, id: &str) {
        self.trade_up.contract.remove(id);
        self.recompute_trade_up();
    }

    /// Recomputes the possible outcomes (legacy recomputed them whenever the
    /// input count changed).
    fn recompute_trade_up(&mut self) {
        let inputs: Vec<(String, Option<f64>)> = self
            .trade_up
            .contract
            .items
            .iter()
            .map(|item| (item.name.clone(), item.wear))
            .collect();
        self.trade_up.outcomes = domain::outcomes(&inputs).unwrap_or_default();
        self.trade_up.notice.clear();
    }

    /// Submits the contract to the GC.
    pub fn submit_trade_up(&mut self) {
        if self.trade_up.busy || !self.trade_up.contract.is_full() {
            return;
        }
        self.trade_up.busy = true;
        self.trade_up.error.clear();
        self.trade_up_dialog_error = None;
        let ids: Vec<String> = self
            .trade_up
            .contract
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect();
        let attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let result = core.craft_trade_up(ids).await;
            reply.send(Message::TradeUpResult { attempt, result });
        });
    }

    /// Legacy `retryConnection`: re-runs the GC handshake and inventory load.
    pub fn retry_connection(&mut self) {
        if self.reconnecting || self.session_busy || self.profile.is_none() {
            return;
        }
        self.reconnecting = true;
        self.login_error.clear();
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let result = match core.session_generation() {
                Some(generation) => app_core::reconnect(&core, generation).await,
                None => Err("Sign in to Steam first.".to_owned()),
            };
            reply.send(Message::Reconnect(result));
        });
    }

    /// Legacy `checkSteam`: looks for a running Steam client and prompts.
    pub fn check_steam(&mut self) {
        self.steam_checking = true;
        self.login_error.clear();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::SteamRunning(crate::steam::running().await));
        });
    }

    /// Dismisses the prompt and drops the per-session latch, so a later
    /// sign-in asks again (legacy remounted the login form).
    pub fn dismiss_steam_prompt(&mut self) {
        self.steam_prompt = None;
        self.steam_asked = false;
        self.pending_saved_login = None;
    }

    /// Starts a saved login whose Steam check just finished without a prompt.
    fn continue_saved_login(&mut self, account: String) {
        self.pending_saved_login = None;
        self.steam_prompt = None;
        self.begin_saved_login(account);
    }

    /// Steam was asked to close but is still running (a stuck process, or no
    /// permission). Keeps the prompt open with the reason so the user can
    /// cancel or log in anyway, instead of logging in as if it worked.
    pub fn report_steam_still_running(&mut self, error: String) {
        if let Some(prompt) = &mut self.steam_prompt {
            prompt.closing = false;
            prompt.error = error;
        } else {
            self.login_error = error;
        }
    }

    /// Legacy "Dont show this again": clears the `steamLogin` preference.
    pub fn forget_steam_prompt(&mut self) {
        if self.show_steam_login {
            self.show_steam_login = false;
            self.save_steam_login();
        }
    }

    /// Legacy `closeSteam`: asks the OS to terminate the Steam client.
    pub fn close_steam(&mut self) {
        let Some(prompt) = &mut self.steam_prompt else {
            return;
        };
        let Some(pid) = prompt.pid else {
            return;
        };
        prompt.closing = true;
        prompt.error.clear();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::SteamClosed(crate::steam::close(pid).await));
        });
    }

    /// Continues logging in without closing Steam.
    pub fn login_without_closing_steam(&mut self) {
        self.steam_prompt = None;
        self.submit_login();
    }

    /// Opens the container dialog for one row.
    pub fn begin_open_container(&mut self, item: &InventoryItem) {
        if self.session_busy || self.profile.is_none() {
            return;
        }
        // Legacy paired the container with the user's first key; preselect it
        // so "Open" works in one click.
        let key = self
            .inventory
            .as_ref()
            .and_then(|snapshot| {
                snapshot
                    .items
                    .iter()
                    .find(|row| crate::inventory::matches_property(row, "Key"))
            })
            .map(|row| row.id.clone())
            .unwrap_or_default();
        self.storage_notice.clear();
        self.open_container = Some(OpenContainerDialog {
            container_id: item.id.clone(),
            container_name: item.name.clone(),
            key_id: key,
            ..Default::default()
        });
    }

    pub fn submit_open_container(&mut self) {
        let Some(dialog) = &mut self.open_container else {
            return;
        };
        if dialog.busy || dialog.key_id.is_empty() {
            return;
        }
        dialog.busy = true;
        dialog.error.clear();
        let container = dialog.container_id.clone();
        let key = dialog.key_id.clone();
        let attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let result = core.open_container(container.clone(), key).await;
            reply.send(Message::ContainerOpened {
                attempt,
                container,
                result,
            });
        });
    }

    pub fn rename_storage(&mut self) {
        let Some(dialog) = &mut self.rename_dialog else {
            return;
        };
        if dialog.busy {
            return;
        }
        dialog.busy = true;
        dialog.error.clear();
        let casket_id = dialog.casket_id.clone();
        let name = dialog.name.trim().to_owned();
        let attempt = self.attempt;
        let session_attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let result = core.rename_storage(casket_id.clone(), name.clone()).await;
            reply.send(Message::StorageRename {
                attempt,
                session_attempt,
                casket: casket_id,
                name,
                result,
            });
        });
    }

    pub fn open_storage_contents(&mut self, casket_id: String, name: String) {
        if self.session_busy || self.profile.is_none() {
            return;
        }
        let is_inventory_storage = self.inventory.as_ref().is_some_and(|snapshot| {
            snapshot
                .items
                .iter()
                .any(|item| item.id == casket_id && item.is_storage_unit)
        });
        if !is_inventory_storage {
            self.inventory_error = "That storage unit is not in the loaded inventory.".into();
            return;
        }
        self.storage_selection = Some((casket_id.clone(), name));
        self.storage_contents = None;
        self.storage_error.clear();
        self.withdraw_error.clear();
        self.storage_notice.clear();
        self.storage_busy = true;
        self.storage_attempt += 1;
        let attempt = self.storage_attempt;
        let session_attempt = self.attempt;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            let result = core.storage_contents(casket_id.clone()).await;
            reply.send(Message::Storage {
                attempt,
                session_attempt,
                casket: casket_id,
                result,
            });
        });
    }

    pub fn close_storage_contents(&mut self) {
        self.storage_attempt += 1;
        self.storage_selection = None;
        self.storage_contents = None;
        self.storage_grouped.clear();
        self.storage_majors.clear();
        self.storage_busy = false;
        self.storage_error.clear();
        self.withdraw_busy = None;
        self.withdraw_error.clear();
        self.storage_notice.clear();
    }

    pub fn remove_account(&mut self, username: String) {
        self.removing = true;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::Deleted(core.store_delete_account(username).await));
        });
    }

    pub fn move_account(&mut self, username: String, index: usize) {
        if self.account_order_busy {
            return;
        }
        self.account_order_busy = true;
        let core = self.core.clone();
        let reply = self.reply.clone();
        self.rt.spawn(async move {
            reply.send(Message::Moved(
                core.store_set_account_position(username, index),
            ));
        });
    }
}

impl eframe::App for CasemoveApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(event) = self.events_rx.try_recv() {
            self.handle_event(event);
        }
        while let Ok(message) = self.rx.try_recv() {
            self.handle(ctx, message);
        }

        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(24.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, theme::SEPARATOR)),
            )
            .show(ctx, |ui| self.status_bar(ui));

        let sidebar = egui::SidePanel::left("sidebar")
            .resizable(true)
            .default_width(self.sidebar_width)
            .width_range(184.0..=320.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, theme::SEPARATOR))
                    .inner_margin(egui::Margin {
                        left: 14,
                        right: 14,
                        top: 18,
                        bottom: 14,
                    }),
            )
            .show(ctx, |ui| self.sidebar(ui));
        self.persist_sidebar_width(sidebar.response.rect.width(), ctx);

        // The saved scale is applied during construction above; Cmd+/-/0 are
        // handled by egui itself and land in the options at the end of the
        // pass that consumed them, so the write below lags one frame at most.
        let zoom = ctx.zoom_factor();
        if (zoom - self.saved_zoom).abs() > 0.001 {
            self.saved_zoom = zoom;
            self.save_zoom(zoom);
        }

        // The account list belongs to the home/login screen; a restored page
        // is only shown while signed in, so mirror the central panel.
        let on_page = self.profile.is_some()
            && (self.inventory_open
                || self.overview_open
                || self.storage_open
                || self.trade_up_open);
        if !on_page {
            egui::SidePanel::right("saved-accounts")
                .resizable(false)
                .exact_width(272.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::SURFACE)
                        .inner_margin(egui::Margin {
                            left: 12,
                            right: 12,
                            top: 20,
                            bottom: 16,
                        }),
                )
                .show(ctx, |ui| self.accounts_panel(ui));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::BG))
            .show(ctx, |ui| {
                if self.overview_open && self.profile.is_some() {
                    overview_view::show(ui, self);
                } else if self.inventory_open && self.profile.is_some() {
                    inventory_view::show(ui, self);
                } else if self.storage_open && self.profile.is_some() {
                    storage_view::show(ui, self);
                } else if self.trade_up_open && self.profile.is_some() {
                    tradeup_view::show(ui, self);
                } else if self.settings_open {
                    settings_view::show(ui, self);
                } else {
                    login::show(ui, self);
                }
            });

        // egui's `Modal::should_close` is true on Esc for whichever modal
        // draws first, and consumes the key while doing so. The decision is
        // taken once here, before any modal renders, and given to every open
        // dialog, so the dialog on top receives it.
        let escape = ctx.input(|input| input.key_pressed(egui::Key::Escape));
        self.open_container_dialog(ctx, escape);
        self.steam_prompt_dialog(ctx, escape);
        self.trade_up_review_dialog(ctx, escape);
        self.rename_storage_dialog(ctx, escape);
        self.move_to_storage_dialog(ctx, escape);
        self.removal_dialog(ctx, escape);
    }
}

// --- container opening -----------------------------------------------------

impl CasemoveApp {
    fn open_container_dialog(&mut self, ctx: &egui::Context, escape: bool) {
        let Some(dialog) = self.open_container.clone() else {
            return;
        };
        if !dialog.busy && escape {
            self.open_container = None;
            return;
        }
        let keys: Vec<(String, String)> = self
            .inventory
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .items
                    .iter()
                    .filter(|row| crate::inventory::matches_property(row, "Key"))
                    .map(|row| {
                        let label = row
                            .custom_name
                            .clone()
                            .filter(|name| !name.is_empty())
                            .unwrap_or_else(|| row.name.clone());
                        (row.id.clone(), label)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let selected = keys
            .iter()
            .find(|(id, _)| *id == dialog.key_id)
            .map(|(_, label)| label.clone())
            .unwrap_or_else(|| "Select a key".to_owned());
        let mut key_id = dialog.key_id.clone();
        let mut open = false;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("open_container")).show(ctx, |ui| {
            ui.set_min_width(400.0);
            ui.label(egui::RichText::new("Open container").size(19.0).strong());
            ui.label(
                egui::RichText::new(&dialog.container_name)
                    .size(12.0)
                    .color(theme::MUTED),
            );
            if !dialog.error.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&dialog.error).size(12.0).color(theme::ERROR));
            }
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Key").size(12.0));
            egui::ComboBox::from_id_salt("open-container-key")
                .selected_text(selected)
                .width(340.0)
                .show_ui(ui, |ui| {
                    for (id, label) in &keys {
                        ui.selectable_value(&mut key_id, id.clone(), label);
                    }
                });
            if keys.is_empty() {
                ui.label(
                    egui::RichText::new("No keys in this inventory.")
                        .size(11.0)
                        .color(theme::MUTED),
                );
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let button = egui::Button::new(
                    egui::RichText::new(if dialog.busy { "Opening..." } else { "Open" })
                        .color(theme::CARD),
                )
                .fill(theme::ACCENT);
                if ui
                    .add_enabled(!dialog.busy && !key_id.is_empty(), button)
                    .clicked()
                {
                    open = true;
                }
                if ui
                    .add_enabled(!dialog.busy, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if let Some(dialog) = &mut self.open_container {
            dialog.key_id = key_id;
        }
        if cancel {
            self.open_container = None;
        } else if open {
            self.submit_open_container();
        }
    }
}

// --- Steam-close prompt ----------------------------------------------------

impl CasemoveApp {
    fn steam_prompt_dialog(&mut self, ctx: &egui::Context, escape: bool) {
        let Some(prompt) = self.steam_prompt.clone() else {
            return;
        };
        if !prompt.closing && escape {
            self.dismiss_steam_prompt();
            return;
        }
        let mut close_and_login = false;
        let mut login_only = false;
        let mut cancel = false;
        let mut dont_show = prompt.dont_show;
        let can_close = prompt.pid.is_some() && !prompt.closing && !self.steam_checking;
        let modal = egui::Modal::new(egui::Id::new("steam_running")).show(ctx, |ui| {
            ui.set_min_width(440.0);
            ui.label(egui::RichText::new("Steam is currently running").size(19.0).strong());
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "You can continue to use the app, but it is recommended to close Steam as it \
                     can otherwise require a restart of your pc before Valve allows you to \
                     connect to VAC secured servers.",
                )
                .size(12.0)
                .color(theme::TEXT),
            );
            if !prompt.error.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&prompt.error).size(12.0).color(theme::ERROR));
            }
            ui.add_space(10.0);
            ui.checkbox(&mut dont_show, "Dont show this again");
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let close_button = egui::Button::new(
                    egui::RichText::new(if prompt.closing {
                        "Closing Steam..."
                    } else {
                        "Close and login"
                    })
                    .color(egui::Color32::WHITE),
                )
                .fill(egui::Color32::from_rgb(0x1f, 0x7f, 0x47));
                if ui
                    .add_enabled(can_close, close_button)
                    .on_disabled_hover_text("Steam's process could not be identified; log in without closing it")
                    .clicked()
                {
                    close_and_login = true;
                }
                if ui
                    .add_enabled(!prompt.closing, egui::Button::new("Login without closing"))
                    .clicked()
                {
                    login_only = true;
                }
                if ui
                    .add_enabled(!prompt.closing, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if let Some(prompt) = &mut self.steam_prompt {
            prompt.dont_show = dont_show;
        }
        if dont_show {
            self.forget_steam_prompt();
        }
        if cancel || modal.should_close() {
            self.dismiss_steam_prompt();
        } else if close_and_login {
            self.close_steam();
        } else if login_only {
            self.login_without_closing_steam();
        }
    }
}

// --- trade-up review -------------------------------------------------------

impl CasemoveApp {
    fn trade_up_review_dialog(&mut self, ctx: &egui::Context, escape: bool) {
        if !self.trade_up_review {
            return;
        }
        if !self.trade_up.busy && escape {
            self.trade_up_review = false;
            return;
        }
        let busy = self.trade_up.busy;
        let items: Vec<domain::CandidateItem> = self.trade_up.contract.items.clone();
        let total: f64 = items.iter().map(|item| item.price.unwrap_or(0.0)).sum();
        let expected: f64 = self
            .trade_up
            .outcomes
            .iter()
            .map(|outcome| {
                pricing::resolve(
                    &outcome.name,
                    Some(outcome.wear_name),
                    pricing::PriceSource::SteamListing,
                    None,
                )
                .unwrap_or(0.0)
                    * (outcome.percentage / 100.0)
            })
            .sum();
        let mut confirm = false;
        let mut cancel = false;
        let images = &mut self.images;
        let images = &mut *images;
        let average = self.trade_up.contract.average_wear();
        let currency = self.currency.clone();
        let money = |amount: f64| pricing::format_currency(&currency, amount, 2);
        let error = self.trade_up_dialog_error.clone().unwrap_or_default();
        egui::Modal::new(egui::Id::new("trade_up_review")).show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new("Review trade-up").size(19.0).strong());
            ui.label(
                egui::RichText::new(format!(
                    "{} items · average float {:.9}",
                    items.len(),
                    average
                ))
                .size(12.0)
                .color(theme::MUTED),
            );
            if !error.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&error).size(12.0).color(theme::ERROR));
            }
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                for item in &items {
                    ui.horizontal(|ui| {
                        images::media_box(
                            ui,
                            images,
                            item.image_url.as_deref(),
                            egui::vec2(34.0, 24.0),
                            egui::CornerRadius::same(3),
                            egui_phosphor::regular::CUBE,
                            12.0,
                            theme::ICON,
                        );
                        ui.label(egui::RichText::new(&item.name).size(theme::BODY_SIZE));
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(
                                        item.price.map(money).unwrap_or_default(),
                                    )
                                    .size(theme::MUTED_SIZE)
                                    .color(theme::MUTED),
                                );
                                ui.label(
                                    egui::RichText::new(
                                        item.wear.map(|wear| format!("{wear:.6}")).unwrap_or_default(),
                                    )
                                    .monospace()
                                    .size(10.0)
                                    .color(theme::MUTED),
                                );
                            },
                        );
                    });
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("Cost {}", money(total)))
                        .size(12.0)
                        .color(theme::ERROR_STRONG),
                );
                ui.label(
                    egui::RichText::new(format!("Expected value {}", money(expected)))
                        .size(12.0)
                        .color(theme::GREEN),
                );
            });
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let confirm_button = egui::Button::new(
                    egui::RichText::new(if busy { "Submitting..." } else { "Confirm trade-up" })
                        .color(theme::CARD),
                )
                .fill(theme::ACCENT);
                if ui
                    .add_enabled(!busy && items.len() == domain::Contract::CAPACITY, confirm_button)
                    .clicked()
                {
                    confirm = true;
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if cancel {
            self.trade_up_review = false;
        } else if confirm {
            self.submit_trade_up();
        }
    }
}

// --- sidebar ---------------------------------------------------------------

impl CasemoveApp {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(brand) = &self.brand {
                ui.add(
                    egui::Image::new((brand.id(), egui::vec2(28.0, 28.0)))
                        .fit_to_exact_size(egui::vec2(28.0, 28.0)),
                );
            }
            let label = ui.add(
                egui::Label::new(egui::RichText::new("casemove").size(20.0).strong())
                    .sense(egui::Sense::click()),
            );
            if label.clicked() {
                self.inventory_open = false;
                self.overview_open = false;
                self.storage_open = false;
                self.trade_up_open = false;
                self.settings_open = false;
            }
        });
        ui.add_space(16.0);

        ui.horizontal(|ui| {
            let avatar_url = self
                .profile
                .as_ref()
                .and_then(|profile| profile.avatar_url.clone());
            images::avatar(ui, &mut self.images, avatar_url.as_deref(), 32.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(profile_display_name(self.profile.clone())).size(13.0),
                );
                let (status, color) = match (&self.conn_state, self.profile.is_some()) {
                    (_, false) => ("Not connected", theme::ERROR_STRONG),
                    (app_core::ConnState::Connected, true) => ("Connected", theme::GREEN),
                    (app_core::ConnState::Retrying | app_core::ConnState::Lost, true) => {
                        ("Reconnecting...", egui::Color32::from_rgb(0xf5, 0xc2, 0x42))
                    }
                    (app_core::ConnState::RetryFailed(_), true) => {
                        ("Not connected", theme::ERROR_STRONG)
                    }
                };
                ui.label(egui::RichText::new(status).size(11.0).color(color));
                let balance = wallet_balance(self.wallet.clone());
                if !balance.is_empty() {
                    ui.label(egui::RichText::new(balance).size(11.0));
                }
            });
        });
        ui.add_space(16.0);

        let nav: [(&str, &str, &'static str); 5] = [
            ("Overview", egui_phosphor::regular::CHART_LINE, "overview"),
            ("Inventory", egui_phosphor::regular::ARCHIVE, "inventory"),
            ("Storage", egui_phosphor::regular::VAULT, "storage"),
            (
                "Trade-ups",
                egui_phosphor::regular::ARROWS_CLOCKWISE,
                "tradeup",
            ),
            ("Settings", egui_phosphor::regular::GEAR, "settings"),
        ];
        let width = ui.available_width();
        for (name, glyph, key) in nav {
            let current = match key {
                "overview" => self.overview_open,
                "inventory" => self.inventory_open,
                "storage" => self.storage_open,
                "tradeup" => self.trade_up_open,
                _ => self.settings_open,
            };
            // Settings only needs a local preference store, not a session.
            let enabled = key == "settings" || (self.profile.is_some() && !self.session_busy);
            let label = format!("{glyph}  {name}");
            let response = ui
                .add_enabled_ui(enabled, |ui| {
                    ui.add_sized(
                        egui::vec2(width, 30.0),
                        egui::Button::selectable(current, label),
                    )
                })
                .inner;
            if response.clicked() {
                self.overview_open = key == "overview";
                self.inventory_open = key == "inventory";
                self.storage_open = key == "storage";
                self.settings_open = key == "settings";
                self.trade_up_open = key == "tradeup";
            }
            if response.hovered() && !enabled {
                let tip = if self.profile.is_none() {
                    "Sign in to Steam first"
                } else {
                    "Waiting for the Steam session"
                };
                response.on_hover_text(tip);
            }
        }

        let disconnected = self.profile.is_some()
            && !matches!(self.conn_state, app_core::ConnState::Connected);
        if disconnected {
            ui.add_space(10.0);
            let (label, tip) = match &self.conn_state {
                app_core::ConnState::RetryFailed(error) => ("Retry connection", Some(error.clone())),
                _ => ("Retrying...", None),
            };
            let button = egui::Button::new(
                egui::RichText::new(format!(
                    "{}  {label}",
                    egui_phosphor::regular::ARROW_CLOCKWISE
                ))
                .color(theme::TEXT),
            )
            .fill(egui::Color32::from_rgb(0x2f, 0x3a, 0x2a))
            .min_size(egui::vec2(ui.available_width(), 30.0));
            let response = ui
                .add_enabled(!self.reconnecting, button)
                .on_hover_text(
                    tip.unwrap_or_else(|| "Retries the CS2 Game Coordinator connection".into()),
                );
            if response.clicked() {
                self.retry_connection();
            }
        }

        ui.add_space((ui.available_height() - 40.0).max(0.0));
        let mut sign_out = false;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::SHIELD_CHECK)
                    .size(18.0)
                    .color(theme::MUTED),
            );
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("Casemove")
                        .size(12.0)
                        .color(theme::MUTED),
                );
                ui.label(
                    egui::RichText::new(format!("v{APP_VERSION}"))
                        .size(10.0)
                        .color(theme::MUTED),
                );
            });
            if self.profile.is_some() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = egui::RichText::new(egui_phosphor::regular::SIGN_OUT)
                        .size(15.0)
                        .color(theme::MUTED);
                    if ui
                        .add_enabled(!self.session_busy, egui::Button::new(label).frame(false))
                        .on_hover_text("Sign out")
                        .clicked()
                    {
                        sign_out = true;
                    }
                });
            }
        });
        if sign_out {
            self.sign_out();
        }
    }

    // --- status strip -------------------------------------------------------

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let mut first = true;
        let mut segment = |ui: &mut egui::Ui, text: &str, color: egui::Color32| {
            if !first {
                ui.label(egui::RichText::new("·").size(11.0).color(theme::BORDER));
            }
            first = false;
            ui.label(egui::RichText::new(text).size(11.0).color(color));
        };

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            let (session, color) = if self.session_busy || self.loading {
                ("Connecting…", theme::STATUS_TEXT)
            } else if self.profile.is_none() {
                ("Steam not connected", theme::ERROR_STRONG)
            } else {
                match &self.conn_state {
                    app_core::ConnState::Connected => ("Steam connected", theme::GREEN),
                    app_core::ConnState::Lost | app_core::ConnState::Retrying => {
                        ("Reconnecting to CS2…", egui::Color32::from_rgb(0xf5, 0xc2, 0x42))
                    }
                    app_core::ConnState::RetryFailed(_) => {
                        ("CS2 disconnected", theme::ERROR_STRONG)
                    }
                }
            };
            segment(ui, session, color);

            let inventory = if self.inventory_busy {
                "Inventory loading…".to_owned()
            } else {
                self.inventory
                    .as_ref()
                    .map(|snapshot| format!("Inventory: {} items", snapshot.items.len()))
                    .unwrap_or_else(|| "Inventory: not loaded".to_owned())
            };
            segment(ui, &inventory, theme::STATUS_TEXT);

            if self.prices_busy {
                segment(ui, "Prices updating…", theme::STATUS_TEXT);
            }
            if let Some(items) = &self.storage_aggregate {
                segment(ui, &format!("Storage: {} items", items.len()), theme::STATUS_TEXT);
            }
            if let Some(selection) = &self.storage_selection {
                segment(ui, &format!("Unit: {}", selection.1), theme::STATUS_TEXT);
            }
            if !self.update_error.is_empty() {
                segment(ui, "Update check failed", theme::ERROR);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let check = egui::RichText::new("Check for updates")
                    .size(11.0)
                    .color(theme::MUTED);
                if ui
                    .add(egui::Button::new(check).frame(false))
                    .on_hover_text("Compares this build with the latest GitHub release")
                    .clicked()
                {
                    self.check_for_update();
                }
                let balance = wallet_balance(self.wallet.clone());
                if !balance.is_empty() {
                    ui.label(
                        egui::RichText::new(balance)
                            .size(11.0)
                            .color(theme::GREEN),
                    );
                }
            });
        });
        self.update_banner(ui);
    }

    /// "Update ready" strip, as legacy showed above the home content; the
    /// download opens the release page instead of restarting an Electron
    /// updater the native build does not have.
    fn update_banner(&mut self, ui: &mut egui::Ui) {
        if !self.update.available {
            return;
        }
        let muted = self.update.latest.clone().unwrap_or_default();
        egui::Frame::NONE
            .fill(theme::RAISED)
            .inner_margin(egui::Margin::symmetric(10, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(egui_phosphor::regular::ARROW_CIRCLE_UP)
                            .size(13.0)
                            .color(theme::GREEN),
                    );
                    ui.label(
                        egui::RichText::new(format!("Update {muted} is ready."))
                            .size(11.0)
                            .color(theme::TEXT),
                    );
                    if let Some(url) = self.update.download_url.clone()
                        && ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Download").size(11.0),
                                )
                                .frame(false),
                            )
                            .clicked()
                    {
                        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                    }
                });
            });
    }

    // --- saved accounts ---------------------------------------------------

    fn accounts_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("SAVED ACCOUNTS")
                    .size(11.0)
                    .strong()
                    .color(theme::MUTED),
            );
            if !self.accounts.is_empty() {
                ui.label(
                    egui::RichText::new(format!("· {}", self.accounts.len()))
                        .size(11.0)
                        .color(theme::MUTED),
                );
            }
        });
        ui.add_space(8.0);

        if self.loading {
            ui.label(
                egui::RichText::new("Loading accounts...")
                    .size(11.0)
                    .color(theme::MUTED),
            );
        }
        if !self.account_error.is_empty() {
            ui.label(
                egui::RichText::new(&self.account_error)
                    .size(11.0)
                    .color(theme::ERROR),
            );
        }
        if !self.loading && self.accounts.is_empty() {
            egui::Frame::NONE
                .fill(theme::RAISED)
                .corner_radius(6)
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(0x65, 0x65, 0x65),
                ))
                .inner_margin(egui::Margin::symmetric(12, 14))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(egui_phosphor::regular::USER)
                                .size(20.0)
                                .color(theme::ICON),
                        );
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new("No saved accounts")
                                .size(12.0)
                                .color(theme::MUTED),
                        );
                    });
                });
        }

        enum Action {
            Select(String),
            Login(String),
            Move(String, usize),
            Remove(String),
        }
        let mut action: Option<Action> = None;
        let accounts: Vec<(usize, SavedAccount)> = self
            .accounts
            .iter()
            .enumerate()
            .map(|(index, account)| (index, account.clone()))
            .collect();
        let pointer = ui.ctx().input(|input| input.pointer.latest_pos());
        let mut rects: Vec<(usize, egui::Rect)> = Vec::new();
        let mut dropped: Option<String> = None;

        for (index, account) in accounts {
            let enabled = !self.login_busy && !self.session_busy && self.profile.is_none();
            let (rect, response) = ui.allocate_exact_size(
                egui::vec2(ui.available_width(), 44.0),
                egui::Sense::click_and_drag(),
            );
            rects.push((index, rect));

            let hovered = pointer.is_some_and(|position| rect.contains(position));
            let drop_target = self.drag_account.is_some() && hovered;
            if drop_target {
                ui.painter().rect_filled(
                    rect,
                    6,
                    egui::Color32::from_rgba_unmultiplied(0x4f, 0x46, 0xe5, 70),
                );
            } else if hovered {
                ui.painter().rect_filled(rect, 6, theme::RAISED);
            } else if self.drag_account.as_ref() == Some(&account.username) {
                ui.painter().rect_filled(
                    rect,
                    6,
                    egui::Color32::from_rgba_unmultiplied(0x4f, 0x46, 0xe5, 40),
                );
            }

            if response.drag_started() {
                self.drag_account = Some(account.username.clone());
            }
            if response.drag_stopped() && self.drag_account.is_some() {
                dropped = Some(account.username.clone());
            }
            if enabled && response.clicked() {
                action = Some(Action::Select(account.username.clone()));
            }
            if response.hovered() && self.drag_account.is_none() {
                response
                    .ctx
                    .set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.horizontal_centered(|ui| {
                    images::avatar(ui, &mut self.images, account.image_url.as_deref(), 28.0);
                    ui.add_space(8.0);
                    // Reserve the action area up front so a long name cannot
                    // push the buttons out of the row.
                    let actions = if hovered { 62.0 } else { 0.0 };
                    let name_width = (rect.width() - 40.0 - actions).max(48.0);
                    ui.vertical(|ui| {
                        ui.add_sized(
                            egui::vec2(name_width, 16.0),
                            egui::Label::new(
                                egui::RichText::new(&account.display_name).strong().size(12.0),
                            )
                            .truncate(),
                        );
                        ui.add_sized(
                            egui::vec2(name_width, 14.0),
                            egui::Label::new(
                                egui::RichText::new(&account.username)
                                    .size(11.0)
                                    .color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    });
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 4.0;
                            if hovered {
                                if ui
                                    .add_enabled(
                                        !self.login_busy && !self.session_busy,
                                        egui::Button::new(egui::RichText::new(
                                            egui_phosphor::regular::TRASH,
                                        )),
                                    )
                                    .on_hover_text("Remove saved account")
                                    .clicked()
                                {
                                    action = Some(Action::Remove(account.username.clone()));
                                }
                                if account.has_saved_login
                                    && ui
                                        .add_enabled(
                                            enabled,
                                            egui::Button::new(egui::RichText::new(
                                                egui_phosphor::regular::SIGN_IN,
                                            )),
                                        )
                                        .on_hover_text(format!(
                                            "Sign in as {}",
                                            account.display_name
                                        ))
                                        .clicked()
                                {
                                    action = Some(Action::Login(account.username.clone()));
                                }
                            }
                        },
                    );
                });
            });
        }

        if let Some(username) = dropped {
            self.drag_account = None;
            let target = pointer
                .and_then(|position| {
                    rects
                        .iter()
                        .find(|(_, rect)| rect.contains(position))
                        .map(|(index, _)| *index)
                })
                .filter(|target| {
                    self.accounts
                        .iter()
                        .position(|account| account.username == username)
                        .is_some_and(|current| current != *target)
                });
            if let Some(target) = target {
                action = Some(Action::Move(username, target));
            }
        }

        match action {
            Some(Action::Select(username)) => self.select_account(username),
            Some(Action::Login(username)) => self.login_saved(username),
            Some(Action::Move(username, index)) => self.move_account(username, index),
            Some(Action::Remove(username)) => self.pending_removal = Some(username),
            None => {}
        }
    }

    // --- removal confirmation ---------------------------------------------

    fn move_to_storage_dialog(&mut self, ctx: &egui::Context, escape: bool) {
        let Some(dialog) = self.move_dialog.clone() else {
            return;
        };
        if !dialog.busy && escape {
            self.move_dialog = None;
            return;
        }
        let mut units: Vec<(String, String, usize)> = self
            .inventory
            .as_ref()
            .map(|snapshot| {
                snapshot
                    .items
                    .iter()
                    .filter(|item| item.is_storage_unit)
                    .map(|item| {
                        (
                            item.id.clone(),
                            item.custom_name
                                .clone()
                                .filter(|name| !name.is_empty())
                                .unwrap_or_else(|| item.name.clone()),
                            item.storage_item_count.unwrap_or(0) as usize,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        units.sort_by_key(|unit| unit.1.to_lowercase());

        let busy = dialog.busy;
        let mut lines = dialog.lines.clone();
        let mut query = dialog.query.clone();
        let mut target: Option<String> = None;
        let mut cancel = false;
        egui::Modal::new(egui::Id::new("move_to_storage")).show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.label(
                egui::RichText::new("Move to storage")
                    .size(19.0)
                    .strong(),
            );
            let total: usize = dialog.lines.iter().map(|line| line.quantity).sum();
            let available: usize = dialog.lines.iter().map(|line| line.ids.len()).sum();
            if dialog.lines.len() == 1 {
                let line = &dialog.lines[0];
                ui.label(
                    egui::RichText::new(&line.name)
                        .color(theme::MUTED),
                );
            } else {
                ui.label(
                    egui::RichText::new(format!(
                        "{} groups · {total} of {available} items",
                        dialog.lines.len()
                    ))
                    .color(theme::MUTED),
                );
            }
            if !dialog.error.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&dialog.error).size(12.0).color(theme::ERROR));
            }
            if busy && dialog.total > 0 {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "Moving {} of {}...",
                        dialog.done, dialog.total
                    ))
                    .size(12.0)
                    .color(theme::MUTED),
                );
            }
            if dialog.lines.len() > 1 {
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for line in &mut lines {
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&line.name)
                                    .size(12.0)
                                    .color(theme::MUTED),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let max = line.ids.len().max(1);
                                    ui.add(
                                        egui::DragValue::new(&mut line.quantity)
                                            .range(1..=max as u64),
                                    );
                                },
                            );
                        });
                    }
                });
            }
            ui.add_space(10.0);
            ui.add(
                egui::TextEdit::singleline(&mut query)
                    .hint_text("Search storage units")
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                let needle = query.to_lowercase();
                let visible: Vec<&(String, String, usize)> = units
                    .iter()
                    .filter(|(_, name, _)| needle.is_empty() || name.to_lowercase().contains(&needle))
                    .collect();
                if visible.is_empty() {
                    ui.label(
                        egui::RichText::new("No storage units")
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                }
                for (id, name, count) in visible {
                    let full = *count >= 1000 || *count + total > 1000;
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new(name).size(13.0));
                            ui.label(
                                egui::RichText::new(format!("{count} / 1000 items"))
                                    .size(10.0)
                                    .color(if full { theme::ERROR } else { theme::MUTED }),
                            );
                        });
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                let label = if busy {
                                    "Moving..."
                                } else if full {
                                    "Not enough space"
                                } else {
                                    "Move here"
                                };
                                if ui
                                    .add_enabled(
                                        !busy && !full,
                                        egui::Button::new(
                                            egui::RichText::new(label).color(if full {
                                                theme::MUTED
                                            } else {
                                                theme::CARD
                                            }),
                                        )
                                        .fill(if full {
                                            theme::RAISED
                                        } else {
                                            theme::ACCENT
                                        }),
                                    )
                                    .clicked()
                                {
                                    target = Some(id.clone());
                                }
                            },
                        );
                    });
                    ui.separator();
                }
            });
            ui.add_space(6.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!busy, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if let Some(dialog) = &mut self.move_dialog {
            dialog.query = query;
            dialog.lines = lines;
        }
        if cancel {
            self.move_dialog = None;
        } else if let Some(id) = target {
            self.move_item_to_storage(id);
        }
    }

    fn rename_storage_dialog(&mut self, ctx: &egui::Context, escape: bool) {
        let Some(dialog) = self.rename_dialog.clone() else {
            return;
        };
        if !dialog.busy && escape {
            self.rename_dialog = None;
            return;
        }
        let busy = dialog.busy;
        let mut name = dialog.name.clone();
        let mut cancel = false;
        let mut confirm = false;
        egui::Modal::new(egui::Id::new("rename_storage")).show(ctx, |ui| {
            ui.set_min_width(380.0);
            ui.label(
                egui::RichText::new("Rename storage unit")
                    .size(19.0)
                    .strong(),
            );
            ui.label(
                egui::RichText::new(&dialog.current_name)
                    .size(12.0)
                    .color(theme::MUTED),
            );
            if !dialog.error.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(&dialog.error).size(12.0).color(theme::ERROR));
            }
            ui.add_space(10.0);
            let field = ui.add(
                egui::TextEdit::singleline(&mut name)
                    .hint_text("New name")
                    .desired_width(f32::INFINITY),
            );
            if !busy {
                field.request_focus();
            }
            ui.add_space(12.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let confirm_button = egui::Button::new(
                    egui::RichText::new(if busy { "Renaming..." } else { "Rename" })
                        .color(theme::CARD),
                )
                .fill(theme::ACCENT);
                if ui
                    .add_enabled(
                        !busy && !name.trim().is_empty(),
                        confirm_button,
                    )
                    .clicked()
                {
                    confirm = true;
                }
                if ui
                    .add_enabled(!busy, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if let Some(dialog) = &mut self.rename_dialog {
            dialog.name = name;
        }
        if cancel {
            self.rename_dialog = None;
        } else if confirm {
            self.rename_storage();
        }
    }

    fn removal_dialog(&mut self, ctx: &egui::Context, escape: bool) {
        let Some(username) = self.pending_removal.clone() else {
            return;
        };
        if !self.removing && escape {
            self.pending_removal = None;
            return;
        }
        let removing = self.removing;
        let mut cancel = false;
        let mut confirm = false;
        egui::Modal::new(egui::Id::new("confirm_remove_account")).show(ctx, |ui| {
            ui.set_min_width(360.0);
            ui.label(
                egui::RichText::new("Remove saved account?")
                    .size(19.0)
                    .strong(),
            );
            ui.label(egui::RichText::new(&username).color(theme::MUTED));
            ui.add_space(16.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let remove = egui::Button::new(
                    egui::RichText::new(if removing {
                        "Removing..."
                    } else {
                        "Remove account"
                    })
                    .color(egui::Color32::WHITE),
                )
                .fill(theme::DANGER);
                if ui.add_enabled(!removing, remove).clicked() {
                    confirm = true;
                }
                if ui
                    .add_enabled(!removing, egui::Button::new("Cancel"))
                    .clicked()
                {
                    cancel = true;
                }
            });
        });
        if cancel {
            self.pending_removal = None;
        } else if confirm {
            self.remove_account(username);
        }
    }
}

// --- shared formatting ------------------------------------------------------

pub fn currency_symbol(currency: &str) -> &'static str {
    match currency {
        "USD" => "$",
        "EUR" => "€",
        "GBP" => "£",
        "RUB" => "₽",
        "JPY" | "CNY" => "¥",
        "KRW" => "₩",
        "BRL" => "R$",
        "TRY" => "₺",
        "PLN" => "zł",
        "UAH" => "₴",
        "INR" => "₹",
        "AUD" => "A$",
        "CAD" => "C$",
        "CHF" => "CHF ",
        "SEK" | "NOK" | "DKK" => "kr ",
        _ => "",
    }
}

pub fn format_balance(wallet: &WalletInfo) -> String {
    let symbol = currency_symbol(&wallet.currency);
    if symbol.is_empty() {
        format!("{} {:.2}", wallet.currency, wallet.balance)
    } else {
        format!("{symbol}{:.2}", wallet.balance)
    }
}

pub fn profile_display_name(profile: Option<SteamProfile>) -> String {
    profile
        .map(|value| {
            value
                .display_name
                .filter(|name| !name.is_empty())
                .unwrap_or(value.account)
        })
        .unwrap_or_else(|| "Steam account".into())
}

pub fn wallet_balance(wallet: Option<WalletInfo>) -> String {
    wallet
        .filter(|info| info.has_wallet && info.balance > 0.0)
        .map(|info| format_balance(&info))
        .unwrap_or_default()
}

fn prices_cache_path() -> std::path::PathBuf {
    app_core::config_dir().join("prices-live.json")
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// The user's Downloads folder, matching legacy `~/Downloads/casemove.csv`.
fn downloads_dir() -> std::path::PathBuf {
    #[cfg(windows)]
    let home = std::env::var_os("USERPROFILE").map(std::path::PathBuf::from);
    #[cfg(not(windows))]
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    home.unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("Downloads")
}

/// Writes the report, then reveals it where the platform supports that.
fn write_report(path: &std::path::Path, csv: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, csv)?;
    reveal(path);
    Ok(())
}

/// Opens the file manager at the report, best effort.
#[cfg(target_os = "windows")]
fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer")
        .arg(format!("/select,{}", path.display()))
        .spawn();
}

#[cfg(target_os = "macos")]
fn reveal(path: &std::path::Path) {
    let _ = std::process::Command::new("open").arg("-R").arg(path).spawn();
}

#[cfg(all(unix, not(target_os = "macos")))]
fn reveal(path: &std::path::Path) {
    if let Some(dir) = path.parent() {
        let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
    }
}

/// Re-resolves one row's price: the bundled snapshot overridden by any live
/// quote in the cache.
fn reprice(item: &mut InventoryItem, cache: &pricing::LiveCache) {
    if !item.moveable {
        return;
    }
    let key = pricing::live_key(&item.name, item.wear_name.as_deref());
    item.price = pricing::resolve(
        &item.name,
        item.wear_name.as_deref(),
        pricing::PriceSource::SteamListing,
        cache.price(&key),
    );
}
