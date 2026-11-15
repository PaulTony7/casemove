//! Overview page: value totals, the Overall bar chart, and the category
//! pie — legacy's `/stats` view (`runOverview` + `leftGraph/barChartOverall`
//! + `categoryDistribution`).

use crate::app::{profile_display_name, CasemoveApp};
use crate::inventory::{self, GroupedItem};
use crate::inventory_view;
use crate::theme;
use eframe::egui;
use std::collections::HashMap;

/// Legacy `settings.overview.by`: what both charts aggregate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverviewBy {
    #[default]
    Price,
    Volume,
}

impl OverviewBy {
    pub fn label(self) -> &'static str {
        match self {
            Self::Price => "Price",
            Self::Volume => "Volume",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Price => "price",
            Self::Volume => "volume",
        }
    }

    pub fn from_store(value: &str) -> Option<Self> {
        match value {
            "price" => Some(Self::Price),
            "volume" => Some(Self::Volume),
            _ => None,
        }
    }
}

/// Legacy chart.js dataset colors (0.2 alpha fills over the dark background).
const INVENTORY_COLOR: [u8; 3] = [255, 99, 132];
const STORAGE_COLOR: [u8; 3] = [50, 91, 136];

fn category_rgb(value: &str) -> Option<[u8; 3]> {
    match value {
        "characters" => Some([255, 99, 132]),
        "status_icons" => Some([153, 102, 255]),
        "weapon_cases" => Some([54, 162, 235]),
        "patches" => Some([99, 102, 241]),
        "music_kits" => Some([107, 114, 128]),
        "default_generated" => Some([75, 192, 192]),
        "stickers" => Some([255, 206, 86]),
        "tools" => Some([255, 159, 64]),
        _ => None,
    }
}

fn category_fill(value: &str) -> egui::Color32 {
    match category_rgb(value) {
        Some(rgb) => egui::Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 51),
        None => theme::MUTED,
    }
}

fn category_border(value: &str) -> egui::Color32 {
    match category_rgb(value) {
        Some(rgb) => egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
        None => theme::MUTED,
    }
}

struct Bar {
    name: String,
    inventory: f64,
    storage: f64,
}

struct OverviewData {
    inventory_value: f64,
    inventory_items: usize,
    storage_value: f64,
    storage_items: usize,
    bars: Vec<Bar>,
    /// Combined inventory + storage value per `domain::CATEGORIES` entry.
    categories: Vec<f64>,
}

fn value_of(row: &GroupedItem, by: OverviewBy) -> f64 {
    match by {
        OverviewBy::Price => row.representative.price.unwrap_or(0.0) * row.combined_qty as f64,
        OverviewBy::Volume => row.combined_qty as f64,
    }
}

/// Legacy `barChartOverall`: moveable rows only, aggregated by bare item
/// name, strongest twenty names first (ties broken by name for stability).
fn bar_points(
    inventory_rows: &[&GroupedItem],
    storage_rows: &[&GroupedItem],
    by: OverviewBy,
) -> Vec<Bar> {
    let mut totals: HashMap<String, [f64; 2]> = HashMap::new();
    for (slot, rows) in [(0_usize, inventory_rows), (1, storage_rows)] {
        for row in rows {
            if !row.representative.moveable {
                continue;
            }
            let entry = totals
                .entry(row.representative.name.clone())
                .or_default();
            entry[slot] += value_of(row, by);
        }
    }
    let mut points: Vec<Bar> = totals
        .into_iter()
        .map(|(name, values)| Bar {
            name,
            inventory: values[0],
            storage: values[1],
        })
        .collect();
    points.sort_by(|left, right| {
        (right.inventory + right.storage)
            .total_cmp(&(left.inventory + left.storage))
            .then_with(|| left.name.cmp(&right.name))
    });
    points.truncate(20);
    points
}

/// Legacy `categoryDistribution`: categorized rows contribute price (missing
/// prices count as zero) or volume (every row counts, moveable or not).
fn category_totals(rows: &[&GroupedItem], by: OverviewBy) -> Vec<f64> {
    let mut totals = vec![0.0; domain::CATEGORIES.len()];
    for row in rows {
        let category = row.representative.category.as_deref().unwrap_or("");
        // `InventoryItem.category` carries the display name (classify_category
        // returns `CATEGORIES`' second element), not the path key.
        if let Some(slot) = domain::CATEGORIES.iter().position(|(_, name)| *name == category) {
            totals[slot] += value_of(row, by);
        }
    }
    totals
}

