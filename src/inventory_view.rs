//! Inventory and storage-unit contents: toolbar, filters, sortable table,
//! and pagination. Both sections share the same chrome so the storage view
//! is just the inventory view over a different dataset.

use crate::app::CasemoveApp;
use crate::images::{self, ImageCache};
use crate::inventory::{self, GroupedItem};
use crate::theme;
use app_commands::InventoryItem;
use eframe::egui;
use std::collections::HashSet;

const PAGE_SIZE: usize = inventory::PAGE_SIZE;

/// Which dataset a section renders: the loaded inventory or the open storage
/// unit's contents.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Inventory,
    Storage,
}

/// Optional table columns; the item id stays hidden until asked for.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Columns {
    pub rarity: bool,
    pub wear: bool,
    pub seed: bool,
    pub qty: bool,
    pub price: bool,
    pub id: bool,
    pub actions: bool,
}

impl Default for Columns {
    fn default() -> Self {
        Self {
            rarity: true,
            wear: true,
            seed: true,
            qty: true,
            price: true,
            id: false,
            actions: true,
        }
    }
}

pub struct InventoryViewState {
    pub query: String,
    pub sort: String,
    pub sort_desc: bool,
    pub units_only: bool,
    pub page: usize,
    pub selected_categories: HashSet<String>,
    pub selected_rarities: HashSet<String>,
    pub selected_majors: HashSet<String>,
    pub selected_properties: HashSet<String>,
    pub columns: Columns,
}

impl Default for InventoryViewState {
    fn default() -> Self {
        Self {
            query: String::new(),
            sort: "position".to_owned(),
            sort_desc: false,
            units_only: false,
            page: 0,
            selected_categories: HashSet::new(),
            selected_rarities: HashSet::new(),
            selected_majors: HashSet::new(),
            selected_properties: HashSet::new(),
            columns: Columns::default(),
        }
    }
}

/// Global key handling inside a section.
struct Shortcuts {
    focus_search: bool,
    escape: bool,
}

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut back = false;
    let mut refresh = false;
    let mut sign_out = false;
    let mut open_storage: Option<(String, String)> = None;
    let mut clipboard: Option<String> = None;

    let shortcuts = Shortcuts {
        focus_search: ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::F)),
        escape: ui.input(|i| i.key_pressed(egui::Key::Escape))
            && !egui::Popup::is_any_open(ui.ctx()),
    };

    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: theme::PAGE_MARGIN,
            right: theme::PAGE_MARGIN,
            top: 14,
            bottom: 10,
        })
        .show(ui, |ui| {
            header(ui, app, &mut back, &mut refresh, &mut sign_out);
            if !app.schema_notice.is_empty() {
                ui.label(
                    egui::RichText::new(&app.schema_notice)
                        .size(11.0)
                        .color(theme::MUTED),
                );
            }
            ui.add_space(8.0);
            if app.storage_selection.is_some() {
                storage_section(ui, app, &shortcuts, &mut clipboard);
            } else {
                main_section(ui, app, &shortcuts, &mut open_storage, &mut clipboard);
            }
        });

    if back {
        app.close_storage_contents();
    }
    if refresh {
        app.refresh_inventory();
    }
    if sign_out {
        app.sign_out();
    }
    if let Some((casket_id, name)) = open_storage {
        app.open_storage_contents(casket_id, name);
    }
    if let Some(text) = clipboard {
        ui.ctx().copy_text(text);
    }
    if app.view.page != app.saved_page {
        app.saved_page = app.view.page;
        app.save_page();
    }
}

