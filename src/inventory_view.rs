//! Inventory and storage-unit contents: one command bar, a filter popover,
//! a sortable table with a pinned header and footer, and a pager. Both
//! datasets share the same chrome so the storage view is just the inventory
//! view over a different list.

use crate::app::CasemoveApp;
use crate::images;
use crate::inventory::{self, GroupedItem};
use crate::theme;
use app_commands::InventoryItem;
use eframe::egui;
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// Rows per page; the pager steps through these.
pub const PAGE_SIZE: usize = 50;

/// Which dataset a section renders: the loaded inventory or the open storage
/// unit's contents.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Inventory,
    Storage,
}

/// Deferred actions collected while rendering, applied after the
/// `&mut CasemoveApp` borrow ends.
#[derive(Default)]
struct SectionOut {
    open_storage: Option<(String, String)>,
    move_to: Option<(String, String)>,
    open_container: Option<InventoryItem>,
    withdraw: Option<String>,
    rename: Option<(String, String)>,
    clipboard: Option<String>,
    move_selected: bool,
}

/// Table sort keys. `Category` and `Tradehold` have no column of their own
/// but stay reachable from the toolbar sort menu.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    #[default]
    Position,
    Name,
    Rarity,
    Float,
    Quantity,
    Price,
    Category,
    Tradehold,
}

impl SortKey {
    const ALL: [Self; 8] = [
        Self::Position,
        Self::Name,
        Self::Rarity,
        Self::Float,
        Self::Quantity,
        Self::Price,
        Self::Category,
        Self::Tradehold,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Position => "Position",
            Self::Name => "Name",
            Self::Rarity => "Rarity",
            Self::Float => "Float",
            Self::Quantity => "Quantity",
            Self::Price => "Price",
            Self::Category => "Category",
            Self::Tradehold => "Tradehold",
        }
    }

    /// The key `inventory::sort_rows` understands.
    fn mode(self) -> &'static str {
        match self {
            Self::Position => "position",
            Self::Name => "name",
            Self::Rarity => "rarity",
            Self::Float => "wear",
            Self::Quantity => "qty",
            Self::Price => "price",
            Self::Category => "category",
            Self::Tradehold => "tradehold",
        }
    }
}

/// Which table is on screen; columns are remembered per table.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TableId {
    Inventory,
    Storage,
}

impl TableId {
    pub fn store_key(self) -> &'static str {
        match self {
            Self::Inventory => "inventory",
            Self::Storage => "storage",
        }
    }
}

/// A column the user can hide.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColumnId {
    Rarity,
    Wear,
    Qty,
    Price,
    Stickers,
    Seed,
    Tradehold,
    Category,
    Id,
}

impl ColumnId {
    const ALL: [Self; 9] = [
        Self::Rarity,
        Self::Wear,
        Self::Qty,
        Self::Price,
        Self::Stickers,
        Self::Seed,
        Self::Tradehold,
        Self::Category,
        Self::Id,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Rarity => "Rarity",
            Self::Wear => "Wear / float",
            Self::Qty => "Quantity",
            Self::Price => "Price",
            Self::Stickers => "Stickers",
            Self::Seed => "Paint seed",
            Self::Tradehold => "Tradehold",
            Self::Category => "Category",
            Self::Id => "Item ID",
        }
    }
}

/// Column visibility, persisted under `ui.columns.<table>`.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ColumnSet {
    pub visible: BTreeSet<ColumnId>,
}

impl Default for ColumnSet {
    fn default() -> Self {
        Self {
            visible: [
                ColumnId::Wear,
                ColumnId::Qty,
                ColumnId::Price,
                ColumnId::Stickers,
                ColumnId::Tradehold,
            ]
            .into_iter()
            .collect(),
        }
    }
}

impl ColumnSet {
    fn is_visible(&self, column: ColumnId) -> bool {
        self.visible.contains(&column)
    }
}

pub struct InventoryViewState {
    pub query: String,
    pub sort: SortKey,
    pub sort_desc: bool,
    pub units_only: bool,
    /// Zero-based page shown in the pager.
    pub page: usize,
    pub selected_categories: HashSet<String>,
    pub selected_rarities: HashSet<String>,
    pub selected_majors: HashSet<String>,
    pub selected_properties: HashSet<String>,
    pub columns: ColumnSet,
    /// Selected grouped rows: representative id -> selected quantity.
    pub selected: BTreeMap<String, usize>,
    /// First row of the last selection, for shift-click ranges.
    pub anchor: Option<String>,
}

impl InventoryViewState {
    pub fn new(columns: ColumnSet, sort: SortKey, sort_desc: bool) -> Self {
        Self {
            query: String::new(),
            sort,
            sort_desc,
            units_only: false,
            page: 0,
            selected_categories: HashSet::new(),
            selected_rarities: HashSet::new(),
            selected_majors: HashSet::new(),
            selected_properties: HashSet::new(),
            columns,
            selected: BTreeMap::new(),
            anchor: None,
        }
    }

    /// Back to the first page whenever the result set changes.
    fn reset_page(&mut self) {
        self.page = 0;
    }
}

impl Default for InventoryViewState {
    fn default() -> Self {
        Self::new(ColumnSet::default(), SortKey::default(), false)
    }
}

/// Key handling inside a section.
struct Shortcuts {
    focus_search: bool,
    escape: bool,
}

/// Which tables had their column set edited; persisted after the frame.
#[derive(Default)]
struct ColumnEdits {
    inventory: bool,
    storage: bool,
}

impl ColumnEdits {
    fn mark(&mut self, table: TableId) {
        match table {
            TableId::Inventory => self.inventory = true,
            TableId::Storage => self.storage = true,
        }
    }
}

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut back = false;
    let mut refresh = false;
    let mut sign_out = false;
    let mut export = false;
    let mut out = SectionOut::default();
    let mut column_edits = ColumnEdits::default();
    let mut footer: Option<Footer> = None;
    // The pager's buttons grow with the UI zoom, so the strip is measured
    // from the pager's own size instead of a fixed height.
    let footer_height = FOOTER_HEIGHT.max(PAGER_BUTTON * ui.ctx().zoom_factor() + 16.0);
    let available = ui.available_rect_before_wrap();

    let shortcuts = Shortcuts {
        focus_search: ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)),
        escape: ui.input(|i| i.key_pressed(egui::Key::Escape))
            && !egui::Popup::is_any_open(ui.ctx()),
    };

    // The page stops above the band while the table is shown, so the card and
    // its shadow never reach the bottom edge.
    let page_rect = egui::Rect::from_min_max(
        available.min,
        egui::pos2(
            available.right(),
            (available.bottom() - PAGE_BAND_GAP - footer_height).max(available.top()),
        ),
    );
    let filled = ui
        .scope_builder(egui::UiBuilder::new().max_rect(page_rect), |ui| {
            theme::page().show(ui, |ui| {
                let page = Page {
                    shortcuts: &shortcuts,
                    out: &mut out,
                    footer: &mut footer,
                    refresh: &mut refresh,
                    sign_out: &mut sign_out,
                    export: &mut export,
                    column_edits: &mut column_edits,
                };
                if app.storage_selection.is_some() {
                    storage_section(ui, app, page, &mut back);
                } else {
                    main_section(ui, app, page);
                }
            })
        })
        .inner;

    // Without a pager, the page is a single block filling the view down to
    // the band; then it owns the band's space too.
    let page_rect = if footer.is_none() {
        egui::Rect::from_min_max(
            available.min,
            egui::pos2(available.right(), filled.response.rect.bottom()),
        )
    } else {
        page_rect
    };

    // The pager band anchors to the bottom edge of the view: opaque, on its
    // own layer so nothing paints over it, and running the view's full width
    // even though the pager's content stays aligned with the page's margins.
    let band = egui::Rect::from_min_max(
        egui::pos2(available.left(), page_rect.bottom() + PAGE_BAND_GAP),
        available.max,
    );
    let mut footer_out = SectionOut::default();
    if let Some(mut footer) = footer {
        let layer = egui::LayerId::new(egui::Order::Middle, egui::Id::new("inventory-footer"));
        ui.scope_builder(
            egui::UiBuilder::new().layer_id(layer).max_rect(band),
            |ui| {
                ui.set_clip_rect(band);
                footer_bar(
                    ui,
                    &mut app.view,
                    &mut footer_out,
                    Footer {
                        labels: std::mem::take(&mut footer.labels),
                        ..footer
                    },
                );
            },
        );
        out.move_selected |= footer_out.move_selected;
    }

    if back {
        app.close_storage_contents();
    }
    if refresh {
        app.refresh_inventory();
    }
    if sign_out {
        app.sign_out();
    }
    if let Some((casket_id, name)) = out.open_storage {
        app.open_storage_contents(casket_id, name);
    }
    if let Some((item_id, item_name)) = out.move_to {
        app.begin_move_to_storage(item_id, item_name);
    }
    if let Some(item_id) = out.withdraw {
        app.withdraw_item(item_id);
    }
    if let Some((casket_id, name)) = out.rename {
        app.begin_rename_storage(casket_id, name);
    }
    if out.move_selected {
        let lines = app
            .grouped
            .iter()
            .filter_map(|row| {
                let quantity = *app.view.selected.get(&row.representative.id)?;
                Some(crate::app::MoveLine {
                    name: row.representative.name.clone(),
                    ids: row.combined_ids.clone(),
                    quantity,
                })
            })
            .collect();
        app.begin_bulk_move(lines);
    }
    if let Some(text) = out.clipboard {
        ui.ctx().copy_text(text);
    }
    if export {
        app.export_csv();
    }
    if let Some(row) = out.open_container {
        app.begin_open_container(&row);
    }
    if column_edits.inventory {
        let columns = app.view.columns.clone();
        app.save_columns(TableId::Inventory, &columns);
    }
    if column_edits.storage {
        let columns = app.storage_view.columns.clone();
        app.save_columns(TableId::Storage, &columns);
    }
}

