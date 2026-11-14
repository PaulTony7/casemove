use crate::app::CasemoveApp;
use crate::images::{self, ImageCache};
use crate::inventory::{self, GroupedItem};
use crate::theme;
use app_commands::InventoryItem;
use eframe::egui;
use std::collections::HashSet;

const PAGE_SIZE: usize = inventory::PAGE_SIZE;
const INVENTORY_COLUMNS: [(&str, &str); 11] = [
    ("item", "Item"),
    ("rarity", "Rarity"),
    ("wear", "Wear / Float"),
    ("seed", "Seed"),
    ("qty", "Qty"),
    ("price", "Price"),
    ("collection", "Collection"),
    ("stickers", "Stickers / Patches"),
    ("trade_up", "Trade-up"),
    ("id", "Item ID"),
    ("actions", "Actions"),
];

pub struct InventoryViewState {
    pub query: String,
    pub sort: String,
    pub units_only: bool,
    pub page: usize,
    pub selected_categories: HashSet<String>,
    pub selected_rarities: HashSet<String>,
    pub selected_majors: HashSet<String>,
    pub selected_properties: HashSet<String>,
    pub visible_columns: HashSet<String>,
}

impl Default for InventoryViewState {
    fn default() -> Self {
        Self {
            query: String::new(),
            sort: "position".to_owned(),
            units_only: false,
            page: 0,
            selected_categories: HashSet::new(),
            selected_rarities: HashSet::new(),
            selected_majors: HashSet::new(),
            selected_properties: HashSet::new(),
            visible_columns: [
                "item", "rarity", "wear", "seed", "qty", "price", "id", "actions",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        }
    }
}

impl InventoryViewState {
    pub fn set_columns(&mut self, columns: Vec<String>) {
        let valid: HashSet<&str> = INVENTORY_COLUMNS.iter().map(|(key, _)| *key).collect();
        self.visible_columns = columns
            .iter()
            .map(String::as_str)
            .filter(|column| valid.contains(column))
            .map(str::to_owned)
            .collect();
        self.visible_columns.insert("item".to_owned());
    }

    pub fn columns(&self) -> Vec<String> {
        INVENTORY_COLUMNS
            .iter()
            .filter(|(key, _)| self.visible_columns.contains(*key))
            .map(|(key, _)| (*key).to_owned())
            .collect()
    }
}

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut back = false;
    let mut refresh = false;
    let mut sign_out = false;
    let mut open_storage: Option<(String, String)> = None;

    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: 28,
            right: 28,
            top: 32,
            bottom: 16,
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
            ui.add_space(16.0);

            if app.storage_selection.is_some() {
                storage_section(ui, app);
            } else {
                main_section(ui, app, &mut open_storage);
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
            ui.label(egui::RichText::new(title).size(26.0).strong());
            let status = if app.storage_selection.is_some() {
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
            };
            ui.label(egui::RichText::new(status).size(12.0).color(theme::MUTED));
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let sign_out_button =
                egui::Button::new(egui::RichText::new(egui_phosphor::regular::SIGN_OUT));
            if ui.add(sign_out_button).on_hover_text("Sign out").clicked() {
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

fn storage_section(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let name = app
        .storage_selection
        .as_ref()
        .map(|(_, name)| name.clone())
        .unwrap_or_default();
    ui.add_space(4.0);
    ui.label(egui::RichText::new(name).size(13.0).color(theme::MUTED));
    ui.add_space(8.0);

    if !app.storage_error.is_empty() {
        error_box(ui, &app.storage_error, None);
    }

    if app.storage_contents.as_ref().is_some_and(Vec::is_empty) {
        empty_state(ui, "Storage unit is empty");
    }

    if let Some(items) = &app.storage_contents
        && !items.is_empty()
    {
        // Legacy storage totals: combined_QTY x price over the contents
        // (grouping does not change the sum; unpriced rows count as zero).
        let contents_value: f64 = items
            .iter()
            .map(|item| item.price.unwrap_or(0.0) * item.quantity as f64)
            .sum();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "Contents value: {}",
                    pricing::format_usd(contents_value)
                ))
                .size(13.0)
                .strong(),
            );
        });
        ui.add_space(6.0);
        let columns = app.view.columns();
        table(
            ui,
            &mut app.images,
            TableRows::Storage(items),
            &columns,
            &mut None::<(String, String)>,
        );
    }
}

