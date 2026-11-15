//! Storage-unit management: every unit with its name, capacity, item count,
//! and value, plus a shortcut into its contents. Values come from the same
//! "Load storage units" pass the overview uses.

use crate::app::CasemoveApp;
use crate::images::{self, ImageCache};
use crate::inventory::STORAGE_CAPACITY;
use crate::theme;
use eframe::egui;

struct UnitRow {
    id: String,
    name: String,
    image_url: Option<String>,
    count: usize,
    loaded: bool,
    value: f64,
}

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut load = false;
    let mut open: Option<(String, String)> = None;

    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: theme::PAGE_MARGIN,
            right: theme::PAGE_MARGIN,
            top: 14,
            bottom: 10,
        })
        .show(ui, |ui| {
            let rows = unit_rows(app);
            header(ui, app, &rows, &mut load);
            ui.add_space(8.0);

            if !app.storage_aggregate_error.is_empty() {
                crate::inventory_view::error_box(ui, &app.storage_aggregate_error, None);
            }
            if app.inventory.is_none() {
                crate::inventory_view::empty_state(
                    ui,
                    "Inventory not loaded",
                    Some(("Load inventory", egui_phosphor::regular::ARROW_CLOCKWISE)),
                );
                return;
            }
            if rows.is_empty() {
                crate::inventory_view::empty_state(ui, "No storage units", None);
                return;
            }

            search(ui, app);
            ui.add_space(6.0);
            table(ui, app, &rows, &mut open);
        });

    if load {
        app.load_all_storage();
    }
    if let Some((id, name)) = open {
        app.open_storage_contents(id, name);
        if app.storage_selection.is_some() {
            app.inventory_open = true;
            app.overview_open = false;
            app.storage_open = false;
            app.save_view("inventory");
        }
    }
}

fn unit_rows(app: &CasemoveApp) -> Vec<UnitRow> {
    let Some(snapshot) = &app.inventory else {
        return Vec::new();
    };
    let mut rows: Vec<UnitRow> = snapshot
        .items
        .iter()
        .filter(|item| item.is_storage_unit)
        .map(|item| {
            let contents = app.storage_unit_contents.get(&item.id);
            let loaded = contents.is_some();
            let value = contents
                .map(|items| {
                    items
                        .iter()
                        .map(|item| item.price.unwrap_or(0.0) * item.quantity as f64)
                        .sum()
                })
                .unwrap_or(0.0);
            UnitRow {
                id: item.id.clone(),
                name: item
                    .custom_name
                    .clone()
                    .filter(|name| !name.is_empty())
                    .unwrap_or_else(|| item.name.clone()),
                image_url: item.image_url.clone(),
                count: contents
                    .map(|items| items.len())
                    .unwrap_or_else(|| item.storage_item_count.unwrap_or(0) as usize),
                loaded,
                value,
            }
        })
        .collect();
    rows.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    rows
}

fn header(ui: &mut egui::Ui, app: &CasemoveApp, rows: &[UnitRow], load: &mut bool) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(egui::RichText::new("Storage units").size(theme::TITLE_SIZE));
            let items: usize = rows.iter().map(|row| row.count).sum();
            let all_loaded = !rows.is_empty() && rows.iter().all(|row| row.loaded);
            let value: f64 = rows.iter().map(|row| row.value).sum();
            let mut summary = format!(
                "{} {} · {} {}",
                rows.len(),
                if rows.len() == 1 { "unit" } else { "units" },
                items,
                if items == 1 { "item" } else { "items" }
            );
            if all_loaded {
                summary.push_str(&format!(" · {}", pricing::format_usd(value)));
            }
            ui.label(
                egui::RichText::new(summary)
                    .size(theme::MUTED_SIZE)
                    .color(theme::MUTED),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let label = if app.storage_aggregate_busy {
                "Loading storage...".to_owned()
            } else if app.storage_aggregate.is_some() {
                format!("{}  Refresh values", egui_phosphor::regular::ARROW_CLOCKWISE)
            } else {
                format!("{}  Load storage units", egui_phosphor::regular::CLOUD_ARROW_DOWN)
            };
            if ui
                .add_enabled(
                    !app.storage_aggregate_busy
                        && !app.session_busy
                        && !app.inventory_busy
                        && app.inventory.is_some(),
                    egui::Button::new(label),
                )
                .clicked()
            {
                *load = true;
            }
        });
    });
}

fn search(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    ui.add(
        egui::TextEdit::singleline(&mut app.storage_query)
            .hint_text("Search storage units")
            .desired_width(260.0),
    );
}

fn table(
    ui: &mut egui::Ui,
    app: &mut CasemoveApp,
    rows: &[UnitRow],
    open: &mut Option<(String, String)>,
) {
    let query = app.storage_query.to_lowercase();
    let visible: Vec<&UnitRow> = rows
        .iter()
        .filter(|row| {
            query.is_empty()
                || row.name.to_lowercase().contains(&query)
                || row.id.contains(&query)
        })
        .collect();

    let width = ui.available_width();
    let height = ui.available_height().max(160.0);
    let builder = egui_extras::TableBuilder::new(ui)
        .striped(true)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .id_salt("storage-units-table")
        .max_scroll_height(height)
        .column(egui_extras::Column::remainder().at_least(200.0))
        .column(egui_extras::Column::initial(width * 0.26).at_least(140.0))
        .column(egui_extras::Column::initial(width * 0.16).at_least(100.0))
        .column(egui_extras::Column::initial(96.0));

    builder
        .header(theme::HEADER_HEIGHT, |mut header| {
            for label in ["Unit", "Items", "Value", ""] {
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new(label)
                            .size(11.0)
                            .color(theme::MUTED),
                    );
                });
            }
        })
        .body(|body| {
            body.rows(theme::ROW_HEIGHT, visible.len(), |mut row| {
                let unit = visible[row.index()];
                row.col(|ui| identity(ui, &mut app.images, unit));
                row.col(|ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "{} / {}",
                            unit.count, STORAGE_CAPACITY
                        ))
                        .size(theme::BODY_SIZE),
                    );
                });
                row.col(|ui| {
                    let text = if unit.loaded {
                        pricing::format_usd(unit.value)
                    } else {
                        "-".into()
                    };
                    ui.label(
                        egui::RichText::new(text)
                            .size(theme::BODY_SIZE)
                            .color(if unit.loaded { theme::GREEN } else { theme::MUTED }),
                    );
                });
                row.col(|ui| {
                    if ui.button("Open").clicked() {
                        *open = Some((unit.id.clone(), unit.name.clone()));
                    }
                });
            });
        });
}

fn identity(ui: &mut egui::Ui, images: &mut ImageCache, unit: &UnitRow) {
    ui.horizontal(|ui| {
        images::media_box(
            ui,
            images,
            unit.image_url.as_deref().filter(|url| !url.is_empty()),
            egui::vec2(30.0, 30.0),
            egui::CornerRadius::same(4),
            egui_phosphor::regular::VAULT,
            16.0,
            theme::ICON,
        );
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&unit.name).size(theme::BODY_SIZE));
            ui.label(
                egui::RichText::new(&unit.id)
                    .monospace()
                    .size(10.0)
                    .color(theme::MUTED),
            );
        });
    });
}