/// The deferred sinks every section writes into; bundled so the heading
/// helpers do not need a dozen parameters.
struct Page<'a> {
    shortcuts: &'a Shortcuts,
    out: &'a mut SectionOut,
    /// Where the section leaves its pager's contents for the bottom band.
    footer: &'a mut Option<Footer>,
    refresh: &'a mut bool,
    sign_out: &'a mut bool,
    export: &'a mut bool,
    column_edits: &'a mut ColumnEdits,
}

fn main_section(ui: &mut egui::Ui, app: &mut CasemoveApp, page: Page<'_>) {
    let Page {
        shortcuts,
        out,
        footer,
        refresh,
        sign_out,
        export,
        column_edits,
    } = page;

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            let loaded = app.inventory.as_ref().map(|snapshot| snapshot.items.len());
            let value = app
                .inventory
                .is_some()
                .then(|| filtered_value(app, &app.grouped, &app.view));
            title_row(ui, "Inventory", summary(loaded, value.as_deref()));
            if app.inventory_busy {
                subtle(ui, "Waiting for the CS2 Game Coordinator…");
            } else if app.inventory.is_none() {
                subtle(
                    ui,
                    "Close the game first: connecting marks this account as playing CS2.",
                );
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if icon_button(ui, egui_phosphor::regular::SIGN_OUT, false, "Sign out").clicked() {
                *sign_out = true;
            }
            let (text, enabled) = if app.inventory_busy {
                ("Loading…".to_owned(), false)
            } else if app.inventory.is_some() {
                ("Refresh".to_owned(), true)
            } else {
                ("Connect to CS2".to_owned(), true)
            };
            let button = primary_button(format!(
                "{}  {text}",
                egui_phosphor::regular::ARROW_CLOCKWISE
            ));
            if ui
                .add_enabled(enabled, button)
                .on_hover_text("Re-read the inventory from the Game Coordinator")
                .clicked()
            {
                *refresh = true;
            }
        });
    });

    if !app.inventory_error.is_empty() {
        error_box(
            ui,
            &app.inventory_error,
            app.inventory
                .is_some()
                .then_some("Showing the last loaded snapshot."),
        );
    }
    if !app.storage_notice.is_empty() {
        notice(ui, &app.storage_notice);
    }
    ui.add_space(8.0);

    if app.inventory.is_none() {
        if !app.inventory_busy && app.inventory_error.is_empty() {
            let connect = empty_state(
                ui,
                "Inventory not loaded",
                Some(("Connect to CS2", egui_phosphor::regular::ARROW_CLOCKWISE)),
            );
            if connect {
                *refresh = true;
            }
        }
        return;
    }

    let uses = SectionUses {
        shortcuts,
        out,
        footer,
        export,
        column_edits,
    };
    items_section(ui, app, Section::Inventory, uses);
}

fn storage_section(ui: &mut egui::Ui, app: &mut CasemoveApp, page: Page<'_>, back: &mut bool) {
    let name = app
        .storage_selection
        .as_ref()
        .map(|(_, name)| name.clone())
        .unwrap_or_default();
    let count = app.storage_contents.as_ref().map(|items| items.len());
    let value = app
        .storage_contents
        .is_some()
        .then(|| filtered_value(app, &app.storage_grouped, &app.storage_view));

    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            title_row(ui, &name, summary(count, value.as_deref()));
            subtle(ui, "Storage contents");
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if icon_button(ui, egui_phosphor::regular::SIGN_OUT, false, "Sign out").clicked() {
                app.sign_out();
            }
            let button = egui::Button::new(format!(
                "{}  Inventory",
                egui_phosphor::regular::ARROW_LEFT
            ));
            if ui.add(button).clicked() {
                *back = true;
            }
        });
    });

    if !app.storage_error.is_empty() {
        error_box(ui, &app.storage_error, None);
    }
    if !app.withdraw_error.is_empty() {
        error_box(ui, &app.withdraw_error.clone(), None);
    }
    if !app.storage_notice.is_empty() {
        notice(ui, &app.storage_notice);
    }
    ui.add_space(8.0);

    if app.storage_busy {
        loading(ui, "Loading storage contents…");
        return;
    }
    if app.storage_contents.is_none() {
        return;
    }
    if app.storage_grouped.is_empty() {
        empty_state(ui, "Storage unit is empty", None);
        return;
    }

    let uses = SectionUses {
        shortcuts: page.shortcuts,
        out: page.out,
        footer: page.footer,
        export: page.export,
        column_edits: page.column_edits,
    };
    let page_before = app.storage_view.page;
    items_section(ui, app, Section::Storage, uses);
    if app.storage_view.page != page_before {
        app.save_storage_page();
    }
}

/// `Title` followed by a muted summary on the same baseline.
fn title_row(ui: &mut egui::Ui, title: &str, summary: Option<String>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(
            egui::RichText::new(title)
                .size(theme::TITLE_SIZE)
                .strong()
                .color(theme::TEXT),
        );
        if let Some(summary) = summary {
            ui.label(
                egui::RichText::new(summary)
                    .size(theme::MUTED_SIZE)
                    .color(theme::MUTED),
            );
        }
    });
}

/// `1,204 items · $12,340.18`, or `Not loaded` when there is no list yet.
fn summary(loaded: Option<usize>, value: Option<&str>) -> Option<String> {
    let count = loaded?;
    let mut text = format!(
        "{count} {}",
        if count == 1 { "item" } else { "items" }
    );
    if let Some(value) = value {
        text.push_str(" · ");
        text.push_str(value);
    }
    Some(text)
}

fn subtle(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::MUTED_SIZE)
            .color(theme::MUTED),
    );
}

fn filtered_value(app: &CasemoveApp, grouped: &[GroupedItem], view: &InventoryViewState) -> String {
    let value: f64 = filtered_rows(grouped, view)
        .iter()
        .map(|row| row.representative.price.unwrap_or(0.0) * row.combined_qty as f64)
        .sum();
    app.money(value)
}

/// Command bar, active-filter chips, table, and footer for one dataset.
fn items_section(ui: &mut egui::Ui, app: &mut CasemoveApp, section: Section, uses: SectionUses<'_>) {
    let SectionUses {
        shortcuts,
        out,
        footer: section_footer,
        export,
        column_edits,
    } = uses;
    let table_id = match section {
        Section::Inventory => TableId::Inventory,
        Section::Storage => TableId::Storage,
    };

    let mut refresh_prices = false;
    {
        let view = match section {
            Section::Inventory => &mut app.view,
            Section::Storage => &mut app.storage_view,
        };
        let majors: &[String] = match section {
            Section::Inventory => &app.majors,
            Section::Storage => &app.storage_majors,
        };
        let bar = CommandBar {
            hint: match section {
                Section::Inventory => "Search items, IDs, stickers…",
                Section::Storage => "Search contents",
            },
            shortcuts,
            can_export: app.inventory.is_some() || app.storage_aggregate.is_some(),
            prices_busy: app.prices_busy,
            export,
            refresh_prices: &mut refresh_prices,
        };
        command_bar(ui, view, table_id, majors, bar, column_edits);
        active_chips(ui, view);
    }
    if refresh_prices {
        app.refresh_live_prices(true);
    }

    let (grouped, view) = match section {
        Section::Inventory => (&app.grouped, &mut app.view),
        Section::Storage => (&app.storage_grouped, &mut app.storage_view),
    };
    let mut rows = filtered_rows(grouped, view);
    sort_rows(&mut rows, view);

    let total = rows.len();
    if total == 0 {
        let filtered = has_active_filters(view);
        let clear = empty_state(
            ui,
            if filtered {
                "No matching items"
            } else {
                "Nothing here"
            },
            filtered.then_some(("Clear filters", egui_phosphor::regular::FUNNEL_SIMPLE_X)),
        );
        if clear {
            clear_filters(view);
        }
        return;
    }

    let pages = total.div_ceil(PAGE_SIZE).max(1);
    view.page = view.page.min(pages - 1);
    let start = view.page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);
    let selectable = section == Section::Inventory;

    let withdraw_busy = app.withdraw_busy.clone();
    let rt = app.rt.handle().clone();
    let mut footer = FooterState::default();
    // The pager lives in the page's bottom band, outside this card; the card
    // just leaves room for it and hands its contents over.
    *section_footer = Some(Footer {
        total,
        page: view.page,
        pages,
        all_rows: rows
            .iter()
            .filter(|row| selectable_row(&row.representative))
            .map(|row| (row.representative.id.clone(), row.combined_qty))
            .collect(),
        labels: Vec::new(),
    });
    theme::card().show(ui, |ui| {
        card_header(
            ui,
            CardHeader { total },
        );

        let available = ui.available_rect_before_wrap();
        let band_height = (FOOTER_HEIGHT.max(PAGER_BUTTON * ui.ctx().zoom_factor() + 16.0))
            .min(available.height());
        let table_rect = egui::Rect::from_min_max(
            available.min,
            egui::pos2(
                available.right(),
                (available.bottom() - band_height - TABLE_GAP).max(available.top()),
            ),
        );

        let context = TableContext {
            out,
            currency: &app.currency,
            withdraw_busy: &withdraw_busy,
            section,
            selectable,
        };
        let page_rows = &rows[start..end];
        ui.scope_builder(egui::UiBuilder::new().max_rect(table_rect), |ui| {
            for row in page_rows {
                prefetch_thumbnail(ui, &rt, &row.representative);
            }
            let interactive =
                table(ui, view, page_rows, context, &mut footer, table_rect.height());
            if let Some(index) = footer.clicked_row {
                apply_row_click(view, &interactive, index);
            }
        });
    });
}