fn main_section(
    ui: &mut egui::Ui,
    app: &mut CasemoveApp,
    open_storage: &mut Option<(String, String)>,
) {
    if app.inventory.is_none() {
        ui.horizontal_top(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::INFO)
                    .size(16.0)
                    .color(theme::STATUS_TEXT),
            );
            ui.label(
                egui::RichText::new(
                    "Close any running game before connecting. Steam will mark this account as playing CS2.",
                )
                .size(12.0)
                .color(theme::STATUS_TEXT),
            );
        });
        ui.add_space(14.0);
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
            empty_state(ui, "Inventory not loaded");
        }
        return;
    }

    toolbar(ui, app);
    filter_chips(ui, app);

    let mut rows = visible_rows(&app.grouped, &app.view);
    inventory::sort_rows(&mut rows, &app.view.sort);

    let inventory_value: f64 = rows
        .iter()
        .map(|row| row.representative.price.unwrap_or(0.0) * row.combined_qty as f64)
        .sum();
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "Inventory value: {}",
                pricing::format_usd(inventory_value)
            ))
            .size(13.0)
            .strong(),
        );
    });
    ui.add_space(6.0);

    if rows.is_empty() {
        let empty_inventory = app
            .inventory
            .as_ref()
            .is_some_and(|snapshot| snapshot.items.is_empty());
        empty_state(
            ui,
            if empty_inventory {
                "Inventory empty"
            } else {
                "No matching items"
            },
        );
        return;
    }

    let total = rows.len();
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    let effective_page = app.view.page.min(pages - 1);
    let start = effective_page * PAGE_SIZE;
    let end = (start + PAGE_SIZE).min(total);

    let columns = app.view.columns();
    table(
        ui,
        &mut app.images,
        TableRows::Grouped(&rows[start..end]),
        &columns,
        open_storage,
    );

    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{}-{} of {}", start + 1, end, total))
                .size(12.0)
                .color(theme::MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let next = egui::Button::new(egui::RichText::new(egui_phosphor::regular::CARET_RIGHT));
            if ui.add_enabled(app.view.page + 1 < pages, next).clicked() {
                app.view.page += 1;
            }
            let previous =
                egui::Button::new(egui::RichText::new(egui_phosphor::regular::CARET_LEFT));
            if ui.add_enabled(app.view.page > 0, previous).clicked() {
                app.view.page = app.view.page.saturating_sub(1);
            }
        });
    });
}

