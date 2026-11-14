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
    /// Persisted sidebar width and the last inventory page.
    pub sidebar_width: f32,
    pub saved_page: usize,
    /// Account row currently being dragged to reorder the saved list.
    pub drag_account: Option<String>,

    pub inventory: Option<InventorySnapshot>,
    pub grouped: Vec<crate::inventory::GroupedItem>,
    pub majors: Vec<String>,
    pub inventory_busy: bool,
    pub inventory_error: String,
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
        let (inventory_open, overview_open) =
            match core.store_get::<String>("ui.view").as_deref() {
                Some("inventory") => (true, false),
                Some("overview") => (false, true),
                _ => (false, false),
            };
        let saved_page = core.store_get::<usize>("ui.page").unwrap_or(0);
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
            overview_by,
            sidebar_width,
            saved_page,
            drag_account: None,
            inventory: None,
            grouped: Vec::new(),
            majors: Vec::new(),
            inventory_busy: false,
            inventory_error: String::new(),
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
            view: inventory_view::InventoryViewState {
                page: saved_page,
                ..Default::default()
            },
            storage_view: inventory_view::InventoryViewState::default(),
            storage_grouped: Vec::new(),
            storage_majors: Vec::new(),
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
        // Live schema and image catalog refresh in the background; the
        // bundled copies stay in place until each one succeeds.
        let schema_core = self.core.clone();
        self.rt.spawn(async move {
            schema_core.refresh_schema().await;
        });
        let catalog_reply = self.reply.clone();
        self.rt.spawn(async move {
            let catalog = app_core::fetch_image_catalog().await;
            catalog_reply.send(Message::ImageCatalog(catalog));
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
                            self.storage_contents = Some(items);
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
            Message::ImageCatalog(catalog) => self.images.set_catalog(catalog),
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
            reply.send(Message::LivePrices(
                app_core::fetch_live_prices(keys).await,
            ));
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

    /// Persists which page was last open (`overview` / `inventory`).
    pub fn save_view(&self, value: &'static str) {
        let core = self.core.clone();
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("ui.view", value);
        }));
    }

    /// Persists the inventory table's page.
    pub fn save_page(&self) {
        let core = self.core.clone();
        let page = self.view.page;
        drop(self.rt.spawn_blocking(move || {
            let _ = core.store_set("ui.page", page);
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
        self.set_inventory(None);
        self.inventory_open = false;
        self.overview_open = false;
        self.inventory_error.clear();
        self.inventory_busy = false;
        self.storage_selection = None;
        self.storage_contents = None;
        self.storage_grouped.clear();
        self.storage_majors.clear();
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
        self.storage_grouped.clear();
        self.storage_majors.clear();
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

        if !self.inventory_open && !self.overview_open {
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
                let (status, color) = if self.profile.is_some() {
                    ("Signed in to Steam", theme::GREEN)
                } else {
                    ("Not connected", theme::ERROR_STRONG)
                };
                ui.label(egui::RichText::new(status).size(11.0).color(color));
                let balance = wallet_balance(self.wallet.clone());
                if !balance.is_empty() {
                    ui.label(egui::RichText::new(balance).size(11.0));
                }
            });
        });
        ui.add_space(16.0);

        let nav: [(&str, &str); 2] = [
            ("Overview", egui_phosphor::regular::CHART_LINE),
            ("Inventory", egui_phosphor::regular::ARCHIVE),
        ];
        let width = ui.available_width();
        for (name, glyph) in nav {
            let is_overview = name == "Overview";
            let is_inventory = !is_overview;
            let enabled = self.profile.is_some() && !self.session_busy;
            let current = if is_overview {
                self.overview_open
            } else {
                self.inventory_open && !self.overview_open
            };
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
                self.overview_open = is_overview;
                self.inventory_open = is_inventory;
                self.save_view(if is_overview { "overview" } else { "inventory" });
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

        ui.add_space((ui.available_height() - 40.0).max(0.0));
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
                ui.label(egui::RichText::new("v0.1.0").size(10.0).color(theme::MUTED));
            });
        });
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

            let (session, color) = if self.session_busy {
                ("Connecting…", theme::STATUS_TEXT)
            } else if self.profile.is_some() {
                ("Steam connected", theme::GREEN)
            } else {
                ("Steam not connected", theme::ERROR_STRONG)
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

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
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