fn overview_data(app: &CasemoveApp) -> OverviewData {
    let snapshot = app.inventory.as_ref();
    let inventory_value = snapshot
        .map(|snapshot| {
            snapshot
                .items
                .iter()
                .map(|item| item.price.unwrap_or(0.0) * item.quantity as f64)
                .sum()
        })
        .unwrap_or(0.0);
    let inventory_items = snapshot.map(|snapshot| snapshot.items.len()).unwrap_or(0);
    let storage_items = snapshot
        .map(|snapshot| {
            snapshot
                .items
                .iter()
                .filter(|item| item.is_storage_unit)
                .map(|item| item.storage_item_count.unwrap_or(0) as usize)
                .sum()
        })
        .unwrap_or(0);
    let storage_value = app
        .storage_aggregate
        .as_ref()
        .map(|items| {
            items
                .iter()
                .map(|item| item.price.unwrap_or(0.0) * item.quantity as f64)
                .sum()
        })
        .unwrap_or(0.0);

    let visible = inventory_view::filtered_rows(&app.grouped, &app.view);
    let storage_grouped = app
        .storage_aggregate
        .as_ref()
        .map(|items| inventory::group_items(items.clone()))
        .unwrap_or_default();
    let storage_visible = inventory_view::filtered_rows(&storage_grouped, &app.view);

    let bars = bar_points(&visible, &storage_visible, app.overview_by);
    let mut categories = category_totals(&visible, app.overview_by);
    for (slot, value) in category_totals(&storage_visible, app.overview_by)
        .into_iter()
        .enumerate()
    {
        categories[slot] += value;
    }

    OverviewData {
        inventory_value,
        inventory_items,
        storage_value,
        storage_items,
        bars,
        categories,
    }
}

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut load = false;
    let mut connect_inventory = false;
    theme::page().show(ui, |ui| {
            header(ui, app, &mut load);
            ui.add_space(10.0);
            if !app.storage_aggregate_error.is_empty() {
                inventory_view::error_box(ui, &app.storage_aggregate_error, None);
            }
            if app.inventory.is_none() {
                let connect = inventory_view::empty_state(
                    ui,
                    "Inventory not loaded",
                    Some((
                        "Load inventory",
                        egui_phosphor::regular::ARROW_CLOCKWISE,
                    )),
                );
                if connect {
                    connect_inventory = true;
                }
                return;
            }
            let data = overview_data(app);
            egui::ScrollArea::vertical().show(ui, |ui| {
                cards(ui, &data, &app.currency);
                ui.add_space(12.0);
                charts(ui, app, &data);
            });
        });
    if load {
        app.load_all_storage();
    }
    if connect_inventory {
        app.refresh_inventory();
    }
}

/// Legacy keys the greeting off the local hour; without a time crate this
/// reads UTC.
fn greeting() -> &'static str {
    let hours = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| (elapsed.as_secs() / 3600) % 24)
        .unwrap_or(18);
    match hours {
        0..=3 => "Wassup",
        4..=11 => "Good morning",
        12 => "Good noon",
        13..=17 => "Good afternoon",
        _ => "Good Evening",
    }
}

fn header(ui: &mut egui::Ui, app: &mut CasemoveApp, load: &mut bool) {
    let mut chosen_market = None;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new(format!(
                    "{}, {}.",
                    greeting(),
                    profile_display_name(app.profile.clone())
                ))
                .size(theme::HEADING_SIZE)
                .color(theme::TEXT_STRONG),
            );
            let units = app
                .inventory
                .as_ref()
                .map(|snapshot| {
                    snapshot
                        .items
                        .iter()
                        .filter(|item| item.is_storage_unit)
                        .count()
                })
                .unwrap_or(0);
            let detail = if app.storage_aggregate_busy {
                "Loading storage units...".to_owned()
            } else if app.storage_aggregate.is_some() {
                format!("{units} storage units loaded")
            } else {
                "Storage units not loaded".to_owned()
            };
            ui.label(
                egui::RichText::new(detail)
                    .size(theme::MUTED_SIZE)
                    .color(theme::MUTED),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(market) = crate::price_source::show(ui, app) {
                chosen_market = Some(market);
            }
            let text = if app.storage_aggregate_busy {
                "Loading…".to_owned()
            } else if app.storage_aggregate.is_some() {
                "Reload units".to_owned()
            } else {
                "Load storage units".to_owned()
            };
            // Legacy: green until every unit has been loaded (or while busy).
            let fill = if app.storage_aggregate.is_none() || app.storage_aggregate_busy {
                theme::GREEN
            } else {
                theme::RAISED
            };
            let button = egui::Button::new(
                egui::RichText::new(format!("{}  {text}", egui_phosphor::regular::FOLDERS))
                    .color(egui::Color32::WHITE),
            )
            .fill(fill);
            let enabled = !app.storage_aggregate_busy && app.inventory.is_some();
            if ui.add_enabled(enabled, button).clicked() {
                *load = true;
            }
        });
    });
    crate::price_source::apply(app, chosen_market);
}