fn header(
    ui: &mut egui::Ui,
    app: &mut CasemoveApp,
    back: &mut bool,
    refresh: &mut bool,
    sign_out: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            let title = if app.storage_selection.is_some() {
                "Storage contents"
            } else {
                "Inventory"
            };
            ui.label(egui::RichText::new(title).size(theme::TITLE_SIZE).strong());
            ui.label(
                egui::RichText::new(header_status(app))
                    .size(theme::MUTED_SIZE)
                    .color(theme::MUTED),
            );
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if ui
                .add(egui::Button::new(egui::RichText::new(
                    egui_phosphor::regular::SIGN_OUT,
                )))
                .on_hover_text("Sign out")
                .clicked()
            {
                *sign_out = true;
            }
            if app.storage_selection.is_some() {
                let back_button =
                    egui::Button::new(format!("{}  Inventory", egui_phosphor::regular::ARROW_LEFT));
                if ui.add(back_button).clicked() {
                    *back = true;
                }
            } else {
                let text = if app.inventory_busy {
                    "Loading..."
                } else if app.inventory.is_some() {
                    "Refresh"
                } else {
                    "Connect to CS2"
                };
                let refresh_button = egui::Button::new(
                    egui::RichText::new(format!(
                        "{}  {text}",
                        egui_phosphor::regular::ARROW_CLOCKWISE
                    ))
                    .color(egui::Color32::WHITE),
                )
                .fill(theme::ACCENT);
                if ui
                    .add_enabled(!app.inventory_busy, refresh_button)
                    .clicked()
                {
                    *refresh = true;
                }
            }
        });
    });
    ui.separator();
}

fn header_status(app: &CasemoveApp) -> String {
    if app.storage_selection.is_some() {
        if app.storage_busy {
            "Loading storage contents...".to_owned()
        } else {
            app.storage_contents
                .as_ref()
                .map(|items| format!("{} items", items.len()))
                .unwrap_or_else(|| "Contents not loaded".into())
        }
    } else if app.inventory_busy {
        "Waiting for the CS2 Game Coordinator...".to_owned()
    } else {
        app.inventory
            .as_ref()
            .map(|snapshot| format!("{} items loaded", snapshot.items.len()))
            .unwrap_or_else(|| "Not loaded".into())
    }
}

fn main_section(
    ui: &mut egui::Ui,
    app: &mut CasemoveApp,
    shortcuts: &Shortcuts,
    open_storage: &mut Option<(String, String)>,
    clipboard: &mut Option<String>,
) {
    if app.inventory.is_none() {
        ui.horizontal_top(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::INFO)
                    .size(14.0)
                    .color(theme::STATUS_TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "Close any running game before connecting. Steam will mark this account as playing CS2.",
                )
                .size(theme::MUTED_SIZE)
                .color(theme::STATUS_TEXT),
            );
        });
        ui.add_space(8.0);
    }

    if !app.inventory_error.is_empty() {
        let note = app
            .inventory
            .is_some()
            .then_some("Showing the last loaded snapshot.");
        error_box(ui, &app.inventory_error, note);
    }

    if app.inventory.is_none() {
        if !app.inventory_busy && app.inventory_error.is_empty() {
            let connect = empty_state(
                ui,
                "Inventory not loaded",
                Some((
                    "Connect to CS2",
                    egui_phosphor::regular::ARROW_CLOCKWISE,
                )),
            );
            if connect {
                app.refresh_inventory();
            }
        }
        return;
    }

    items_section(
        ui,
        app,
        Section::Inventory,
        shortcuts,
        open_storage,
        clipboard,
    );
}

fn storage_section(
    ui: &mut egui::Ui,
    app: &mut CasemoveApp,
    shortcuts: &Shortcuts,
    clipboard: &mut Option<String>,
) {
    let name = app
        .storage_selection
        .as_ref()
        .map(|(_, name)| name.clone())
        .unwrap_or_default();
    ui.label(egui::RichText::new(name).size(theme::BODY_SIZE).color(theme::MUTED));
    ui.add_space(4.0);

    if !app.storage_error.is_empty() {
        error_box(ui, &app.storage_error, None);
    }
    if app.storage_busy {
        ui.label(
            egui::RichText::new("Loading storage contents...")
                .size(theme::MUTED_SIZE)
                .color(theme::MUTED),
        );
        return;
    }
    if app.storage_contents.is_none() {
        return;
    }
    if app.storage_grouped.is_empty() {
        empty_state(ui, "Storage unit is empty", None);
        return;
    }

    items_section(
        ui,
        app,
        Section::Storage,
        shortcuts,
        &mut None::<(String, String)>,
        clipboard,
    );
}