/// The card's title row: the result count, like "21472 orders".
#[derive(Clone, Copy)]
struct CardHeader {
    total: usize,
}

fn card_header(ui: &mut egui::Ui, header: CardHeader) {
    let CardHeader { total } = header;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let count = format!(
            "{total} {}",
            if total == 1 { "item" } else { "items" }
        );
        ui.label(
            egui::RichText::new(count)
                .size(theme::HEADING_SIZE)
                .color(theme::TEXT_STRONG),
        );
    });
    ui.add_space(14.0);
}

/// Rows paint whatever thumbnails are already cached; fetching happens once
/// per URL through the app's worker thread.
fn prefetch_thumbnail(ui: &egui::Ui, rt: &tokio::runtime::Handle, item: &InventoryItem) {
    let urls = std::iter::once(item.image_url.as_deref().filter(|url| !url.is_empty())).chain(
        item.stickers
            .iter()
            .take(4)
            .map(|sticker| sticker.image_url.as_deref()),
    );
    for url in urls.flatten() {
        images::prefetch(rt, ui.ctx(), url);
    }
}

fn apply_row_click(view: &mut InventoryViewState, rows: &[InteractiveRow], index: usize) {
    let Some(row) = rows.get(index) else {
        return;
    };
    if !row.selectable {
        return;
    }
    let shift = row.shift;
    if shift
        && let Some(anchor) = view.anchor.as_deref()
        && let Some(from) = rows.iter().position(|row| row.id == anchor)
    {
        let range = from.min(index)..=from.max(index);
        let wanted = !view.selected.contains_key(&rows[index].id);
        for row in rows.get(range).unwrap_or_default() {
            if !row.selectable {
                continue;
            }
            if wanted {
                view.selected.insert(row.id.clone(), row.quantity);
            } else {
                view.selected.remove(&row.id);
            }
        }
        return;
    }
    if view.selected.remove(&row.id).is_none() {
        view.selected.insert(row.id.clone(), row.quantity);
    }
    view.anchor = Some(row.id.clone());
}

/// What `items_section` hands to its children.
struct SectionUses<'a> {
    shortcuts: &'a Shortcuts,
    out: &'a mut SectionOut,
    footer: &'a mut Option<Footer>,
    export: &'a mut bool,
    column_edits: &'a mut ColumnEdits,
}

struct CommandBar<'a> {
    hint: &'a str,
    shortcuts: &'a Shortcuts,
    can_export: bool,
    prices_busy: bool,
    export: &'a mut bool,
    refresh_prices: &'a mut bool,
}

fn command_bar(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    table_id: TableId,
    majors: &[String],
    bar: CommandBar<'_>,
    column_edits: &mut ColumnEdits,
) {
    let CommandBar {
        hint,
        shortcuts,
        can_export,
        prices_busy,
        export,
        refresh_prices,
    } = bar;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;

        let search = ui.add(
            egui::TextEdit::singleline(&mut view.query)
                .hint_text(hint)
                .desired_width(340.0)
                .margin(egui::vec2(10.0, 6.0))
                .background_color(theme::CARD),
        );
        if shortcuts.focus_search {
            search.request_focus();
        }
        if search.changed() {
            view.reset_page();
        }
        if shortcuts.escape {
            if search.has_focus() {
                search.surrender_focus();
            } else if has_active_filters(view) {
                clear_filters(view);
            }
        }

        let active = active_filter_count(view);
        let label = if active > 0 {
            format!("{}  Filters · {active}", egui_phosphor::regular::FUNNEL)
        } else {
            format!("{}  Filters", egui_phosphor::regular::FUNNEL)
        };
        let filters = egui::Button::new(egui::RichText::new(label).size(12.0)).fill(
            if active > 0 {
                theme::SELECTED_ROW
            } else {
                theme::RAISED
            },
        );
        let filters = ui
            .add(filters)
            .on_hover_text("Category, rarity, majors, and item properties");
        let _ = egui::Popup::menu(&filters)
            .id(egui::Id::new(("items-filters", table_id.store_key())))
            .show(|ui| {
                ui.set_width(FILTER_WIDTH);
                filter_panel(ui, view, majors);
            });

        let columns = ui
            .add(
                egui::Button::new(
                    egui::RichText::new(egui_phosphor::regular::COLUMNS).size(12.0),
                )
                .fill(theme::RAISED),
            )
            .on_hover_text("Choose columns");
        let _ = egui::Popup::menu(&columns)
            .id(egui::Id::new(("items-columns", table_id.store_key())))
            .show(|ui| columns_panel(ui, view, table_id, column_edits));

        let sort = format!(
            "{}  {}",
            egui_phosphor::regular::ARROWS_DOWN_UP,
            view.sort.label()
        );
        let _ =
            egui::containers::menu::MenuButton::new(egui::RichText::new(sort).size(12.0)).ui(
                ui,
                |ui| {
                    ui.set_min_width(160.0);
                    for key in SortKey::ALL {
                        let checked = view.sort == key;
                        let text = if checked {
                            format!(
                                "{}  {}",
                                if view.sort_desc {
                                    egui_phosphor::regular::CARET_DOWN
                                } else {
                                    egui_phosphor::regular::CARET_UP
                                },
                                key.label()
                            )
                        } else {
                            format!("    {}", key.label())
                        };
                        if ui.selectable_label(checked, text).clicked() {
                            apply_sort(view, key);
                        }
                    }
                },
            );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if icon_button(
                ui,
                egui_phosphor::regular::DOWNLOAD_SIMPLE,
                !can_export,
                "Export CSV (casemove.csv in Downloads)",
            )
            .clicked()
            {
                *export = true;
            }
            let label = if prices_busy {
                "Updating prices…"
            } else {
                "Refresh prices"
            };
            let button = egui::Button::new(
                egui::RichText::new(format!(
                    "{}  {label}",
                    egui_phosphor::regular::ARROWS_CLOCKWISE
                ))
                .size(12.0),
            );
            if ui.add_enabled(!prices_busy, button).clicked() {
                *refresh_prices = true;
            }
        });
    });
    ui.add_space(6.0);
}

fn filter_panel(ui: &mut egui::Ui, view: &mut InventoryViewState, majors: &[String]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.label(
            egui::RichText::new(format!("{}  Filters", egui_phosphor::regular::FUNNEL))
                .size(theme::BODY_SIZE)
                .color(theme::TEXT_STRONG),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_enabled(
                    has_active_filters(view),
                    egui::Button::new(
                        egui::RichText::new(format!(
                            "{}  Clear all",
                            egui_phosphor::regular::FUNNEL_SIMPLE_X
                        ))
                        .size(theme::MUTED_SIZE),
                    )
                    .frame(false),
                )
                .clicked()
            {
                clear_filters(view);
            }
        });
    });
    ui.add_space(6.0);
    ui.separator();
    ui.add_space(4.0);

    // The groups can outgrow a tall window; the panel scrolls rather than
    // stretching past the screen.
    egui::ScrollArea::vertical()
        .id_salt("filters-body")
        .max_height(FILTER_MAX_HEIGHT)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            filter_group(ui, "Show");
            ui.checkbox(&mut view.units_only, "Storage units only");

            filter_group(ui, "Category");
            let categories: Vec<(&str, bool)> = domain::CATEGORIES
                .iter()
                .map(|(_, name)| (*name, view.selected_categories.contains(*name)))
                .collect();
            chip_group(ui, view, categories, |view, text| {
                inventory::toggle_filter(&mut view.selected_categories, text.to_owned());
            });

            filter_group(ui, "Rarity");
            let rarities: Vec<(&str, bool)> = (1..=7u32)
                .map(|rarity| {
                    let name = inventory::rarity_name(rarity);
                    (name, view.selected_rarities.contains(name))
                })
                .collect();
            chip_group(ui, view, rarities, |view, text| {
                inventory::toggle_filter(&mut view.selected_rarities, text.to_owned());
            });

            if !majors.is_empty() {
                filter_group(ui, "Majors");
                let list: Vec<(&str, bool)> = majors
                    .iter()
                    .map(|major| (major.as_str(), view.selected_majors.contains(major)))
                    .collect();
                chip_group(ui, view, list, |view, text| {
                    inventory::toggle_filter(&mut view.selected_majors, text.to_owned());
                });
            }

            filter_group(ui, "Properties");
            let properties: Vec<(&str, bool)> = [
                "Tradehold",
                "Nametag",
                "Moveable",
                "Equipped CT",
                "Equipped T",
                "Key",
            ]
            .into_iter()
            .map(|property| (property, view.selected_properties.contains(property)))
            .collect();
            chip_group(ui, view, properties, |view, text| {
                inventory::toggle_filter(&mut view.selected_properties, text.to_owned());
            });
        });
}

/// A section heading, nudged away from whatever ran before it.
fn filter_group(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(title)
            .size(theme::MUTED_SIZE)
            .color(theme::MUTED),
    );
    ui.add_space(3.0);
}

fn chip_group(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    items: Vec<(&str, bool)>,
    toggle: fn(&mut InventoryViewState, &str),
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(5.0, 5.0);
        for (text, selected) in items {
            if chip(ui, selected, text) {
                toggle(view, text);
                view.reset_page();
            }
        }
    });
}