fn cards(ui: &mut egui::Ui, data: &OverviewData, currency: &pricing::Currency) {
    // Equal content widths: the row fits three frames, their 12px side
    // margins, and the two 10px gaps.
    let width = ((ui.available_width() - 92.0) / 3.0).max(140.0);
    let entries: [(&str, &str, f64, usize); 3] = [
        (
            egui_phosphor::regular::DATABASE,
            "Total",
            data.inventory_value + data.storage_value,
            data.inventory_items + data.storage_items,
        ),
        (
            egui_phosphor::regular::FOLDERS,
            "Storage Units",
            data.storage_value,
            data.storage_items,
        ),
        (
            egui_phosphor::regular::ARCHIVE,
            "Inventory",
            data.inventory_value,
            data.inventory_items,
        ),
    ];
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (icon, title, value, items) in entries {
            egui::Frame::NONE
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0_f32, theme::SEPARATOR))
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.set_min_height(56.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(icon).size(18.0).color(theme::MUTED),
                        );
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(title)
                                    .size(11.0)
                                    .color(theme::MUTED),
                            );
                            ui.label(
                                egui::RichText::new(pricing::format_currency(currency, value, 0))
                                    .size(17.0)
                                    .strong()
                                    .color(theme::GREEN),
                            );
                            ui.label(
                                egui::RichText::new(format!("/ {items} items"))
                                    .size(11.0)
                                    .color(theme::MUTED),
                            );
                        });
                    });
                });
        }
    });
}

fn charts(ui: &mut egui::Ui, app: &mut CasemoveApp, data: &OverviewData) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new("Group by")
                .size(theme::MUTED_SIZE)
                .color(theme::MUTED),
        );
        let before = app.overview_by;
        for value in [OverviewBy::Price, OverviewBy::Volume] {
            if ui
                .selectable_value(&mut app.overview_by, value, value.label())
                .clicked()
                && app.overview_by != before
            {
                app.save_overview_by();
            }
        }
    });
    ui.add_space(6.0);

    let by = app.overview_by;
    let total_width = ui.available_width();
    let height = 300.0;
    let spacing = 10.0;
    // Two frames with 10px inner margins on each side plus the gap.
    let usable = (total_width - spacing - 40.0).max(320.0);
    let bar_content = usable * 0.56;
    let pie_content = usable - bar_content;

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = spacing;
        egui::Frame::NONE
            .fill(theme::RAISED)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                let (response, painter) = ui.allocate_painter(
                    egui::vec2(bar_content, height),
                    egui::Sense::hover(),
                );
                paint_bars(&painter, response.rect, &data.bars, by, &app.currency);
            });
        egui::Frame::NONE
            .fill(theme::RAISED)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                let (response, painter) = ui.allocate_painter(
                    egui::vec2(pie_content, height),
                    egui::Sense::hover(),
                );
                paint_pie(&painter, response.rect, data, by, &app.currency);
            });
    });
}