/// Toolbar, active-filter chips, table, and footer for one dataset.
fn items_section(
    ui: &mut egui::Ui,
    app: &mut CasemoveApp,
    section: Section,
    shortcuts: &Shortcuts,
    open_storage: &mut Option<(String, String)>,
    clipboard: &mut Option<String>,
) {
    let view: &mut InventoryViewState = match section {
        Section::Inventory => &mut app.view,
        Section::Storage => &mut app.storage_view,
    };
    let majors: &Vec<String> = match section {
        Section::Inventory => &app.majors,
        Section::Storage => &app.storage_majors,
    };
    let grouped: &Vec<GroupedItem> = match section {
        Section::Inventory => &app.grouped,
        Section::Storage => &app.storage_grouped,
    };
    let prices_busy = app.prices_busy;
    let hint = match section {
        Section::Inventory => "Search inventory",
        Section::Storage => "Search contents",
    };

    let mut refresh_prices = false;
    toolbar(
        ui,
        view,
        majors,
        prices_busy,
        hint,
        shortcuts,
        &mut refresh_prices,
    );
    active_chips(ui, view);

    let mut rows = visible_rows(grouped, view);
    inventory::sort_rows(&mut rows, &view.sort);
    if view.sort_desc {
        rows.reverse();
    }

    let total = rows.len();
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    let page = view.page.min(pages - 1);
    if view.page != page {
        view.page = page;
    }
    let start = page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);

    let mut clear = false;
    if total == 0 {
        let action = has_active_filters(view).then_some((
            "Clear filters",
            egui_phosphor::regular::FUNNEL_SIMPLE_X,
        ));
        if empty_state(ui, "No matching items", action) {
            clear = true;
        }
    } else {
        let value: f64 = rows
            .iter()
            .map(|row| row.representative.price.unwrap_or(0.0) * row.combined_qty as f64)
            .sum();
        let value_label = match section {
            Section::Inventory => format!("Inventory value: {}", pricing::format_usd(value)),
            Section::Storage => format!("Contents value: {}", pricing::format_usd(value)),
        };

        table(
            ui,
            &mut app.images,
            view,
            &rows[start..end],
            open_storage,
            clipboard,
            section == Section::Inventory,
        );
        ui.add_space(6.0);
        footer(ui, view, total, start, end, value_label);
    }

    if clear {
        clear_filters(view);
    }
    if refresh_prices {
        app.refresh_live_prices(true);
    }
}

fn toolbar(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    majors: &[String],
    prices_busy: bool,
    hint: &str,
    shortcuts: &Shortcuts,
    refresh_prices: &mut bool,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;

        ui.label(
            egui::RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                .size(13.0)
                .color(theme::MUTED),
        );
        let search = ui.add(
            egui::TextEdit::singleline(&mut view.query)
                .hint_text(hint)
                .desired_width(240.0),
        );
        if shortcuts.focus_search {
            search.request_focus();
        }
        if search.changed() {
            view.page = 0;
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
            format!("{}  Filters ({active})", egui_phosphor::regular::FUNNEL)
        } else {
            format!("{}  Filters", egui_phosphor::regular::FUNNEL)
        };
        let filters = ui.button(egui::RichText::new(label).size(12.0));
        let _ = egui::Popup::menu(&filters)
            .id(egui::Id::new("items-filters"))
            .show(|ui| filter_panel(ui, view, majors));

        let columns = ui
            .button(egui::RichText::new(egui_phosphor::regular::COLUMNS))
            .on_hover_text("Choose columns");
        let _ = egui::Popup::menu(&columns)
            .id(egui::Id::new("items-columns"))
            .show(|ui| columns_panel(ui, view));

        let sort_text = format!(
            "{}  {}",
            egui_phosphor::regular::ARROWS_DOWN_UP,
            sort_label(&view.sort)
        );
        let _ = egui::containers::menu::MenuButton::new(
            egui::RichText::new(sort_text).size(12.0),
        )
        .ui(ui, |ui| {
            for value in [
                "position",
                "name",
                "rarity",
                "wear",
                "qty",
                "category",
                "tradehold",
                "price",
            ] {
                if ui
                    .selectable_value(&mut view.sort, value.to_owned(), sort_label(value))
                    .clicked()
                {
                    view.page = 0;
                }
            }
        });

        let prices_text = if prices_busy {
            "Updating prices…"
        } else {
            "Refresh prices"
        };
        let prices_button = egui::Button::new(
            egui::RichText::new(format!(
                "{}  {prices_text}",
                egui_phosphor::regular::ARROWS_CLOCKWISE
            ))
            .size(12.0),
        );
        if ui.add_enabled(!prices_busy, prices_button).clicked() {
            *refresh_prices = true;
        }
    });
    ui.add_space(6.0);
}