fn columns_panel(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    table_id: TableId,
    edits: &mut ColumnEdits,
) {
    ui.set_min_width(180.0);
    ui.horizontal(|ui| {
        filter_label(ui, "COLUMNS");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reset = egui::Button::new(egui::RichText::new("Reset").size(11.0));
            if ui
                .add_enabled(view.columns != ColumnSet::default(), reset)
                .clicked()
            {
                view.columns = ColumnSet::default();
                edits.mark(table_id);
            }
        });
    });
    ui.add_space(2.0);
    for column in ColumnId::ALL {
        let mut visible = view.columns.is_visible(column);
        if ui.checkbox(&mut visible, column.label()).changed() {
            if visible {
                view.columns.visible.insert(column);
            } else {
                view.columns.visible.remove(&column);
            }
            edits.mark(table_id);
        }
    }
}

/// The filters currently applied, as chips that switch themselves off.
fn active_chips(ui: &mut egui::Ui, view: &mut InventoryViewState) {
    let mut entries: Vec<(u8, String)> = Vec::new();
    if view.units_only {
        entries.push((0, "Storage units only".to_owned()));
    }
    entries.extend(
        view.selected_categories
            .iter()
            .cloned()
            .map(|value| (1, value)),
    );
    entries.extend(
        view.selected_rarities
            .iter()
            .cloned()
            .map(|value| (2, value)),
    );
    entries.extend(view.selected_majors.iter().cloned().map(|value| (3, value)));
    entries.extend(
        view.selected_properties
            .iter()
            .cloned()
            .map(|value| (4, value)),
    );
    if entries.is_empty() {
        return;
    }

    let mut cleared_any = false;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.spacing_mut().item_spacing.y = 4.0;
        for (group, value) in entries {
            if chip(ui, true, &value) {
                match group {
                    0 => view.units_only = false,
                    1 => {
                        view.selected_categories.remove(&value);
                    }
                    2 => {
                        view.selected_rarities.remove(&value);
                    }
                    3 => {
                        view.selected_majors.remove(&value);
                    }
                    _ => {
                        view.selected_properties.remove(&value);
                    }
                }
                cleared_any = true;
            }
        }
    });
    if cleared_any {
        view.reset_page();
    }
    ui.add_space(4.0);
}

fn filter_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(10.0)
            .color(theme::MUTED)
            .strong(),
    );
}

fn chip(ui: &mut egui::Ui, selected: bool, text: &str) -> bool {
    // A Button lays itself out to exactly the text's size; a Frame around a
    // Label stretches to whatever row height the parent layout hands it.
    let text = egui::RichText::new(text)
        .size(theme::MUTED_SIZE)
        .color(if selected { egui::Color32::WHITE } else { theme::TEXT });
    ui.add(
        egui::Button::new(text)
            .corner_radius(99)
            .min_size(egui::vec2(0.0, 0.0))
            .fill(if selected {
                theme::CHIP_ACTIVE
            } else {
                egui::Color32::TRANSPARENT
            })
            .stroke(egui::Stroke::new(
                1.0_f32,
                if selected {
                    theme::CHIP_ACTIVE
                } else {
                    theme::BORDER
                },
            )),
    )
    .clicked()
}

fn active_filter_count(view: &InventoryViewState) -> usize {
    view.selected_categories.len()
        + view.selected_rarities.len()
        + view.selected_majors.len()
        + view.selected_properties.len()
        + usize::from(view.units_only)
}

fn has_active_filters(view: &InventoryViewState) -> bool {
    !view.query.trim().is_empty() || active_filter_count(view) > 0
}

fn clear_filters(view: &mut InventoryViewState) {
    view.query.clear();
    view.units_only = false;
    view.selected_categories.clear();
    view.selected_rarities.clear();
    view.selected_majors.clear();
    view.selected_properties.clear();
    view.reset_page();
}

/// The rows the table shows: the current filters applied to the grouped
/// inventory, in sort order. Shared with the overview charts.
pub fn filtered_rows<'a>(
    grouped: &'a [GroupedItem],
    view: &InventoryViewState,
) -> Vec<&'a GroupedItem> {
    let query = view.query.trim().to_lowercase();
    grouped
        .iter()
        .filter(|row| {
            inventory::matches_filters(
                row,
                &query,
                view.units_only,
                &view.selected_categories,
                &view.selected_rarities,
                &view.selected_majors,
                &view.selected_properties,
            )
        })
        .collect()
}

fn sort_rows(rows: &mut [&GroupedItem], view: &InventoryViewState) {
    inventory::sort_rows(rows, view.sort.mode());
    if view.sort_desc {
        rows.reverse();
    }
}

fn apply_sort(view: &mut InventoryViewState, key: SortKey) {
    if view.sort == key {
        view.sort_desc = !view.sort_desc;
    } else {
        view.sort = key;
        // Numeric columns are most useful biggest-first.
        view.sort_desc = matches!(key, SortKey::Price | SortKey::Quantity | SortKey::Float);
    }
    view.reset_page();
}

pub(crate) fn empty_state(ui: &mut egui::Ui, title: &str, action: Option<(&str, &str)>) -> bool {
    let mut clicked = false;
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(egui_phosphor::regular::ARCHIVE)
                .size(28.0)
                .color(theme::MUTED),
        );
        ui.add_space(10.0);
        ui.label(egui::RichText::new(title).size(14.0).color(theme::MUTED));
        if let Some((text, glyph)) = action {
            ui.add_space(10.0);
            if ui.add(primary_button(format!("{glyph}  {text}"))).clicked() {
                clicked = true;
            }
        }
    });
    ui.add_space(24.0);
    clicked
}

pub(crate) fn error_box(ui: &mut egui::Ui, text: &str, note: Option<&str>) {
    egui::Frame::NONE
        .fill(theme::SURFACE)
        .stroke(egui::Stroke::new(1.0_f32, theme::SEPARATOR))
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_max_width(760.0);
            ui.horizontal_top(|ui| {
                ui.label(
                    egui::RichText::new(egui_phosphor::regular::WARNING)
                        .size(13.0)
                        .color(theme::ERROR_STRONG),
                );
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(text).size(theme::BODY_SIZE).color(theme::ERROR));
                    if let Some(note) = note {
                        subtle(ui, note);
                    }
                });
            });
        });
    ui.add_space(6.0);
}

/// A transient success line after a move, withdrawal, or rename.
fn notice(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(egui_phosphor::regular::CHECK_CIRCLE)
                .size(12.0)
                .color(theme::GREEN),
        );
        ui.label(
            egui::RichText::new(text)
                .size(theme::MUTED_SIZE)
                .color(theme::GREEN),
        );
    });
}

fn loading(ui: &mut egui::Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.spinner();
        subtle(ui, text);
    });
}

fn primary_button(text: String) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(theme::CARD)).fill(theme::ACCENT)
}

/// A square icon-only button; `disabled` greys it out without consuming it.
fn icon_button(ui: &mut egui::Ui, glyph: &str, disabled: bool, hover: &str) -> egui::Response {
    let response = ui
        .add_enabled(
            !disabled,
            egui::Button::new(egui::RichText::new(glyph).size(13.0))
                .frame(false)
                .min_size(egui::vec2(24.0, 24.0)),
        )
        .on_hover_text(hover);
    if response.hovered() && !disabled {
        response
            .ctx
            .set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

// --- table -----------------------------------------------------------------

/// The space between table columns, in points. It is a fixed length rather
/// than a multiple of the text size, so a zoom step moves it with every other
/// length instead of letting it crowd or balloon next to the labels.
pub(crate) fn column_gap() -> f32 {
    theme::COLUMN_GAP
}

const FOOTER_HEIGHT: f32 = 44.0;
/// The pager's page buttons; the footer band grows with the zoom to hold them.
const PAGER_BUTTON: f32 = 28.0;
/// Breathing room between the table's last row and the footer bar.
const TABLE_GAP: f32 = 12.0;
/// Clearance between the page and the pager band under it.
const PAGE_BAND_GAP: f32 = 12.0;
/// The item column never shrinks below this, whatever else is visible.
const MIN_ITEM_WIDTH: f32 = 220.0;
/// Width of the accent bar marking a selected row's left edge.
const SELECTION_BAR_WIDTH: f32 = 3.0;
/// Base width of the free-growing text columns (wear, category).
const TEXT_COLUMN_WIDTH: f32 = 130.0;
/// The filter popover's fixed width and scrolling body cap.
const FILTER_WIDTH: f32 = 420.0;
const FILTER_MAX_HEIGHT: f32 = 460.0;
const THUMBNAIL: egui::Vec2 = egui::vec2(52.0, 40.0);
/// Sticker swatches, sized to sit inside a row without growing it.
const STICKER_THUMBNAIL: egui::Vec2 = egui::vec2(16.0, 20.0);

/// A column's identity; `Select`, `Item` and `Actions` are always on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cell {
    Item,
    Rarity,
    Wear,
    Qty,
    Price,
    Stickers,
    Seed,
    Tradehold,
    Category,
    Id,
    Actions,
}

impl Cell {
    fn from_column(column: ColumnId) -> Self {
        match column {
            ColumnId::Rarity => Self::Rarity,
            ColumnId::Wear => Self::Wear,
            ColumnId::Qty => Self::Qty,
            ColumnId::Price => Self::Price,
            ColumnId::Stickers => Self::Stickers,
            ColumnId::Seed => Self::Seed,
            ColumnId::Tradehold => Self::Tradehold,
            ColumnId::Category => Self::Category,
            ColumnId::Id => Self::Id,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Item => "Item",
            Self::Rarity => "Rarity",
            Self::Wear => "Float",
            Self::Qty => "Qty",
            Self::Price => "Price",
            Self::Stickers => "Stickers",
            Self::Seed => "Seed",
            Self::Tradehold => "Tradehold",
            Self::Category => "Category",
            Self::Id => "Item ID",
            Self::Actions => "",
        }
    }

    fn sort(self) -> Option<SortKey> {
        match self {
            Self::Item => Some(SortKey::Name),
            Self::Rarity => Some(SortKey::Rarity),
            Self::Wear => Some(SortKey::Float),
            Self::Qty => Some(SortKey::Quantity),
            Self::Price => Some(SortKey::Price),
            Self::Category => Some(SortKey::Category),
            Self::Tradehold => Some(SortKey::Tradehold),
            Self::Stickers | Self::Seed | Self::Id | Self::Actions => None,
        }
    }

    /// The column's comfortable width; `Item` claims what the others leave.
    /// Sized against the UI zoom so the labels keep their room at any scale.
    fn fixed_width(self, scale: f32) -> f32 {
        let base = match self {
            Self::Wear | Self::Category => TEXT_COLUMN_WIDTH,
            Self::Rarity => 72.0,
            Self::Qty => 40.0,
            Self::Price => 92.0,
            Self::Stickers => 56.0,
            Self::Seed => 52.0,
            Self::Tradehold => 72.0,
            Self::Id => 112.0,
            Self::Actions => 26.0,
            Self::Item => 0.0,
        };
        base * scale
    }

    fn width(self, spare: f32, scale: f32) -> egui_extras::Column {
        match self {
            Self::Item => egui_extras::Column::remainder().at_least(MIN_ITEM_WIDTH * scale),
            // Fixed columns size to their content so none spills right when
            // the window narrows.
            Self::Actions => egui_extras::Column::exact(self.fixed_width(scale)),
            _ => egui_extras::Column::initial(self.fixed_width(scale) + spare * self.stretch())
                .clip(true),
        }
    }

    /// How spare width is shared out: the text columns spread across the
    /// row, rarity and float take only what they need, numbers stay fixed.
    fn stretch(self) -> f32 {
        match self {
            Self::Category => 0.5,
            Self::Stickers => 0.2,
            Self::Seed | Self::Tradehold | Self::Id => 0.15,
            Self::Rarity | Self::Wear | Self::Qty | Self::Price => 0.0,
            Self::Item | Self::Actions => 0.0,
        }
    }
}

struct TableContext<'a> {
    out: &'a mut SectionOut,
    currency: &'a pricing::Currency,
    withdraw_busy: &'a Option<String>,
    section: Section,
    selectable: bool,
}