fn toolbar(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut columns_changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS).color(theme::MUTED),
            );
            let search = ui.add(
                egui::TextEdit::singleline(&mut app.view.query)
                    .hint_text("Search inventory")
                    .desired_width(320.0),
            );
            if search.changed() {
                app.view.page = 0;
            }
        });
        let units = ui.checkbox(&mut app.view.units_only, "Storage units only");
        if units.changed() {
            app.view.page = 0;
        }
        let before = app.view.sort.clone();
        egui::ComboBox::from_id_salt("inventory-sort")
            .selected_text(sort_label(&before))
            .show_ui(ui, |ui| {
                for value in [
                    "position",
                    "name",
                    "rarity",
                    "wear",
                    "qty",
                    "category",
                    "tradehold",
                    "price",
                    "collection",
                    "stickers",
                    "storage",
                ] {
                    ui.selectable_value(&mut app.view.sort, value.to_owned(), sort_label(value));
                }
            });
        if app.view.sort != before {
            app.view.page = 0;
        }
        egui::ComboBox::from_id_salt("inventory-columns")
            .selected_text("Columns")
            .show_ui(ui, |ui| {
                for (key, label) in INVENTORY_COLUMNS {
                    if key == "item" {
                        let mut visible = true;
                        ui.add_enabled(false, egui::Checkbox::new(&mut visible, label));
                        continue;
                    }
                    let mut visible = app.view.visible_columns.contains(key);
                    if ui.checkbox(&mut visible, label).changed() {
                        if visible {
                            app.view.visible_columns.insert(key.to_owned());
                        } else {
                            app.view.visible_columns.remove(key);
                        }
                        columns_changed = true;
                    }
                }
            });
        let prices_text = if app.prices_busy {
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
        if ui.add_enabled(!app.prices_busy, prices_button).clicked() {
            app.refresh_live_prices(true);
        }
    });
    if columns_changed {
        let columns = app.view.columns();
        let core = app.core.clone();
        drop(app.rt.spawn_blocking(move || {
            if let Err(error) = core.store_set("inventory.columns", columns) {
                eprintln!("casemove settings: {error}");
            }
        }));
    }
    ui.add_space(14.0);
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
        "collection" => "Collection",
        "stickers" => "Sticker count",
        "storage" => "Storage name",
        _ => "Position",
    }
}

fn filter_chips(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.spacing_mut().item_spacing.y = 8.0;
        filter_label(ui, "CATEGORY");
        for (_, category) in domain::CATEGORIES {
            if chip(
                ui,
                app.view.selected_categories.contains(category),
                category,
            ) {
                inventory::toggle_filter(&mut app.view.selected_categories, category.to_owned());
            }
        }
        ui.add_space(6.0);
        filter_label(ui, "RARITY");
        for rarity in 1..=7u32 {
            let name = inventory::rarity_name(rarity);
            if chip(ui, app.view.selected_rarities.contains(name), name) {
                inventory::toggle_filter(&mut app.view.selected_rarities, name.to_owned());
            }
        }
        if !app.majors.is_empty() {
            ui.add_space(6.0);
            filter_label(ui, "MAJORS");
            for major in &app.majors {
                if chip(ui, app.view.selected_majors.contains(major), major) {
                    inventory::toggle_filter(&mut app.view.selected_majors, major.clone());
                }
            }
        }
        ui.add_space(6.0);
        filter_label(ui, "PROPERTIES");
        for property in [
            "Tradehold",
            "Nametag",
            "Moveable",
            "Equipped CT",
            "Equipped T",
            "Stickers",
            "Patches",
        ] {
            if chip(
                ui,
                app.view.selected_properties.contains(property),
                property,
            ) {
                inventory::toggle_filter(&mut app.view.selected_properties, property.to_owned());
            }
        }
    });
    ui.add_space(6.0);
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
        .inner_margin(egui::Margin::symmetric(10, 2));
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

/// The rows the inventory table shows: the current filters applied to the
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

pub(crate) fn empty_state(ui: &mut egui::Ui, title: &str) {
    ui.add_space(60.0);
    ui.vertical_centered(|ui| {
        ui.label(
            egui::RichText::new(egui_phosphor::regular::ARCHIVE)
                .size(32.0)
                .color(theme::MUTED),
        );
        ui.add_space(16.0);
        ui.label(egui::RichText::new(title).size(16.0).color(theme::MUTED));
    });
    ui.add_space(60.0);
}

pub(crate) fn error_box(ui: &mut egui::Ui, text: &str, note: Option<&str>) {
    let inner = egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(12, 8))
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
    ui.add_space(14.0);
}

// --- table -----------------------------------------------------------------

enum TableRows<'a> {
    Grouped(&'a [&'a GroupedItem]),
    Storage(&'a [InventoryItem]),
}

