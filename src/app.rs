use crate::images::{self, ImageCache};
use crate::messages::{Message, Reply};
use crate::{icons, inventory_view, login, overview_view, theme};
use app_commands::{
    BrowserTokenLoginParams, InventoryItem, InventorySnapshot, PasswordLoginParams, SavedAccount,
    SteamProfile, WalletInfo,
};
use app_core::{AppEvent, Core};
use eframe::egui;
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
    pub overview_by: overview_view::OverviewBy,

    pub inventory: Option<InventorySnapshot>,
    pending_inventory_events: Vec<AppEvent>,
    pub grouped: Vec<crate::inventory::GroupedItem>,
    pub majors: Vec<String>,
    pub inventory_busy: bool,
    pub inventory_error: String,
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
    pub price_cache: pricing::LiveCache,
    pub prices_busy: bool,
    pub view: inventory_view::InventoryViewState,
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
        let mut view = inventory_view::InventoryViewState::default();
        if let Some(columns) = core.store_get::<Vec<String>>("inventory.columns") {
            view.set_columns(columns);
        }
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
            inventory_open: false,
            overview_open: false,
            overview_by,
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
            price_cache: pricing::LiveCache::load(&prices_cache_path()),
            prices_busy: false,
            view,
        };
        app.initial_load();
        app
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
            }
            Message::SchemaRefresh(result) => match result {
                Ok(report) => {
                    self.schema_notice = if report.cache_used {
                        "Using recently refreshed cached item and sticker data.".into()
                    } else if report.warnings.is_empty() {
                        format!(
                            "Item data refreshed; {} image entries are available.",
                            report.image_entries
                        )
                    } else {
                        format!(
                            "Item data refreshed with {} image entries. {}",
                            report.image_entries,
                            report.warnings.join(" ")
                        )
                    };
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
                    self.schema_notice = format!(
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
                            self.storage_contents = Some(items);
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
            Message::StorageAll(attempt, result) => {
                self.storage_aggregate_busy = false;
                if attempt == self.attempt {
                    match result {
                        Ok(items) => {
                            self.storage_aggregate = Some(items);
                            self.storage_aggregate_error.clear();
                            self.apply_live_prices();
                            self.refresh_live_prices(false);
                        }
                        Err(error) => self.storage_aggregate_error = error,
                    }
                }
            }
            Message::Image { url, result } => self.images.insert(ctx, url, result),
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
        self.inventory = snapshot;
        // Contents may have changed with the new snapshot; reload on demand.
        self.storage_aggregate = None;
        self.storage_aggregate_error.clear();
        self.view.page = 0;
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

    pub fn submit_login(&mut self) {
        if self.loading || self.login_busy || self.session_busy || self.profile.is_some() {
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
        self.inventory_error.clear();
        self.inventory_busy = false;
        self.storage_selection = None;
        self.storage_contents = None;
        self.storage_busy = false;
        self.storage_error.clear();
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
        self.storage_busy = false;
        self.storage_error.clear();
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

        egui::SidePanel::left("sidebar")
            .resizable(false)
            .exact_width(256.0)
            .frame(
                egui::Frame::NONE
                    .fill(theme::SURFACE)
                    .stroke(egui::Stroke::new(1.0_f32, theme::SEPARATOR))
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 28,
                        bottom: 20,
                    }),
            )
            .show(ctx, |ui| self.sidebar(ui));

        if !self.inventory_open && !self.overview_open {
            egui::SidePanel::right("saved-accounts")
                .resizable(false)
                .exact_width(288.0)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::SURFACE)
                        .inner_margin(egui::Margin {
                            left: 16,
                            right: 16,
                            top: 40,
                            bottom: 40,
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
                } else {
                    login::show(ui, self);
                }
            });

        self.removal_dialog(ctx);
    }
}

// --- sidebar ---------------------------------------------------------------