/// One row's click outcome, kept per shown row so range selection can be
/// applied after the table has finished laying rows out.
#[derive(Clone)]
struct InteractiveRow {
    id: String,
    quantity: usize,
    selectable: bool,
    shift: bool,
}

#[derive(Default)]
struct FooterState {
    clicked_row: Option<usize>,
    /// `(id, name)` of every shown row, for the selection summary.
    labels: Vec<(String, String)>,
}

fn table(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    rows: &[&GroupedItem],
    context: TableContext<'_>,
    footer: &mut FooterState,
    height: f32,
) -> Vec<InteractiveRow> {
    let section = context.section;

    let mut columns = vec![Cell::Item];
    for column in ColumnId::ALL {
        if view.columns.is_visible(column) {
            columns.push(Cell::from_column(column));
        }
    }
    columns.push(Cell::Actions);

    // Everything fits: fixed columns that would leave no room for the name
    // are dropped, and the rest share what is left.
    let scale = ui.ctx().zoom_factor();
    let width = ui.available_width();
    fit_columns(&mut columns, width, scale);
    let gaps = column_gap() * (columns.len().saturating_sub(1) as f32);
    let fixed: f32 = columns
        .iter()
        .filter(|cell| **cell != Cell::Item)
        .map(|cell| cell.fixed_width(scale))
        .sum();
    // Narrow columns give way first; if even the minimums overflow, the whole
    // table scrolls sideways rather than spilling past the card.
    let spare = (width - gaps - fixed).max(0.0);
    let content_width = if spare > 0.0 {
        width
    } else {
        gaps + fixed + MIN_ITEM_WIDTH * scale
    };

    // Wider gaps between columns, so each one reads as its own block.
    let gap = column_gap();
    ui.spacing_mut().item_spacing.x = gap;
    let sorted_desc = view.sort_desc;

    // Everything fits when the item column still gets its floor, give or take
    // the point egui rounds by; then the table renders directly and no scroll
    // area (and so no horizontal bar) exists at all.
    let inner_height = height;
    if content_width <= width + 1.0 {
        let inner = egui::Rect::from_min_size(ui.min_rect().min, egui::vec2(width, inner_height));
        return ui
            .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                table_body(ui, view, rows, context, footer, columns, sorted_desc, false)
            })
            .inner;
    }

    // Otherwise the table pans sideways. Its bars take their own strip, so
    // they never cover a row, and the horizontal strip is reserved up front:
    // egui only learns it is needed after laying the content out, which would
    // leave the last row hidden under it.
    ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
    let reserved = ui.spacing().scroll.bar_width + ui.spacing().scroll.bar_inner_margin;
    let inner_height = height - reserved;
    let mut output = Vec::new();
    egui::ScrollArea::both()
        .id_salt(("items-scroll", section_salt(section)))
        .auto_shrink([false, false])
        .max_height(inner_height)
        .min_scrolled_width(width)
        .show_viewport(ui, |ui, _| {
            let inner = egui::Rect::from_min_size(
                ui.min_rect().min,
                egui::vec2(content_width, inner_height),
            );
            output = ui
                .scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
                    table_body(ui, view, rows, context, footer, columns, sorted_desc, true)
                })
                .inner;
        });
    output
}

/// Lays out the header and rows inside an already-sized `ui`.
#[allow(clippy::too_many_arguments)]
fn table_body(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    rows: &[&GroupedItem],
    context: TableContext<'_>,
    footer: &mut FooterState,
    columns: Vec<Cell>,
    sorted_desc: bool,
    outer_scrolls: bool,
) -> Vec<InteractiveRow> {
    let TableContext {
        out,
        currency,
        withdraw_busy,
        section,
        selectable,
    } = context;
    // The inner `ui` is sized to the content, so `remainder` and the spare
    // share resolve against it rather than the viewport.
    let width = ui.available_width();
    let scale = ui.ctx().zoom_factor();
    let fixed: f32 = columns
        .iter()
        .filter(|cell| **cell != Cell::Item)
        .map(|cell| cell.fixed_width(scale))
        .sum();
    let spare = (width - column_gap() * (columns.len().saturating_sub(1) as f32) - fixed).max(0.0);
    let _ = withdraw_busy;
    let height = ui.available_height();
    let mut interactive: Vec<InteractiveRow> = Vec::with_capacity(rows.len());
    let shift = ui.input(|input| input.modifiers.shift);
    let header_rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(ui.available_width(), theme::HEADER_HEIGHT),
    );
    let painter = ui.painter().clone();
    let mut table_builder = egui_extras::TableBuilder::new(ui)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .id_salt(("items-table", section_salt(section)))
        // When an outer area pans sideways it already scrolls vertically, so
        // the table must not scroll a second time; on its own it does.
        .vscroll(!outer_scrolls)
        .auto_shrink([false, false])
        .min_scrolled_height(height)
        .max_scroll_height(height);
    for cell in &columns {
        table_builder = table_builder.column(cell.width(spare, scale));
    }

    let mut sort_click: Option<SortKey> = None;
    let table_builder = table_builder.header(theme::HEADER_HEIGHT, |mut header| {
        for cell in &columns {
            let active = cell.sort() == Some(view.sort);
            let (_, response) = header.col(|ui| {
                if *cell == Cell::Actions {
                    return;
                }
                header_label(ui, *cell, active);
                sort_caret(ui, active, sorted_desc);
            });
            if let Some(key) = cell.sort() {
                if response.hovered() {
                    response
                        .ctx
                        .set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                if response.clicked() {
                    sort_click = Some(key);
                }
            }
        }
    });
    if let Some(key) = sort_click {
        apply_sort(view, key);
    }

    table_builder.body(|body| {
        // A hairline under the header, as in the trade-up picker. It is drawn
        // from the first row once its cells exist (a row's response is only
        // valid after a `col`), which also covers an empty table.
        let mut header_drawn = false;
        body.rows(theme::ROW_HEIGHT, rows.len(), |mut row| {
            let index = row.index();
            let grouped = rows[index];
            let item = &grouped.representative;
            let qty = grouped.combined_qty;
            let selected = selectable && view.selected.contains_key(&item.id);

            let mut clicked = false;

            for cell in &columns {
                let (_, response) = row.col(|ui| {
                    cell_contents(
                        ui,
                        *cell,
                        item,
                        qty,
                        currency,
                        shows_storage_button(section),
                    );
                });
                if response.clicked() {
                    clicked = true;
                }
            }
            if clicked {
                footer.clicked_row = Some(index);
            }

            let response = row.response();
            if !header_drawn {
                header_drawn = true;
                painter.hline(
                    header_rect.x_range(),
                    response.rect.top(),
                    egui::Stroke::new(1.0_f32, theme::SEPARATOR),
                );
            }
            // A hairline closes every row, as on the reference table.
            painter.hline(
                header_rect.x_range(),
                response.rect.bottom(),
                egui::Stroke::new(1.0_f32, theme::SEPARATOR),
            );
            if selected {
                // A vertical accent bar down the row's left edge marks it as
                // selected, inset so it reads as part of the row, not a border.
                let rect = response.rect;
                painter.rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(rect.left(), rect.top() + 6.0),
                        egui::pos2(rect.left() + SELECTION_BAR_WIDTH, rect.bottom() - 6.0),
                    ),
                    egui::CornerRadius::same(2),
                    theme::CHIP_ACTIVE,
                );
            }
            if response.hovered() {
                response
                    .ctx
                    .set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let _ = response.context_menu(|ui| row_menu(ui, item, out, section));

            footer.labels.push((item.id.clone(), item.name.clone()));
            interactive.push(InteractiveRow {
                id: item.id.clone(),
                quantity: qty,
                selectable: selectable && selectable_row(item),
                shift,
            });
        });
    });
    interactive
}