fn table(
    ui: &mut egui::Ui,
    images: &mut ImageCache,
    rows: TableRows<'_>,
    columns: &[String],
    open_storage: &mut Option<(String, String)>,
) {
    let width = ui.available_width();
    let table_height = (ui.available_height() - 44.0).max(240.0);
    let mut storage_action: Option<(String, String)> = None;
    let weights: Vec<f32> = columns.iter().map(|column| column_weight(column)).collect();
    let weight_sum: f32 = weights.iter().sum();
    let mut builder = egui_extras::TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center));
    for (weight, _) in weights.iter().zip(columns) {
        builder = builder
            .column(egui_extras::Column::initial(width * weight / weight_sum).resizable(true));
    }
    builder
        .max_scroll_height(table_height)
        .header(30.0, |mut header| {
            for column in columns {
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new(column_label(column))
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                });
            }
        })
        .body(|body| {
            let count = match &rows {
                TableRows::Grouped(items) => items.len(),
                TableRows::Storage(items) => items.len(),
            };
            body.rows(76.0, count, |mut row| {
                let index = row.index();
                let (item, display_qty) = match &rows {
                    TableRows::Grouped(items) => {
                        (&items[index].representative, items[index].combined_qty)
                    }
                    TableRows::Storage(items) => (&items[index], items[index].quantity as usize),
                };
                for column in columns {
                    row.col(|ui| match column.as_str() {
                        "item" => identity_cell(ui, images, item),
                        "rarity" => {
                            ui.label(
                                egui::RichText::new(inventory::rarity_name(item.rarity))
                                    .size(12.0)
                                    .color(theme::rarity_color(item.rarity)),
                            );
                        }
                        "wear" => wear_cell(ui, item),
                        "seed" => {
                            ui.label(
                                egui::RichText::new(
                                    item.paint_seed
                                        .map(|seed| seed.to_string())
                                        .unwrap_or_else(|| "-".into()),
                                )
                                .size(12.0),
                            );
                        }
                        "qty" => {
                            ui.label(egui::RichText::new(display_qty.to_string()).size(12.0));
                        }
                        "price" => price_cell(ui, item, display_qty),
                        "collection" => {
                            ui.label(
                                item.collection
                                    .as_deref()
                                    .map(|name| egui::RichText::new(name).size(11.0))
                                    .unwrap_or_else(|| {
                                        egui::RichText::new("-").color(theme::MUTED)
                                    }),
                            );
                        }
                        "stickers" => sticker_cell(ui, images, item),
                        "trade_up" => {
                            ui.label(if item.trade_up_eligible {
                                "Eligible"
                            } else {
                                "-"
                            });
                        }
                        "id" => {
                            ui.label(
                                egui::RichText::new(&item.id)
                                    .monospace()
                                    .size(11.0)
                                    .color(theme::MUTED),
                            );
                        }
                        "actions" if item.is_storage_unit => {
                            let contents = egui::Button::new(format!(
                                "{}  Contents",
                                egui_phosphor::regular::FOLDER_OPEN
                            ))
                            .fill(theme::RAISED);
                            if ui.add(contents).clicked() {
                                storage_action = Some((item.id.clone(), item.name.clone()));
                            }
                        }
                        _ => {}
                    });
                }
            });
        });

    if let Some(action) = storage_action
        && let Some(target) = open_storage
    {
        *target = action;
    }
}

fn column_label(key: &str) -> &'static str {
    INVENTORY_COLUMNS
        .iter()
        .find(|(column, _)| *column == key)
        .map(|(_, label)| *label)
        .unwrap_or("")
}

fn column_weight(key: &str) -> f32 {
    match key {
        "item" => 28.0,
        "wear" => 15.0,
        "collection" | "stickers" => 16.0,
        "id" => 18.0,
        "actions" => 12.0,
        "rarity" => 10.0,
        _ => 8.0,
    }
}