fn paint_bars(
    painter: &egui::Painter,
    rect: egui::Rect,
    bars: &[Bar],
    by: OverviewBy,
    currency: &pricing::Currency,
) {
    painter.text(
        rect.left_top() + egui::vec2(rect.width() / 2.0, 0.0),
        egui::Align2::CENTER_TOP,
        "Overall",
        egui::FontId::proportional(13.0),
        theme::TEXT,
    );

    let mut legend_x = rect.left() + 2.0;
    let legend_y = rect.top() + 22.0;
    for (label, rgb) in [
        ("Inventory", INVENTORY_COLOR),
        ("Storage Units", STORAGE_COLOR),
    ] {
        let swatch = egui::Rect::from_min_size(
            egui::pos2(legend_x, legend_y),
            egui::vec2(10.0, 10.0),
        );
        painter.rect_filled(swatch, 2.0, egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
        painter.text(
            swatch.left_top() + egui::vec2(14.0, -2.0),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(11.0),
            theme::MUTED,
        );
        legend_x += 24.0 + label.len() as f32 * 6.5 + 16.0;
    }

    let plot = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 54.0, rect.top() + 40.0),
        egui::pos2(rect.right() - 6.0, rect.bottom() - 118.0),
    );
    if plot.width() <= 4.0 || plot.height() <= 4.0 {
        return;
    }

    let max = bars
        .iter()
        .map(|bar| bar.inventory + bar.storage)
        .fold(0.0_f64, f64::max);
    let mut step = nice_step(max / 4.0);
    if by == OverviewBy::Volume {
        // Legacy volume counts are whole items; never split a tick below one.
        step = step.max(1.0);
    }
    let axis_max = step * (max / step).ceil().max(1.0);

    let ticks = ((axis_max / step) as usize).min(8);
    for index in 0..=ticks {
        let value = step * index as f64;
        let y = plot.bottom() - (value / axis_max) as f32 * plot.height();
        painter.line_segment(
            [egui::pos2(plot.left(), y), egui::pos2(plot.right(), y)],
            egui::Stroke::new(1.0_f32, theme::SEPARATOR),
        );
        let label = match by {
            OverviewBy::Price => pricing::format_currency(currency, value, 2),
            OverviewBy::Volume => format!("{}", value as u64),
        };
        painter.text(
            egui::pos2(plot.left() - 6.0, y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            theme::MUTED,
        );
    }
    painter.line_segment(
        [plot.left_bottom(), plot.right_bottom()],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );
    painter.line_segment(
        [plot.left_top(), plot.left_bottom()],
        egui::Stroke::new(1.0_f32, theme::BORDER),
    );

    let slot_width = plot.width() / 20.0;
    for (index, bar) in bars.iter().enumerate() {
        let center_x = plot.left() + (index as f32 + 0.5) * slot_width;
        let bar_width = (slot_width * 0.7).max(1.0);
        let left = center_x - bar_width / 2.0;
        let inventory_height = (bar.inventory / axis_max) as f32 * plot.height();
        let storage_height = (bar.storage / axis_max) as f32 * plot.height();

        paint_segment(
            painter,
            egui::Rect::from_min_size(
                egui::pos2(left, plot.bottom() - inventory_height),
                egui::vec2(bar_width, inventory_height),
            ),
            INVENTORY_COLOR,
        );
        paint_segment(
            painter,
            egui::Rect::from_min_size(
                egui::pos2(left, plot.bottom() - inventory_height - storage_height),
                egui::vec2(bar_width, storage_height),
            ),
            STORAGE_COLOR,
        );

        // Legacy labels 40 characters; vertical space here fits ~16.
        let galley = painter.layout_no_wrap(
            truncate(&bar.name, 16),
            egui::FontId::proportional(11.0),
            theme::MUTED,
        );
        let pos = egui::pos2(
            center_x - galley.size().y / 2.0,
            plot.bottom() + 6.0 + galley.size().x,
        );
        painter.add(
            egui::epaint::TextShape::new(pos, galley, theme::MUTED)
                .with_angle(-std::f32::consts::FRAC_PI_2),
        );
    }
}

fn paint_segment(painter: &egui::Painter, rect: egui::Rect, rgb: [u8; 3]) {
    if rect.height() < 0.5 {
        return;
    }
    painter.rect_filled(
        rect,
        0.0,
        egui::Color32::from_rgba_unmultiplied(rgb[0], rgb[1], rgb[2], 51),
    );
    painter.rect_stroke(
        rect,
        0.0,
        egui::Stroke::new(
            1.0_f32,
            egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
        ),
        egui::StrokeKind::Inside,
    );
}