/// Drops optional columns until the item column has at least
/// `MIN_ITEM_WIDTH`; the broad text columns go first, then the widest of the
/// rest, and `Select`/`Actions` always stay.
fn fit_columns(columns: &mut Vec<Cell>, width: f32, scale: f32) {
    let pinned = |cell: &Cell| matches!(cell, Cell::Item | Cell::Actions);
    while columns.len() > 2 {
        let fixed: f32 = columns
            .iter()
            .filter(|cell| **cell != Cell::Item)
            .map(|cell| cell.fixed_width(scale))
            .sum();
        if width - fixed >= MIN_ITEM_WIDTH * scale {
            return;
        }
        let widest = columns
            .iter()
            .enumerate()
            .filter(|(_, cell)| !pinned(cell))
            .max_by(|(_, left), (_, right)| {
                let broad = |cell: &&Cell| matches!(cell, Cell::Wear | Cell::Category);
                broad(left)
                    .cmp(&broad(right))
                    .then_with(|| left.fixed_width(scale).total_cmp(&right.fixed_width(scale)))
            })
            .map(|(index, _)| index);
        match widest {
            Some(index) => {
                columns.remove(index);
            }
            None => return,
        }
    }
}

fn selectable_row(item: &InventoryItem) -> bool {
    item.moveable && !item.is_storage_unit
}

fn section_salt(section: Section) -> &'static str {
    match section {
        Section::Inventory => "inventory",
        Section::Storage => "storage",
    }
}

fn shows_storage_button(section: Section) -> bool {
    matches!(section, Section::Inventory)
}

fn sort_caret(ui: &mut egui::Ui, active: bool, descending: bool) {
    if !active {
        return;
    }
    let caret = if descending {
        egui_phosphor::regular::CARET_DOWN
    } else {
        egui_phosphor::regular::CARET_UP
    };
    ui.label(
        egui::RichText::new(caret)
            .size(theme::MUTED_SIZE)
            .color(theme::TEXT_STRONG),
    );
}

fn header_label(ui: &mut egui::Ui, cell: Cell, active: bool) {
    let mut text = egui::RichText::new(cell.label())
        .size(theme::MUTED_SIZE)
        .color(if active { theme::TEXT_STRONG } else { theme::MUTED });
    if active {
        text = text.strong();
    }
    ui.label(text);
}

/// Cells fill their column, so contents start on the header label's x. `Qty`
/// is the exception: it is a right-aligned number, matching its header.
pub(crate) fn cell(ui: &mut egui::Ui, contents: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    let rect = ui.max_rect();
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
        ui.set_min_width(rect.width());
        contents(ui);
    })
    .response
}


fn cell_contents(
    ui: &mut egui::Ui,
    cell_id: Cell,
    item: &InventoryItem,
    qty: usize,
    currency: &pricing::Currency,
    storage_button: bool,
) -> egui::Response {
    cell(ui, |ui| {
        fill_cell(ui, cell_id, item, qty, currency, storage_button);
    })
}

fn fill_cell(
    ui: &mut egui::Ui,
    cell: Cell,
    item: &InventoryItem,
    qty: usize,
    currency: &pricing::Currency,
    storage_button: bool,
) {
    match cell {
        Cell::Item => {
            // Clear of the selection bar painted on the row's left edge.
            ui.add_space(SELECTION_BAR_WIDTH + 7.0);
            identity(ui, item, qty);
        }
        Cell::Rarity => {
            ui.label(
                egui::RichText::new(inventory::rarity_name(item.rarity))
                    .size(theme::MUTED_SIZE)
                    .color(theme::rarity_color(item.rarity)),
            );
        }
        Cell::Wear => {
            ui.label(
                egui::RichText::new(item.wear_name.as_deref().unwrap_or("—"))
                    .size(theme::BODY_SIZE),
            );
        }
        Cell::Qty => {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new(qty.to_string()).size(theme::BODY_SIZE));
            });
        }
        Cell::Price => match item.price {
            Some(price) => {
                ui.label(
                    egui::RichText::new(pricing::format_currency(
                        currency,
                        price * qty as f64,
                        2,
                    ))
                    .size(theme::BODY_SIZE),
                );
            }
            None => {
                muted(ui, "—");
            }
        },
        Cell::Stickers => {
            if item.stickers.is_empty() {
                muted(ui, "—");
            } else {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    for sticker in item.stickers.iter().take(4) {
                        sticker_thumbnail(ui, sticker.image_url.as_deref());
                    }
                    if item.stickers.len() > 4 {
                        ui.label(
                            egui::RichText::new(format!("+{}", item.stickers.len() - 4))
                                .size(10.0)
                                .color(theme::MUTED),
                        );
                    }
                })
                .response
                .on_hover_text(
                    item.stickers
                        .iter()
                        .map(|sticker| format!("{} · {}", sticker.name, sticker.kind))
                        .collect::<Vec<_>>()
                        .join("\n"),
                );
            }
        }
        Cell::Seed => match item.paint_seed {
            Some(seed) => {
                ui.label(
                    egui::RichText::new(seed.to_string())
                        .size(theme::MUTED_SIZE)
                        .color(theme::MUTED),
                );
            }
            None => {
                muted(ui, "—");
            }
        },
        Cell::Tradehold => match tradehold_days(item) {
            Some(days) => {
                ui.label(
                    egui::RichText::new(format!("{days}d"))
                        .size(theme::MUTED_SIZE)
                        .color(theme::GREEN),
                );
            }
            None => {
                muted(ui, "Tradable");
            }
        },
        Cell::Category => match item.category.as_deref() {
            Some(category) => {
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(category)
                            .size(theme::MUTED_SIZE)
                            .color(theme::MUTED),
                    )
                    .truncate(),
                );
            }
            None => {
                muted(ui, "—");
            }
        },
        Cell::Id => {
            ui.label(
                egui::RichText::new(&item.id)
                    .monospace()
                    .size(10.0)
                    .color(theme::MUTED),
            );
        }
        Cell::Actions => {
            if item.is_storage_unit && storage_button {
                ui.label(
                    egui::RichText::new(egui_phosphor::regular::CARET_RIGHT)
                        .size(theme::MUTED_SIZE)
                        .color(theme::MUTED),
                );
            }
        }
    }
}

fn muted(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(
        egui::RichText::new(text)
            .size(theme::MUTED_SIZE)
            .color(theme::MUTED),
    )
}

/// The item cell, laid out like the reference table: rarity dot, thumbnail,
/// then the name with its state badges on one line.
fn identity(ui: &mut egui::Ui, item: &InventoryItem, qty: usize) -> egui::Response {
    let dot = egui::vec2(7.0, 7.0);
    let (rect, _) = ui.allocate_exact_size(dot, egui::Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 3.5, theme::rarity_color(item.rarity));
    item_thumbnail(ui, item);

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let name = egui::RichText::new(&item.name)
            .size(theme::BODY_SIZE)
            .color(theme::TEXT_STRONG);
        ui.add(egui::Label::new(name).truncate());
        if qty > 1 {
            theme::state_badge(ui, &format!("×{qty}"));
        }
        if item.stat_trak {
            theme::state_badge(ui, "StatTrak");
        }
        if item.is_storage_unit {
            theme::state_badge(ui, "Unit");
        }
        if let Some(days) = tradehold_days(item) {
            theme::mint_badge(ui, &format!("{days}d hold"));
        }
    })
    .response
}

/// The item cell's thumbnail: a raised tile with a hairline border, the way
/// the picker framed its rows.
fn item_thumbnail(ui: &mut egui::Ui, item: &InventoryItem) {
    let (rect, _) = ui.allocate_exact_size(THUMBNAIL, egui::Sense::hover());
    ui.painter().rect_filled(rect, 3, theme::RAISED);
    let url = item.image_url.as_deref();
    thumbnail(ui, rect, url, egui_phosphor::regular::CUBE, theme::ICON);
    ui.painter().rect_stroke(
        rect.expand(0.5),
        3,
        egui::Stroke::new(1.0_f32, theme::SEPARATOR),
        egui::StrokeKind::Inside,
    );
}

/// Paint `url` fitted into `rect`, or `fallback` when no texture is cached.
fn thumbnail(
    ui: &egui::Ui,
    rect: egui::Rect,
    url: Option<&str>,
    fallback: &str,
    color: egui::Color32,
) {
    let texture = url
        .filter(|url| !url.is_empty())
        .and_then(images::cached);
    match texture {
        Some(texture) => {
            let natural = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
            let scale = (rect.width() / natural.x).min(rect.height() / natural.y);
            let fitted = natural * scale;
            egui::Image::new((texture.id(), fitted))
                .fit_to_exact_size(fitted)
                .paint_at(ui, egui::Rect::from_center_size(rect.center(), fitted));
        }
        None => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                fallback,
                egui::FontId::proportional(16.0),
                color,
            );
        }
    }
}

/// The sticker cell: one small thumbnail per sticker, then the overflow count.
fn sticker_thumbnail(ui: &mut egui::Ui, url: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(STICKER_THUMBNAIL, egui::Sense::hover());
    thumbnail(ui, rect, url, egui_phosphor::regular::SEAL, theme::MUTED);
}