fn wear_cell(ui: &mut egui::Ui, item: &InventoryItem) {
    ui.vertical(|ui| {
        ui.label(
            egui::RichText::new(item.wear_name.clone().unwrap_or_else(|| "-".into())).size(12.0),
        );
        if let Some(wear) = item.wear {
            ui.label(
                egui::RichText::new(format!("{wear:.6}"))
                    .size(11.0)
                    .color(theme::MUTED),
            );
        }
        if let Some(until) = item.trade_unlock {
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_millis() as i64)
                .unwrap_or(0);
            let delta = until - now_ms;
            let days = delta.div_euclid(86_400_000) + i64::from(delta.rem_euclid(86_400_000) > 0);
            if days > 0 {
                ui.label(
                    egui::RichText::new(format!("{days} days tradehold"))
                        .size(11.0)
                        .color(theme::GREEN),
                );
            }
        }
    });
}

fn price_cell(ui: &mut egui::Ui, item: &InventoryItem, display_qty: usize) {
    match item.price {
        Some(price) => {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(pricing::format_usd(price * display_qty as f64)).size(12.0),
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
            ui.label(egui::RichText::new("-").size(11.0).color(theme::MUTED));
        }
    }
}

fn sticker_cell(ui: &mut egui::Ui, images: &mut ImageCache, item: &InventoryItem) {
    if item.stickers.is_empty() {
        ui.label(egui::RichText::new("-").size(11.0).color(theme::MUTED));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for sticker in &item.stickers {
            ui.horizontal(|ui| {
                images::media_box(
                    ui,
                    images,
                    sticker.image_url.as_deref(),
                    egui::vec2(24.0, 24.0),
                    egui::CornerRadius::ZERO,
                    egui_phosphor::regular::PACKAGE,
                    14.0,
                    egui::Color32::from_rgb(0x6b, 0x72, 0x80),
                );
                let response = ui.label(
                    egui::RichText::new(format!("{} · {}", sticker.name, sticker.kind)).size(11.0),
                );
                let mut details = vec![format!("Slot: {}", sticker.slot)];
                for (label, value) in [
                    ("Wear", sticker.wear),
                    ("Scale", sticker.scale),
                    ("Rotation", sticker.rotation),
                    ("X offset", sticker.offset_x),
                    ("Y offset", sticker.offset_y),
                ] {
                    if let Some(value) = value {
                        details.push(format!("{label}: {value:.4}"));
                    }
                }
                response.on_hover_text(details.join("\n"));
            });
        }
    });
}

fn identity_cell(ui: &mut egui::Ui, images: &mut ImageCache, item: &InventoryItem) {
    ui.horizontal(|ui| {
        images::media_box(
            ui,
            images,
            item.image_url.as_deref(),
            egui::vec2(56.0, 48.0),
            egui::CornerRadius::ZERO,
            egui_phosphor::regular::PACKAGE,
            26.0,
            egui::Color32::from_rgb(0x6b, 0x72, 0x80),
        );
        ui.add_space(10.0);
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&item.name).strong().size(12.0));
            if let Some(custom_name) = &item.custom_name {
                ui.label(
                    egui::RichText::new(format!("\"{custom_name}\""))
                        .size(11.0)
                        .color(theme::MUTED),
                );
            }
            let mut tags: Vec<&str> = Vec::new();
            if item.stat_trak {
                tags.push("StatTrak");
            }
            if item.is_storage_unit {
                tags.push("Storage unit");
            }
            if !tags.is_empty() {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for tag in tags {
                        ui.label(
                            egui::RichText::new(tag)
                                .size(10.0)
                                .color(egui::Color32::from_rgb(0xd6, 0xc1, 0x8a)),
                        );
                    }
                });
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configurable_columns_keep_item_visible_and_discard_unknown_values() {
        let mut view = InventoryViewState::default();
        assert_eq!(
            view.columns(),
            [
                "item", "rarity", "wear", "seed", "qty", "price", "id", "actions"
            ]
        );
        view.set_columns(vec!["stickers".into(), "unknown".into()]);
        assert_eq!(view.columns(), ["item", "stickers"]);
    }
}