fn filter_panel(ui: &mut egui::Ui, view: &mut InventoryViewState, majors: &[String]) {
    ui.set_min_width(340.0);
    ui.set_max_width(460.0);
    ui.horizontal(|ui| {
        filter_label(ui, "FILTERS");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let clear = egui::Button::new(
                egui::RichText::new(format!(
                    "{}  Clear all",
                    egui_phosphor::regular::FUNNEL_SIMPLE_X
                ))
                .size(11.0),
            );
            if ui.add_enabled(has_active_filters(view), clear).clicked() {
                clear_filters(view);
            }
        });
    });
    ui.add_space(4.0);

    let units: Vec<(&str, bool)> = vec![("Storage units only", view.units_only)];
    chip_group(ui, view, "SHOW", units, |view, _| {
        view.units_only = !view.units_only;
    });

    let categories: Vec<(&str, bool)> = domain::CATEGORIES
        .iter()
        .map(|(_, name)| (*name, view.selected_categories.contains(*name)))
        .collect();
    chip_group(ui, view, "CATEGORY", categories, |view, text| {
        inventory::toggle_filter(&mut view.selected_categories, text.to_owned());
    });

    let rarities: Vec<(&str, bool)> = (1..=7u32)
        .map(|rarity| {
            let name = inventory::rarity_name(rarity);
            (name, view.selected_rarities.contains(name))
        })
        .collect();
    chip_group(ui, view, "RARITY", rarities, |view, text| {
        inventory::toggle_filter(&mut view.selected_rarities, text.to_owned());
    });

    if !majors.is_empty() {
        let list: Vec<(&str, bool)> = majors
            .iter()
            .map(|major| (major.as_str(), view.selected_majors.contains(major)))
            .collect();
        chip_group(ui, view, "MAJORS", list, |view, text| {
            inventory::toggle_filter(&mut view.selected_majors, text.to_owned());
        });
    }

    let properties: Vec<(&str, bool)> = ["Tradehold", "Nametag", "Moveable", "Equipped CT", "Equipped T"]
        .into_iter()
        .map(|property| (property, view.selected_properties.contains(property)))
        .collect();
    chip_group(ui, view, "PROPERTIES", properties, |view, text| {
        inventory::toggle_filter(&mut view.selected_properties, text.to_owned());
    });
}

fn chip_group(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    title: &str,
    items: Vec<(&str, bool)>,
    toggle: fn(&mut InventoryViewState, &str),
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.spacing_mut().item_spacing.y = 6.0;
        filter_label(ui, title);
        for (text, selected) in items {
            if chip(ui, selected, text) {
                toggle(view, text);
                view.page = 0;
            }
        }
    });
}

fn columns_panel(ui: &mut egui::Ui, view: &mut InventoryViewState) {
    ui.set_min_width(150.0);
    filter_label(ui, "COLUMNS");
    ui.add_space(2.0);
    let columns = &mut view.columns;
    ui.checkbox(&mut columns.rarity, "Rarity");
    ui.checkbox(&mut columns.wear, "Wear / Float");
    ui.checkbox(&mut columns.seed, "Seed");
    ui.checkbox(&mut columns.qty, "Qty");
    ui.checkbox(&mut columns.price, "Price");
    ui.checkbox(&mut columns.id, "Item ID");
    ui.checkbox(&mut columns.actions, "Actions");
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
                view.page = 0;
            }
        }
    });
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
    let stroke = if selected {
        theme::CHIP_ACTIVE
    } else {
        egui::Color32::from_rgba_unmultiplied(128, 128, 128, 115)
    };
    let frame = egui::Frame::NONE
        .corner_radius(99)
        .fill(if selected {
            theme::CHIP_ACTIVE
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(egui::Stroke::new(1.0_f32, stroke))
        .inner_margin(egui::Margin::symmetric(8, 1));
    let color = if selected {
        egui::Color32::WHITE
    } else {
        theme::TEXT
    };
    frame
        .show(ui, |ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(text).size(11.0).color(color))
                    .sense(egui::Sense::click()),
            )
            .clicked()
        })
        .inner
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
    view.page = 0;
}