fn tradehold_days(item: &InventoryItem) -> Option<i64> {
    let until = item.trade_unlock?;
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    let delta = until - now_ms;
    let days = delta.div_euclid(86_400_000) + i64::from(delta.rem_euclid(86_400_000) > 0);
    (days > 0).then_some(days)
}

/// The row's right-click menu.
fn row_menu(ui: &mut egui::Ui, item: &InventoryItem, out: &mut SectionOut, section: Section) {
    ui.set_min_width(210.0);
    let storage = matches!(section, Section::Storage);
    if item.is_storage_unit {
        if ui
            .button(icon_menu(egui_phosphor::regular::FOLDER_OPEN, "Open contents"))
            .clicked()
        {
            out.open_storage = Some((item.id.clone(), item.name.clone()));
            ui.close();
        }
        if ui
            .button(icon_menu(egui_phosphor::regular::PENCIL_SIMPLE, "Rename unit"))
            .clicked()
        {
            let name = item
                .custom_name
                .clone()
                .filter(|name| !name.is_empty())
                .unwrap_or_else(|| item.name.clone());
            out.rename = Some((item.id.clone(), name));
            ui.close();
        }
        ui.separator();
    } else if storage {
        if item.moveable
            && ui
                .button(icon_menu(
                    egui_phosphor::regular::DOWNLOAD_SIMPLE,
                    "Withdraw to inventory",
                ))
                .clicked()
        {
            out.withdraw = Some(item.id.clone());
            ui.close();
        }
    } else if item.moveable {
        if crate::inventory::matches_property(item, "Container")
            && ui
                .button(icon_menu(egui_phosphor::regular::PACKAGE, "Open container"))
                .clicked()
        {
            out.open_container = Some(item.clone());
            ui.close();
        }
        if ui
            .button(icon_menu(
                egui_phosphor::regular::UPLOAD_SIMPLE,
                "Move to storage",
            ))
            .clicked()
        {
            out.move_to = Some((item.id.clone(), item.name.clone()));
            ui.close();
        }
        ui.separator();
    }
    if ui
        .button(icon_menu(egui_phosphor::regular::COPY, "Copy name"))
        .clicked()
    {
        out.clipboard = Some(item.name.clone());
        ui.close();
    }
    if ui
        .button(icon_menu(egui_phosphor::regular::COPY_SIMPLE, "Copy item ID"))
        .clicked()
    {
        out.clipboard = Some(item.id.clone());
        ui.close();
    }
}

fn icon_menu(glyph: &str, text: &str) -> String {
    format!("{glyph}  {text}")
}

/// The pager band at the bottom of the view.
#[derive(Default)]
struct Footer {
    /// Filtered rows across every page, before paging.
    total: usize,
    page: usize,
    pages: usize,
    /// Every selectable row the filters allow, across all pages, so "Select
    /// all" can take the whole result set rather than the page in view.
    all_rows: Vec<(String, usize)>,
    /// `(row id, item name)` for the shown batch, so the selection strip can
    /// name what is about to move.
    labels: Vec<(String, String)>,
}

fn footer_bar(ui: &mut egui::Ui, view: &mut InventoryViewState, out: &mut SectionOut, footer: Footer) {
    let Footer {
        total,
        page,
        pages,
        all_rows,
        labels,
    } = footer;

    // An opaque band: nothing shows through from behind it.
    let rect = ui.available_rect_before_wrap();
    ui.painter().rect_filled(rect, 0, theme::BG);
    // A hairline separates the pager from the card above, like the reference.
    ui.painter().hline(
        rect.x_range(),
        rect.top(),
        egui::Stroke::new(1.0_f32, theme::SEPARATOR),
    );

    ui.horizontal_centered(|ui| {
        // Left: what is selected, if anything.
        let selected: usize = view.selected.values().sum();
        if selected > 0 {
            ui.spacing_mut().item_spacing.x = 6.0;
            theme::state_badge(ui, &format!("{selected} selected"));
            let names: Vec<&str> = labels
                .iter()
                .filter(|(id, _)| view.selected.contains_key(id))
                .map(|(_, name)| name.as_str())
                .take(2)
                .collect();
            if !names.is_empty() {
                let extra = view.selected.len().saturating_sub(names.len());
                let text = if extra > 0 {
                    format!("{} +{extra} more", names.join(", "))
                } else {
                    names.join(", ")
                };
                muted(ui, &text);
            }
            let move_button = egui::Button::new(egui::RichText::new(format!(
                "{}  Move",
                egui_phosphor::regular::UPLOAD_SIMPLE
            )))
            .fill(theme::CARD)
            .stroke(egui::Stroke::new(1.0_f32, theme::BORDER));
            if ui.add(move_button).clicked() {
                out.move_selected = true;
            }
            if ui.button("Clear").clicked() {
                view.selected.clear();
                view.anchor = None;
            }
        } else {
            muted(ui, &format!("{} {}", total, if total == 1 { "row" } else { "rows" }));
        }

        // Centre: the numbered pager, ‹ 1 2 3 4 … 37 ›.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let width = ui.available_width();
            ui.allocate_ui_with_layout(
                egui::vec2(width, 30.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    // A zero-width spacer centres the strip inside the card.
                    let strip = pager_strip_width(pages);
                    ui.add_space(((width - strip) / 2.0).max(0.0));
                    pager(ui, view, page, pages);
                },
            );
        });

        // Right: select every filtered row, not just the page in view.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if !all_rows.is_empty() {
                let all = all_rows
                    .iter()
                    .all(|(id, _)| view.selected.contains_key(id));
                let label = if all { "Clear selection" } else { "Select all" };
                let button = egui::Button::new(egui::RichText::new(label).size(theme::BODY_SIZE))
                    .fill(if all { theme::PAGE_ACTIVE } else { theme::CARD });
                if ui
                    .add(button)
                    .on_hover_text(if all {
                        "Clears the selection"
                    } else {
                        "Selects every row the filters allow, across all pages"
                    })
                    .clicked()
                {
                    if all {
                        view.selected.clear();
                        view.anchor = None;
                    } else {
                        // Start from a clean slate so stale ids from earlier
                        // filters cannot ride along into a move.
                        view.selected.clear();
                        for (id, quantity) in &all_rows {
                            view.selected.insert(id.clone(), *quantity);
                        }
                        view.anchor = None;
                    }
                }
            }
        });
    });
}

/// Approximate strip width so the pager can be centred before it is laid out:
/// a 26px button, a 6px gap, and page slots of `‹ 1 2 3 4 … 37 ›`.
fn pager_strip_width(pages: usize) -> f32 {
    let slots = pages.clamp(2, 7) as f32;
    slots * 26.0 + (slots - 1.0) * 6.0
}