fn paint_pie(
    painter: &egui::Painter,
    rect: egui::Rect,
    data: &OverviewData,
    by: OverviewBy,
    currency: &pricing::Currency,
) {
    painter.text(
        rect.left_top() + egui::vec2(rect.width() / 2.0, 0.0),
        egui::Align2::CENTER_TOP,
        "Category distribution",
        egui::FontId::proportional(13.0),
        theme::TEXT,
    );

    let slices: Vec<(usize, f64)> = data
        .categories
        .iter()
        .enumerate()
        .filter(|(_, value)| **value > 0.0)
        .map(|(index, value)| (index, *value))
        .collect();
    let total: f64 = slices.iter().map(|(_, value)| value).sum();
    if total <= 0.0 {
        return;
    }

    // Legend on the right (chart.js `position: 'right'`), pie on the left.
    let legend_width = (rect.width() * 0.42).min(170.0);
    let pie_area = egui::Rect::from_min_max(
        egui::pos2(rect.left(), rect.top() + 24.0),
        egui::pos2(rect.right() - legend_width, rect.bottom()),
    );
    let radius = (pie_area.width().min(pie_area.height()) / 2.0 - 8.0).max(10.0);
    let center = egui::pos2(
        pie_area.left() + pie_area.width() / 2.0,
        pie_area.top() + pie_area.height() / 2.0,
    );

    // Fills as one triangle fan per slice. A closed `PathShape` wedge with a
    // stroke produces seam/spike artefacts at the centre and along the arc;
    // explicit triangles keep every edge on the circle.
    let mut mesh = egui::Mesh::default();
    let mut angle = -std::f32::consts::FRAC_PI_2;
    for (index, value) in &slices {
        let (value_key, _) = domain::CATEGORIES[*index];
        let span = (value / total) as f32 * std::f32::consts::TAU;
        let steps = ((span / 0.12).ceil() as usize).clamp(2, 128);
        let fill = category_fill(value_key);
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(center, fill);
        for step in 0..=steps {
            let current = angle + span * step as f32 / steps as f32;
            mesh.colored_vertex(
                center + egui::vec2(current.cos(), current.sin()) * radius,
                fill,
            );
        }
        for step in 0..steps as u32 {
            mesh.add_triangle(base, base + 1 + step, base + 2 + step);
        }
        angle += span;
    }
    painter.add(egui::Shape::mesh(mesh));

    // Outline the rim and the slice boundaries without overdrawing the fills.
    painter.circle_stroke(center, radius, egui::Stroke::new(1.0_f32, theme::BORDER));
    let mut edge = -std::f32::consts::FRAC_PI_2;
    for (_, value) in &slices {
        painter.line_segment(
            [center, center + egui::vec2(edge.cos(), edge.sin()) * radius],
            egui::Stroke::new(1.0_f32, theme::SEPARATOR),
        );
        edge += (value / total) as f32 * std::f32::consts::TAU;
    }

    let mut y = rect.top() + 24.0;
    let legend_x = rect.right() - legend_width + 4.0;
    for (index, value) in &slices {
        let (value_key, name) = domain::CATEGORIES[*index];
        let swatch =
            egui::Rect::from_min_size(egui::pos2(legend_x, y + 2.0), egui::vec2(10.0, 10.0));
        painter.rect_filled(swatch, 2.0, category_fill(value_key));
        painter.rect_stroke(
            swatch,
            2.0,
            egui::Stroke::new(1.0_f32, category_border(value_key)),
            egui::StrokeKind::Inside,
        );
        painter.text(
            swatch.left_top() + egui::vec2(15.0, -2.0),
            egui::Align2::LEFT_TOP,
            name,
            egui::FontId::proportional(11.0),
            theme::TEXT,
        );
        let share = value / total * 100.0;
        let detail = match by {
            OverviewBy::Price => {
                format!("{} · {share:.1}%", pricing::format_currency(currency, *value, 2))
            }
            OverviewBy::Volume => format!("{} · {share:.1}%", *value as u64),
        };
        painter.text(
            egui::pos2(legend_x + 15.0, y + 13.0),
            egui::Align2::LEFT_TOP,
            detail,
            egui::FontId::proportional(10.0),
            theme::MUTED,
        );
        y += 29.0;
    }
}

/// Chart.js-style axis step: 1, 2, or 5 times a power of ten.
fn nice_step(raw: f64) -> f64 {
    if !(raw.is_finite() && raw > 0.0) {
        return 1.0;
    }
    let magnitude = 10f64.powf(raw.log10().floor());
    for factor in [1.0, 2.0, 5.0, 10.0] {
        if magnitude * factor >= raw {
            return magnitude * factor;
        }
    }
    10.0 * magnitude
}