/// The rows the table shows: the current filters applied to the
/// grouped inventory. Shared with the overview charts.
pub fn visible_rows<'a>(
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
            let button = egui::Button::new(
                egui::RichText::new(format!("{glyph}  {text}")).color(egui::Color32::WHITE),
            )
            .fill(theme::ACCENT);
            if ui.add(button).clicked() {
                clicked = true;
            }
        }
    });
    ui.add_space(24.0);
    clicked
}

pub(crate) fn error_box(ui: &mut egui::Ui, text: &str, note: Option<&str>) {
    let inner = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.set_max_width(640.0);
            ui.label(egui::RichText::new(text).size(13.0).color(theme::ERROR));
            if let Some(note) = note {
                ui.label(egui::RichText::new(note).size(12.0).color(theme::MUTED));
            }
        });
    let rect = inner.response.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.min.x + 3.0, rect.max.y)),
        0,
        egui::Color32::from_rgb(0xef, 0x44, 0x44),
    );
    ui.add_space(10.0);
}

// --- table -----------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cell {
    Item,
    Rarity,
    Wear,
    Seed,
    Qty,
    Price,
    Id,
    Actions,
}

struct ColumnDef {
    cell: Cell,
    label: &'static str,
    sort: Option<&'static str>,
    frac: f32,
}

const COLUMN_DEFS: [ColumnDef; 8] = [
    ColumnDef {
        cell: Cell::Item,
        label: "Item",
        sort: Some("name"),
        frac: 0.26,
    },
    ColumnDef {
        cell: Cell::Rarity,
        label: "Rarity",
        sort: Some("rarity"),
        frac: 0.10,
    },
    ColumnDef {
        cell: Cell::Wear,
        label: "Wear / Float",
        sort: Some("wear"),
        frac: 0.13,
    },
    ColumnDef {
        cell: Cell::Seed,
        label: "Seed",
        sort: None,
        frac: 0.06,
    },
    ColumnDef {
        cell: Cell::Qty,
        label: "Qty",
        sort: Some("qty"),
        frac: 0.06,
    },
    ColumnDef {
        cell: Cell::Price,
        label: "Price",
        sort: Some("price"),
        frac: 0.10,
    },
    ColumnDef {
        cell: Cell::Id,
        label: "Item ID",
        sort: None,
        frac: 0.16,
    },
    ColumnDef {
        cell: Cell::Actions,
        label: "Actions",
        sort: None,
        frac: 0.13,
    },
];

fn column_visible(def: &ColumnDef, columns: &Columns, show_actions: bool) -> bool {
    match def.cell {
        Cell::Item => true,
        Cell::Rarity => columns.rarity,
        Cell::Wear => columns.wear,
        Cell::Seed => columns.seed,
        Cell::Qty => columns.qty,
        Cell::Price => columns.price,
        Cell::Id => columns.id,
        Cell::Actions => columns.actions && show_actions,
    }
}