impl CasemoveApp {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(brand) = &self.brand {
                ui.add(
                    egui::Image::new((brand.id(), egui::vec2(36.0, 36.0)))
                        .fit_to_exact_size(egui::vec2(36.0, 36.0)),
                );
            }
            let label = ui.add(
                egui::Label::new(egui::RichText::new("casemove").size(28.0).strong())
                    .sense(egui::Sense::click()),
            );
            if label.clicked() {
                self.inventory_open = false;
                self.overview_open = false;
            }
        });
        ui.add_space(30.0);

        ui.horizontal(|ui| {
            let avatar_url = self
                .profile
                .as_ref()
                .and_then(|profile| profile.avatar_url.clone());
            images::avatar(ui, &mut self.images, avatar_url.as_deref(), 40.0);
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(profile_display_name(self.profile.clone())).size(14.0),
                );
                let (status, color) = if self.profile.is_some() {
                    ("Signed in to Steam", theme::GREEN)
                } else {
                    ("Not connected", theme::ERROR_STRONG)
                };
                ui.label(egui::RichText::new(status).size(12.0).color(color));
                let balance = wallet_balance(self.wallet.clone());
                if !balance.is_empty() {
                    ui.label(egui::RichText::new(balance).size(12.0));
                }
            });
        });
        ui.add_space(26.0);

        let nav: [(&str, &str); 4] = [
            ("Overview", egui_phosphor::regular::CHART_LINE),
            ("Transfer | From", egui_phosphor::regular::DOWNLOAD),
            ("Transfer | To", egui_phosphor::regular::UPLOAD),
            ("Inventory", egui_phosphor::regular::ARCHIVE),
        ];
        for (name, glyph) in nav {
            let is_inventory = name == "Inventory";
            let is_overview = name == "Overview";
            let is_page = is_inventory || is_overview;
            let enabled = is_page && self.profile.is_some() && !self.session_busy;
            let current = if is_overview {
                self.overview_open
            } else if is_inventory {
                self.inventory_open && !self.overview_open
            } else {
                false
            };
            let label = format!("{glyph}  {name}");
            let response = ui
                .add_enabled_ui(enabled, |ui| ui.selectable_label(current, label))
                .inner;
            if response.clicked() {
                self.overview_open = is_overview;
                self.inventory_open = is_inventory;
            }
            if response.hovered() && !enabled {
                let tip = if self.profile.is_none() {
                    "Sign in to Steam first"
                } else if is_page {
                    "Waiting for the Steam session"
                } else {
                    "This page has not been ported yet"
                };
                response.on_hover_text(tip);
            }
        }

        ui.add_space((ui.available_height() - 52.0).max(0.0));
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::SHIELD_CHECK)
                    .size(22.0)
                    .color(theme::MUTED),
            );
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("Casemove")
                        .size(13.0)
                        .color(theme::MUTED),
                );
                ui.label(egui::RichText::new("v0.1.0").size(11.0).color(theme::MUTED));
            });
        });
    }

    // --- saved accounts ---------------------------------------------------

    fn accounts_panel(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("SAVED ACCOUNTS")
                .size(12.0)
                .strong()
                .color(theme::MUTED),
        );
        ui.add_space(18.0);

        if self.loading {
            ui.label(egui::RichText::new("Loading accounts...").color(theme::MUTED));
        }
        if !self.account_error.is_empty() {
            ui.label(
                egui::RichText::new(&self.account_error)
                    .size(12.0)
                    .color(theme::ERROR),
            );
        }
        if !self.loading && self.accounts.is_empty() {
            egui::Frame::NONE
                .fill(theme::RAISED)
                .corner_radius(8)
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(0x65, 0x65, 0x65),
                ))
                .inner_margin(egui::Margin::symmetric(14, 18))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        images::media_box(
                            ui,
                            &mut self.images,
                            None,
                            egui::vec2(40.0, 40.0),
                            egui::CornerRadius::same(20),
                            egui_phosphor::regular::USER,
                            22.0,
                            theme::ICON,
                        );
                        ui.vertical(|ui| {
                            ui.label(egui::RichText::new("Nothing here").strong().size(13.0));
                            ui.label(
                                egui::RichText::new("No saved accounts")
                                    .size(12.0)
                                    .color(theme::MUTED),
                            );
                        });
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
        for (index, account) in accounts {
            let enabled = !self.login_busy && !self.session_busy && self.profile.is_none();
            let frame = egui::Frame::NONE
                .fill(theme::RAISED)
                .corner_radius(8)
                .stroke(egui::Stroke::new(
                    1.0_f32,
                    egui::Color32::from_rgb(0x65, 0x65, 0x65),
                ))
                .inner_margin(egui::Margin::symmetric(14, 18));
            frame.show(ui, |ui| {
                ui.horizontal(|ui| {
                    images::avatar(ui, &mut self.images, account.image_url.as_deref(), 32.0);
                    ui.vertical(|ui| {
                        let name = ui.add(
                            egui::Label::new(
                                egui::RichText::new(&account.display_name)
                                    .strong()
                                    .size(13.0),
                            )
                            .sense(egui::Sense::click()),
                        );
                        let user = ui.add(
                            egui::Label::new(
                                egui::RichText::new(&account.username)
                                    .size(12.0)
                                    .color(theme::MUTED),
                            )
                            .sense(egui::Sense::click()),
                        );
                        if enabled && (name.clicked() || user.clicked()) {
                            action = Some(Action::Select(account.username.clone()));
                        }
                        if name.hovered() || user.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
                                .on_hover_text(format!("Sign in as {}", account.display_name))
                                .clicked()
                        {
                            action = Some(Action::Login(account.username.clone()));
                        }
                        ui.vertical(|ui| {
                            let up = ui
                                .add_enabled(
                                    !self.account_order_busy && index > 0,
                                    egui::Button::new(egui::RichText::new(
                                        egui_phosphor::regular::ARROW_UP,
                                    )),
                                )
                                .on_hover_text("Move account up");
                            let down = ui
                                .add_enabled(
                                    !self.account_order_busy && index + 1 < self.accounts.len(),
                                    egui::Button::new(egui::RichText::new(
                                        egui_phosphor::regular::ARROW_DOWN,
                                    )),
                                )
                                .on_hover_text("Move account down");
                            if up.clicked() {
                                action = Some(Action::Move(
                                    account.username.clone(),
                                    index.saturating_sub(1),
                                ));
                            }
                            if down.clicked() {
                                action = Some(Action::Move(account.username.clone(), index + 1));
                            }
                        });
                    });
                });
            });
            ui.add_space(10.0);
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

    fn removal_dialog(&mut self, ctx: &egui::Context) {
        let Some(username) = self.pending_removal.clone() else {
            return;
        };
        if !self.removing && ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
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