fn truncate(name: &str, max: usize) -> String {
    if name.chars().count() <= max {
        return name.to_owned();
    }
    let mut text: String = name.chars().take(max.saturating_sub(1)).collect();
    text.push('…');
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use app_commands::InventoryItem;

    fn grouped(
        name: &str,
        price: Option<f64>,
        qty: u32,
        moveable: bool,
        category: Option<&str>,
    ) -> GroupedItem {
        let item = InventoryItem {
            id: format!("{name}-{qty}-{moveable}"),
            definition_index: 7,
            name: name.to_owned(),
            custom_name: None,
            image_url: None,
            image_path: None,
            quantity: qty,
            rarity: 5,
            position: 0,
            wear: None,
            wear_name: None,
            paint_seed: None,
            stat_trak: false,
            is_storage_unit: false,
            storage_item_count: None,
            category: category.map(str::to_owned),
            major: None,
            trade_unlock: None,
            moveable,
            equipped_ct: false,
            equipped_t: false,
            price,
            collection: None,
            trade_up_eligible: false,
            stickers: Vec::new(),
            music_kit: None,
            graffiti: None,
        };
        GroupedItem {
            combined_qty: qty as usize,
            combined_ids: vec![item.id.clone()],
            representative: item,
        }
    }

    fn refs(rows: &[GroupedItem]) -> Vec<&GroupedItem> {
        rows.iter().collect()
    }

    #[test]
    fn bars_split_sources_by_name_and_skip_non_moveable_rows() {
        let inventory = [
            grouped("B", Some(2.0), 1, true, None),
            grouped("A", Some(5.0), 1, true, None),
            grouped("C", Some(99.0), 1, false, None),
        ];
        let storage = [grouped("B", Some(1.0), 3, true, None)];

        let bars = bar_points(&refs(&inventory), &refs(&storage), OverviewBy::Price);
        // A = 5, B = 2 + 3 = 5 → equal totals tie-break by name; C is not
        // moveable, so its (large) price never appears.
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].name, "A");
        assert_eq!(bars[0].inventory, 5.0);
        assert_eq!(bars[0].storage, 0.0);
        assert_eq!(bars[1].name, "B");
        assert_eq!(bars[1].inventory, 2.0);
        assert_eq!(bars[1].storage, 3.0);
    }

    #[test]
    fn volume_mode_counts_quantities_even_without_prices() {
        let inventory = [
            grouped("Unpriced", None, 4, true, None),
            grouped("Locked", Some(50.0), 2, false, None),
        ];
        let bars = bar_points(&refs(&inventory), &[], OverviewBy::Volume);
        assert_eq!(bars.len(), 1);
        assert_eq!(bars[0].name, "Unpriced");
        assert_eq!(bars[0].inventory, 4.0);
    }

    #[test]
    fn bars_keep_the_strongest_twenty_names() {
        let rows: Vec<GroupedItem> = (0..25)
            .map(|index| grouped(&format!("Item {index:02}"), Some(1.0), 1, true, None))
            .collect();
        let bars = bar_points(&refs(&rows), &[], OverviewBy::Price);
        assert_eq!(bars.len(), 20);
        // Equal totals → name order, first twenty kept.
        assert_eq!(bars[0].name, "Item 00");
        assert_eq!(bars[19].name, "Item 19");
    }

    #[test]
    fn category_totals_skip_uncategorized_and_count_volume_for_every_row() {
        let mut skin = grouped("Skin", Some(10.0), 1, true, None);
        skin.representative.category = Some("Skins & Knives".into());
        let mut base = grouped("Base", None, 2, false, None);
        base.representative.category = Some("Skins & Knives".into());
        let uncategorized = grouped("Odd", Some(99.0), 1, true, None);
        let rows = [&skin, &base, &uncategorized];

        // Price: unpriced rows contribute zero (legacy nan-to-zero) and
        // uncategorized rows are skipped entirely.
        let totals = category_totals(&rows, OverviewBy::Price);
        let slot = domain::CATEGORIES
            .iter()
            .position(|(_, name)| *name == "Skins & Knives")
            .unwrap();
        assert_eq!(totals[slot], 10.0);
        assert_eq!(totals.iter().sum::<f64>(), 10.0);

        // Volume: every categorized row counts, moveable or not.
        let totals = category_totals(&rows, OverviewBy::Volume);
        assert_eq!(totals[slot], 3.0);
        assert_eq!(totals.iter().sum::<f64>(), 3.0);
    }
}