fn table(
    ui: &mut egui::Ui,
    images: &mut ImageCache,
    view: &mut InventoryViewState,
    rows: &[&GroupedItem],
    open_storage: &mut Option<(String, String)>,
    clipboard: &mut Option<String>,
    show_actions: bool,
) {
    let columns = view.columns;
    let defs: Vec<&ColumnDef> = COLUMN_DEFS
        .iter()
        .filter(|def| column_visible(def, &columns, show_actions))
        .collect();
    if defs.is_empty() {
        return;
    }

    let width = ui.available_width();
    let total_frac: f32 = defs.iter().map(|def| def.frac).sum();
    // Leave room for the footer row (spacing + page controls) below the table.
    let table_height = (ui.available_height() - 40.0).max(200.0);

    let mut builder = egui_extras::TableBuilder::new(ui)
        .striped(true)
        .sense(egui::Sense::click())
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .id_salt("items-table")
        .max_scroll_height(table_height);
    let last = defs.len() - 1;
    for (index, def) in defs.iter().enumerate() {
        let column = if index == last {
            egui_extras::Column::remainder().at_least(88.0)
        } else {
            egui_extras::Column::initial(width * def.frac / total_frac).resizable(true)
        };
        builder = builder.column(column);
    }

    builder
        .header(theme::HEADER_HEIGHT, |mut header| {
            for def in &defs {
                let active = def.sort == Some(view.sort.as_str());
                let (_, response) = header.col(|ui| {
                    let mut text = egui::RichText::new(def.label)
                        .size(11.0)
                        .color(if active { theme::TEXT } else { theme::MUTED });
                    if active {
                        text = text.strong();
                    }
                    ui.label(text);
                    if active {
                        let caret = if view.sort_desc {
                            egui_phosphor::regular::CARET_DOWN
                        } else {
                            egui_phosphor::regular::CARET_UP
                        };
                        ui.label(
                            egui::RichText::new(caret)
                                .size(10.0)
                                .color(theme::TEXT),
                        );
                    }
                });
                if let Some(mode) = def.sort {
                    if response.hovered() {
                        response
                            .ctx
                            .set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                    if response.clicked() {
                        if view.sort == mode {
                            view.sort_desc = !view.sort_desc;
                        } else {
                            view.sort = mode.to_owned();
                            view.sort_desc = false;
                        }
                        view.page = 0;
                    }
                }
            }
        })
        .body(|body| {
            body.rows(theme::ROW_HEIGHT, rows.len(), |mut row| {
                let grouped = rows[row.index()];
                let item = &grouped.representative;
                let display_qty = grouped.combined_qty;

                let (_, identity) = row.col(|ui| identity_cell(ui, images, item));
                identity.on_hover_text(format!("Item ID: {}", item.id));
                if columns.rarity {
                    row.col(|ui| {
                        ui.label(
                            egui::RichText::new(inventory::rarity_name(item.rarity))
                                .size(11.0)
                                .color(theme::rarity_color(item.rarity)),
                        );
                    });
                }
                if columns.wear {
                    row.col(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(
                                    item.wear_name.clone().unwrap_or_else(|| "-".into()),
                                )
                                .size(11.0),
                            );
                            if let Some(wear) = item.wear {
                                ui.label(
                                    egui::RichText::new(format!("{wear:.6}"))
                                        .size(10.0)
                                        .color(theme::MUTED),
                                );
                            }
                        });
                    });
                }
                if columns.seed {
                    row.col(|ui| {
                        ui.label(
                            egui::RichText::new(
                                item.paint_seed
                                    .map(|seed| seed.to_string())
                                    .unwrap_or_else(|| "-".into()),
                            )
                            .size(11.0),
                        );
                    });
                }
                if columns.qty {
                    row.col(|ui| {
                        ui.label(egui::RichText::new(display_qty.to_string()).size(11.0));
                    });
                }
                if columns.price {
                    row.col(|ui| match item.price {
                        Some(price) => {
                            // Legacy `priceRow`: combined price (price x qty)
                            // on top, the per-unit price beneath it — shown
                            // only for stacks with a non-zero price.
                            ui.vertical(|ui| {
                                ui.label(
                                    egui::RichText::new(pricing::format_usd(
                                        price * display_qty as f64,
                                    ))
                                    .size(11.0),
                                );
                                if display_qty > 1 && price != 0.0 {
                                    ui.label(
                                        egui::RichText::new(pricing::format_usd(price))
                                            .size(10.0)
                                            .color(theme::MUTED),
                                    );
                                }
                            });
                        }
                        None => {
                            ui.label(
                                egui::RichText::new("-").size(11.0).color(theme::MUTED),
                            );
                        }
                    });
                }
                if columns.id {
                    row.col(|ui| {
                        ui.label(
                            egui::RichText::new(&item.id)
                                .monospace()
                                .size(10.0)
                                .color(theme::MUTED),
                        );
                    });
                }
                if columns.actions && show_actions {
                    row.col(|ui| {
                        if item.is_storage_unit {
                            let contents = egui::Button::new(format!(
                                "{}  Contents",
                                egui_phosphor::regular::FOLDER_OPEN
                            ))
                            .fill(theme::RAISED);
                            if ui.add(contents).clicked() {
                                *open_storage = Some((item.id.clone(), item.name.clone()));
                            }
                        }
                    });
                }

                let response = row.response();
                let _ = response.context_menu(|ui| {
                    ui.set_min_width(170.0);
                    if ui
                        .button(format!(
                            "{}  Copy name",
                            egui_phosphor::regular::COPY
                        ))
                        .clicked()
                    {
                        *clipboard = Some(item.name.clone());
                        ui.close();
                    }
                    if ui
                        .button(format!(
                            "{}  Copy item ID",
                            egui_phosphor::regular::COPY_SIMPLE
                        ))
                        .clicked()
                    {
                        *clipboard = Some(item.id.clone());
                        ui.close();
                    }
                    if item.is_storage_unit
                        && ui
                            .button(format!(
                                "{}  Open contents",
                                egui_phosphor::regular::FOLDER_OPEN
                            ))
                            .clicked()
                    {
                        *open_storage = Some((item.id.clone(), item.name.clone()));
                        ui.close();
                    }
                });
            });
        });
}