fn pager(ui: &mut egui::Ui, view: &mut InventoryViewState, page: usize, pages: usize) {
    ui.spacing_mut().item_spacing.x = 6.0;

    let mut next_page: Option<usize> = None;
    let step = |ui: &mut egui::Ui, glyph: &str, enabled: bool| -> bool {
        let button = egui::Button::new(egui::RichText::new(glyph).size(13.0))
            .frame(false)
            .min_size(egui::vec2(26.0, 26.0));
        ui.add_enabled(enabled, button)
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .clicked()
    };

    if step(ui, egui_phosphor::regular::CARET_LEFT, page > 0) {
        next_page = Some(page.saturating_sub(1));
    }
    for slot in page_slots(page, pages) {
        match slot {
            Some(index) => {
                let current = index == page;
                let button = egui::Button::new(
                    egui::RichText::new((index + 1).to_string())
                        .size(12.0)
                        .color(if current {
                            theme::TEXT_STRONG
                        } else {
                            theme::MUTED
                        }),
                )
                .fill(if current {
                    theme::PAGE_ACTIVE
                } else {
                    egui::Color32::TRANSPARENT
                })
                .stroke(if current {
                    egui::Stroke::NONE
                } else {
                    egui::Stroke::new(1.0_f32, theme::SEPARATOR)
                })
                .min_size(egui::vec2(28.0, 28.0));
                if ui.add(button).clicked() && !current {
                    next_page = Some(index);
                }
            }
            None => {
                ui.allocate_ui_with_layout(
                    egui::vec2(26.0, 26.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| muted(ui, "…"),
                );
            }
        }
    }
    if step(ui, egui_phosphor::regular::CARET_RIGHT, page + 1 < pages) {
        next_page = Some((page + 1).min(pages - 1));
    }
    if let Some(next) = next_page {
        view.page = next;
    }
}

/// Which page buttons to show: first, last, the current window, and an
/// ellipsis wherever the strip skips pages.
fn page_slots(page: usize, pages: usize) -> Vec<Option<usize>> {
    const WINDOW: usize = 7;
    if pages <= WINDOW {
        return (0..pages).map(Some).collect();
    }
    // The reference's strip: 1 2 3 4 … 37, sliding with the current page.
    let first = page.saturating_sub(2).min(pages - 4);
    let last = first + 3;
    let mut slots: Vec<Option<usize>> = (first..=last).map(Some).collect();
    if first > 0 {
        // Keep the first page reachable when the window has slid away.
        slots.insert(0, Some(0));
        slots.insert(1, None);
    }
    if last < pages - 1 {
        slots.push(None);
        slots.push(Some(pages - 1));
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui;

    fn item(id: u64, name: &str, rarity: u32, wear: Option<f64>, price: Option<f64>) -> GroupedItem {
        GroupedItem {
            representative: InventoryItem {
                id: id.to_string(),
                definition_index: 7,
                name: name.to_owned(),
                custom_name: None,
                image_url: None,
                image_path: None,
                quantity: 1,
                rarity,
                position: id as u32,
                wear,
                wear_name: wear.map(|_| "Factory New".to_owned()),
                paint_seed: Some(1),
                stat_trak: false,
                is_storage_unit: false,
                storage_item_count: None,
                category: Some("Skins & Knives".to_owned()),
                major: None,
                trade_unlock: None,
                moveable: true,
                equipped_ct: false,
                equipped_t: false,
                price,
                collection: None,
                trade_up_eligible: true,
                stickers: Vec::new(),
                music_kit: None,
                graffiti: None,
            },
            combined_ids: vec![id.to_string()],
            combined_qty: 1,
        }
    }

    #[test]
    fn rows_render_headlessly() {
        let ctx = egui::Context::default();
        let app = item(1, "★ Karambit | Doppler", 6, Some(0.0123), Some(1200.0));
        let storage = item(2, "Prisma 2 Case", 1, None, None);
        let wide = egui::Context::default();
        for ctx in [&ctx, &wide] {
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    for cell in [Cell::Item, Cell::Rarity, Cell::Wear, Cell::Qty, Cell::Price, Cell::Stickers, Cell::Seed, Cell::Tradehold, Cell::Category, Cell::Id, Cell::Actions] {
                        ui.horizontal(|ui| {
                            cell_contents(ui, cell, &app.representative, 3, &pricing::Currency::new("USD"), true);
                            cell_contents(ui, cell, &storage.representative, 1, &pricing::Currency::new("USD"), false);
                        });
                    }
                });
            });
        }
    }

    #[test]
    fn header_hairline_reads_the_row_response_after_its_cells() {
        // Regression: reading `TableRow::response` before any `col` panics.
        let ctx = egui::Context::default();
        let app = item(1, "★ Karambit | Doppler", 6, Some(0.0123), Some(1200.0));
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let painter = ui.painter().clone();
                let mut header_rect = None;
                let table = egui_extras::TableBuilder::new(ui)
                    .column(egui_extras::Column::remainder())
                    .header(theme::HEADER_HEIGHT, |mut header| {
                        header_rect = Some(header.col(|_| {}).0);
                    })
                    .body(|body| {
                        body.rows(theme::ROW_HEIGHT, 1, |mut row| {
                            row.col(|ui| {
                                cell_contents(
                                    ui,
                                    Cell::Item,
                                    &app.representative,
                                    1,
                                    &pricing::Currency::new("USD"),
                                    true,
                                );
                            });
                            let top = row.response().rect.top();
                            if let Some(rect) = header_rect {
                                painter.hline(
                                    rect.x_range(),
                                    top,
                                    egui::Stroke::new(1.0_f32, theme::SEPARATOR),
                                );
                            }
                        });
                    });
                let _ = table;
            });
        });
    }

    #[test]
    fn wide_content_sets_a_minimum_scroll_width() {
        // Mirrors the budget `table` uses: if the fixed minimums plus the
        // item floor exceed the window, the content is laid out wider than
        // the viewport so the scroll area can pan.
        let columns = [Cell::Item, Cell::Rarity, Cell::Wear, Cell::Qty];
        let gaps = theme::COLUMN_GAP * (columns.len() - 1) as f32;
        let fixed: f32 = columns
            .iter()
            .filter(|cell| **cell != Cell::Item)
            .map(|cell| cell.fixed_width(1.0))
            .sum();
        let narrow = 300.0;
        assert!(narrow < gaps + fixed + MIN_ITEM_WIDTH);
        let content = gaps + fixed + MIN_ITEM_WIDTH;
        assert!(content > narrow, "content overflows the viewport");
    }

    #[test]
    fn header_click_sorts_and_toggles_direction() {
        let mut view = InventoryViewState::default();
        apply_sort(&mut view, SortKey::Price);
        assert_eq!(view.sort, SortKey::Price);
        assert!(view.sort_desc, "prices start largest-first");
        apply_sort(&mut view, SortKey::Price);
        assert!(!view.sort_desc, "a second click flips direction");
        apply_sort(&mut view, SortKey::Name);
        assert_eq!(view.sort, SortKey::Name);
        assert!(!view.sort_desc, "text sorts start smallest-first");
        assert_eq!(view.page, 0, "a new sort starts at the first page");
    }

    #[test]
    fn click_toggles_and_shift_extends_the_range() {
        let rows: Vec<InteractiveRow> = (0..5)
            .map(|index| InteractiveRow {
                id: index.to_string(),
                quantity: 1,
                selectable: true,
                shift: false,
            })
            .collect();
        let mut view = InventoryViewState::default();
        apply_row_click(&mut view, &rows, 1);
        assert!(view.selected.contains_key("1"));
        apply_row_click(&mut view, &rows, 1);
        assert!(view.selected.is_empty(), "clicking a selected row clears it");

        apply_row_click(&mut view, &rows, 0);
        let mut shifted = rows.clone();
        for row in &mut shifted {
            row.shift = true;
        }
        apply_row_click(&mut view, &shifted, 3);
        assert_eq!(view.selected.len(), 4, "0..=3 selected from the anchor");
        assert!(view.selected.contains_key("3"));
    }

    #[test]
    fn rarity_and_float_give_up_width_first() {
        // They used to claim a share of spare width; now they only take what
        // their content needs so other columns can fit.
        assert_eq!(Cell::Rarity.stretch(), 0.0);
        assert_eq!(Cell::Wear.stretch(), 0.0);
        assert!(Cell::Wear.stretch() < Cell::Category.stretch());
        assert!(Cell::Rarity.fixed_width(1.0) <= 72.0);
    }

    #[test]
    fn column_widths_follow_the_ui_zoom() {
        // Columns that carry text grow with the zoom so their labels are not
        // squeezed or clipped; the gap stays a fixed number of points.
        assert_eq!(Cell::Wear.fixed_width(2.0), Cell::Wear.fixed_width(1.0) * 2.0);
        assert_eq!(Cell::Id.fixed_width(1.5), Cell::Id.fixed_width(1.0) * 1.5);
        assert_eq!(theme::COLUMN_GAP, column_gap());
    }

    #[test]
    fn narrow_tables_drop_the_widest_optional_columns() {
        let mut columns = vec![
            Cell::Item,
            Cell::Category,
            Cell::Wear,
            Cell::Qty,
            Cell::Price,
            Cell::Stickers,
            Cell::Tradehold,
            Cell::Rarity,
            Cell::Actions,
        ];
        fit_columns(&mut columns, 1200.0, 1.0);
        assert_eq!(columns.len(), 9, "a wide window keeps everything");

        fit_columns(&mut columns, 600.0, 1.0);
        assert_eq!(columns.first(), Some(&Cell::Item));
        assert_eq!(columns.last(), Some(&Cell::Actions));
        let fixed: f32 = columns
            .iter()
            .filter(|cell| **cell != Cell::Item)
            .map(|cell| cell.fixed_width(1.0))
            .sum();
        assert!(600.0 - fixed >= MIN_ITEM_WIDTH);
        assert!(!columns.contains(&Cell::Category), "widest dropped first");
    }

    #[test]
    fn pager_slots_keep_first_last_and_window() {
        assert_eq!(page_slots(0, 3), vec![Some(0), Some(1), Some(2)]);
        let slots = page_slots(6, 12);
        assert_eq!(slots.first(), Some(&Some(0)));
        assert_eq!(slots.last(), Some(&Some(11)));
        assert!(slots.contains(&Some(6)), "the current page is always shown");
        assert!(slots.iter().any(Option::is_none), "a skip renders an ellipsis");
        assert!(slots.len() <= 8, "at most four pages plus two ellipses and two ends");
    }

    #[test]
    fn pages_of_fifty() {
        assert_eq!(50usize.div_ceil(PAGE_SIZE), 1);
        assert_eq!(581usize.div_ceil(PAGE_SIZE), 12);
        assert_eq!(0usize.div_ceil(PAGE_SIZE).max(1), 1);
    }

    #[test]
    fn page_changes_reset_on_new_results() {
        let mut view = InventoryViewState {
            page: 5,
            ..Default::default()
        };
        view.reset_page();
        assert_eq!(view.page, 0);
    }

    #[test]
    fn select_all_covers_every_page_not_just_the_visible_one() {
        // 120 movable rows = 3 pages of 50; the footer's select-all takes the
        // whole filtered set, and replaces any stale selection from before.
        let rows: Vec<GroupedItem> = (0..120)
            .map(|id| item(id, "Prisma 2 Case", 1, None, None))
            .collect();
        let filtered: Vec<&GroupedItem> = rows.iter().collect();
        let all_rows: Vec<(String, usize)> = filtered
            .iter()
            .filter(|row| selectable_row(&row.representative))
            .map(|row| (row.representative.id.clone(), row.combined_qty))
            .collect();
        assert_eq!(all_rows.len(), 120, "every filtered row, not PAGE_SIZE");
        assert!(all_rows.len() > PAGE_SIZE);

        let mut view = InventoryViewState::default();
        view.selected.insert("stale".to_owned(), 1);
        view.selected.clear();
        for (id, quantity) in &all_rows {
            view.selected.insert(id.clone(), *quantity);
        }
        assert_eq!(view.selected.len(), 120);
        assert!(!view.selected.contains_key("stale"));
    }

    #[test]
    fn storage_units_are_not_selectable_through_select_all() {
        let mut unit = item(9, "Prisma 2 Case", 1, None, None);
        unit.representative.is_storage_unit = true;
        assert!(!selectable_row(&unit.representative));
    }
}