fn identity_cell(ui: &mut egui::Ui, images: &mut ImageCache, item: &InventoryItem) {
    let url = item.image_url.as_deref().filter(|url| !url.is_empty());
    ui.horizontal(|ui| {
        images::media_box(
            ui,
            images,
            url,
            egui::vec2(44.0, 36.0),
            egui::CornerRadius::same(3),
            egui_phosphor::regular::PACKAGE,
            22.0,
            egui::Color32::from_rgb(0x6b, 0x72, 0x80),
        );
        ui.add_space(8.0);
        ui.vertical(|ui| {
            ui.add(
                egui::Label::new(egui::RichText::new(&item.name).strong().size(12.0)).truncate(),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if let Some(custom_name) = &item.custom_name {
                    ui.label(
                        egui::RichText::new(format!("\"{custom_name}\""))
                            .size(10.0)
                            .color(theme::MUTED),
                    );
                }
                if item.stat_trak {
                    ui.label(
                        egui::RichText::new("StatTrak")
                            .size(10.0)
                            .color(egui::Color32::from_rgb(0xd6, 0xc1, 0x8a)),
                    );
                }
                if item.is_storage_unit {
                    ui.label(
                        egui::RichText::new("Storage unit")
                            .size(10.0)
                            .color(egui::Color32::from_rgb(0xd6, 0xc1, 0x8a)),
                    );
                }
                if let Some(days) = tradehold_days(item) {
                    ui.label(
                        egui::RichText::new(format!("{days} days tradehold"))
                            .size(10.0)
                            .color(theme::GREEN),
                    );
                }
            });
            if !item.stickers.is_empty() {
                let names = item
                    .stickers
                    .iter()
                    .map(|sticker| format!("{} · {}", sticker.name, sticker.kind))
                    .collect::<Vec<_>>()
                    .join(", ");
                ui.label(
                    egui::RichText::new(names)
                        .size(10.0)
                        .color(theme::MUTED),
                );
            }
            for (label, value) in [
                ("Music kit", item.music_kit.as_deref()),
                ("Graffiti", item.graffiti.as_deref()),
            ] {
                if let Some(value) = value {
                    ui.label(
                        egui::RichText::new(format!("{label}: {value}"))
                            .size(10.0)
                            .color(theme::MUTED),
                    );
                }
            }
        });
    });
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

fn footer(
    ui: &mut egui::Ui,
    view: &mut InventoryViewState,
    total: usize,
    start: usize,
    end: usize,
    value_label: String,
) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.label(egui::RichText::new(value_label).size(12.0).strong());
        ui.label(
            egui::RichText::new(format!("{start}-{end} of {total}"))
                .size(theme::MUTED_SIZE)
                .color(theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let pages = total.div_ceil(PAGE_SIZE).max(1);
            let page = view.page.min(pages - 1);
            ui.label(
                egui::RichText::new(format!("Page {} / {pages}", page + 1))
                    .size(theme::MUTED_SIZE)
                    .color(theme::MUTED),
            );
            let next = egui::Button::new(egui::RichText::new(egui_phosphor::regular::CARET_RIGHT));
            if ui.add_enabled(page + 1 < pages, next).clicked() {
                view.page = page + 1;
            }
            let previous =
                egui::Button::new(egui::RichText::new(egui_phosphor::regular::CARET_LEFT));
            if ui.add_enabled(page > 0, previous).clicked() {
                view.page = page.saturating_sub(1);
            }
        });
    });
}

fn sort_label(value: &str) -> &'static str {
    match value {
        "name" => "Name",
        "rarity" => "Rarity",
        "wear" => "Float",
        "qty" => "Qty",
        "category" => "Category",
        "tradehold" => "Tradehold",
        "price" => "Price",
        _ => "Position",
    }
}
