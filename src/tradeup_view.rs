//! Trade-up contracts: a filtered picker over inventory and loaded storage
//! units, the ten-slot contract sidebar with possible outcomes and expected
//! value, and the review dialog that submits the contract.

use crate::app::CasemoveApp;
use crate::images::{self, ImageCache};
use crate::theme;
use domain::{Contract, ContractFilter, Outcome};
use eframe::egui;

/// Picker row derived from an inventory or storage item: legacy keeps
/// `tradeUp` false for Covert items, knives, Souvenirs, and non-generated
/// skins, and leaves `rarityName` unset outside that gate (such rows never
/// match).
pub fn candidate_from(item: &app_commands::InventoryItem) -> Option<domain::CandidateItem> {
    let quality = if item.name.starts_with('★') { 3 } else { 0 };
    if item.is_storage_unit
        || !domain::is_trade_up_skin(&item.name, item.rarity, quality, item.image_path.as_deref())
        || !domain::Collections::bundled().is_eligible(&item.name)
    {
        return None;
    }
    let collection = domain::Collections::bundled()
        .collection_of(&item.name)
        .unwrap_or_default()
        .to_owned();
    Some(domain::CandidateItem {
        id: item.id.clone(),
        name: item.name.clone(),
        custom_name: item.custom_name.clone(),
        collection,
        image_url: item.image_url.clone(),
        rarity_name: domain::rarity_name(item.rarity)?,
        stat_trak: item.stat_trak,
        wear: item.wear,
        wear_name: item.wear_name.clone(),
        price: item.price,
        equipped_t: item.equipped_t,
        equipped_ct: item.equipped_ct,
    })
}

/// Collections that currently offer a pickable item.
fn collections(rows: &[domain::CandidateItem]) -> Vec<String> {
    let mut names: Vec<String> = rows
        .iter()
        .map(|row| row.collection.clone())
        .filter(|name| !name.is_empty())
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut submit = false;

    theme::page().show(ui, |ui| {
            let rows = app.trade_up.candidates.clone();
            header(ui, app, &mut submit);
            ui.add_space(8.0);
            if !app.trade_up.notice.is_empty() {
                ui.label(
                    egui::RichText::new(&app.trade_up.notice)
                        .size(theme::MUTED_SIZE)
                        .color(theme::MUTED),
                );
            }
            filters(ui, app, &rows);
            ui.add_space(6.0);

            let sidebar = 330.0;
            let picker_width = (ui.available_width() - sidebar - 24.0).max(320.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                ui.allocate_ui_with_layout(
                    egui::vec2(picker_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| theme::card().show(ui, |ui| picker(ui, app, &rows)),
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(sidebar, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| theme::card().show(ui, |ui| sidebar_panel(ui, app)),
                );
            });
        });

    if submit {
        app.trade_up_review = true;
    }
}

fn header(ui: &mut egui::Ui, app: &mut CasemoveApp, submit: &mut bool) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                egui::RichText::new("Trade up contracts")
                    .size(theme::HEADING_SIZE)
                    .color(theme::TEXT_STRONG),
            );
            let count = app.trade_up.contract.items.len();
            ui.label(
                egui::RichText::new(format!(
                    "{count} / {} selected · average float {:.9}",
                    Contract::CAPACITY,
                    app.trade_up.contract.average_wear()
                ))
                .size(theme::MUTED_SIZE)
                .color(theme::MUTED),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_submit = app.trade_up.contract.is_full() && !app.trade_up.busy;
            let label = if app.trade_up.busy {
                "Submitting...".to_owned()
            } else {
                format!("{}  Submit trade-up", egui_phosphor::regular::CHECK_CIRCLE)
            };
            if ui
                .add_enabled(can_submit, egui::Button::new(label))
                .on_hover_text("Sends the contract to the CS2 Game Coordinator")
                .clicked()
            {
                *submit = true;
            }
            if ui
                .add_enabled(
                    !app.trade_up.contract.items.is_empty(),
                    egui::Button::new("Clear all"),
                )
                .clicked()
            {
                app.trade_up.contract = Contract::default();
                app.trade_up.outcomes.clear();
            }
            let loaded = app.storage_unit_contents.len();
            let storage_label = if app.storage_aggregate_busy {
                "Loading storage...".to_owned()
            } else if loaded > 0 {
                format!("{loaded} storage units loaded")
            } else {
                format!("{}  Load storage units", egui_phosphor::regular::CLOUD_ARROW_DOWN)
            };
            if ui
                .add_enabled(
                    !app.storage_aggregate_busy
                        && !app.session_busy
                        && !app.inventory_busy
                        && app.inventory.is_some(),
                    egui::Button::new(storage_label),
                )
                .on_hover_text(if loaded > 0 {
                    "Reload every storage unit"
                } else {
                    "Include items kept in storage units"
                })
                .clicked()
            {
                app.load_all_storage();
            }
        });
    });
}

fn filters(ui: &mut egui::Ui, app: &mut CasemoveApp, rows: &[domain::CandidateItem]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.add(
            egui::TextEdit::singleline(&mut app.trade_up.query)
                .hint_text("Search items")
                .desired_width(220.0),
        );
        ui.checkbox(&mut app.trade_up.hide_equipped, "Hide equipped");

        let available = collections(rows);
        let picked = &mut app.trade_up.selected_collections;
        egui::ComboBox::from_id_salt("trade-up-collections")
            .selected_text(if picked.is_empty() {
                "All collections".to_owned()
            } else {
                format!("{} collections", picked.len())
            })
            .width(190.0)
            .show_ui(ui, |ui| {
                for name in &available {
                    let mut on = picked.contains(name);
                    if ui.checkbox(&mut on, name).changed() {
                        if on {
                            picked.push(name.clone());
                        } else {
                            picked.retain(|entry| entry != name);
                        }
                    }
                }
            });

        ui.label(
            egui::RichText::new("Float")
                .size(theme::MUTED_SIZE)
                .color(theme::MUTED),
        );
        ui.add(
            egui::DragValue::new(&mut app.trade_up.min_float)
                .speed(0.001)
                .range(0.0..=1.0)
                .max_decimals(9),
        )
        .on_hover_text("Minimum float");
        ui.label(
            egui::RichText::new("to")
                .size(theme::MUTED_SIZE)
                .color(theme::MUTED),
        );
        ui.add(
            egui::DragValue::new(&mut app.trade_up.max_float)
                .speed(0.001)
                .range(0.0..=1.0)
                .max_decimals(9),
        )
        .on_hover_text("Maximum float");
        // Legacy only let min rise below max and max fall above min.
        if app.trade_up.min_float > app.trade_up.max_float {
            app.trade_up.min_float = app.trade_up.max_float;
        }
    });
    app.trade_up.filter = current_filter(app);
}

fn current_filter(app: &CasemoveApp) -> ContractFilter {
    ContractFilter {
        search: app.trade_up.query.clone(),
        min_float: app.trade_up.min_float,
        max_float: app.trade_up.max_float,
        collections: app.trade_up.selected_collections.clone(),
        hide_equipped: app.trade_up.hide_equipped,
        selected: app
            .trade_up
            .contract
            .items
            .iter()
            .map(|item| item.id.clone())
            .collect(),
    }
}

fn picker(ui: &mut egui::Ui, app: &mut CasemoveApp, rows: &[domain::CandidateItem]) {
    if app.inventory.is_none() {
        ui.label(
            egui::RichText::new("Load the inventory to pick trade-up inputs.")
                .size(theme::BODY_SIZE)
                .color(theme::MUTED),
        );
        return;
    }
    let filter = current_filter(app);
    let custom_names: std::collections::HashMap<String, String> = rows
        .iter()
        .filter_map(|row| Some((row.id.clone(), row.custom_name.clone()?)))
        .collect();
    let mut visible: Vec<&domain::CandidateItem> = rows
        .iter()
        .filter(|row| {
            domain::picker_matches(
                row,
                custom_names.get(&row.id).map(String::as_str),
                &filter,
            )
        })
        .collect();
    // Same rarity/StatTrak lock as legacy, applied after the shared filters.
    if let Some(first) = app.trade_up.contract.items.first() {
        visible.retain(|row| {
            row.rarity_name == first.rarity_name && row.stat_trak == first.stat_trak
        });
    }
    visible.sort_by(|left, right| {
        left.name
            .to_lowercase()
            .cmp(&right.name.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });

    let width = ui.available_width();
    let height = (ui.available_height() - theme::HEADER_HEIGHT).max(160.0);
    let mut clicked: Option<domain::CandidateItem> = None;
    let header_rect = egui::Rect::from_min_size(
        ui.cursor().min,
        egui::vec2(ui.available_width(), theme::HEADER_HEIGHT),
    );
    let painter = ui.painter().clone();
    // Wider gaps between columns, so each one reads as its own block.
    ui.spacing_mut().item_spacing.x = theme::COLUMN_GAP;
    let table = egui_extras::TableBuilder::new(ui)
        .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
        .id_salt("trade-up-picker")
        .max_scroll_height(height)
        .column(egui_extras::Column::remainder().at_least(220.0))
        .column(egui_extras::Column::initial(width * 0.28).at_least(120.0))
        .column(egui_extras::Column::initial(90.0))
        .column(egui_extras::Column::initial(70.0))
        .column(egui_extras::Column::exact(64.0))
        .header(theme::HEADER_HEIGHT, |mut header| {
            for label in ["Product", "Collection", "Price", "Float", ""] {
                header.col(|ui| {
                    ui.label(egui::RichText::new(label).size(theme::MUTED_SIZE).color(theme::MUTED));
                });
            }
        });
    table.body(|body| {
        let mut header_drawn = false;
        body.rows(theme::ROW_HEIGHT, visible.len(), |mut row| {
            {
                let candidate = visible[row.index()];
                row.col(|ui| {
                    crate::inventory_view::cell(ui, |ui| {
                        product_cell(
                            ui,
                            &mut app.images,
                            candidate,
                            custom_names.get(&candidate.id).map(String::as_str),
                        );
                    });
                });
                row.col(|ui| {
                    crate::inventory_view::cell(ui, |ui| {
                        ui.label(
                            egui::RichText::new(&candidate.collection)
                                .size(theme::MUTED_SIZE)
                                .color(theme::MUTED),
                        );
                    });
                });
                row.col(|ui| {
                    crate::inventory_view::cell(ui, |ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(
                                    candidate
                                        .price
                                        .map(|price| app.money(price))
                                        .unwrap_or_else(|| "-".to_owned()),
                                )
                                .size(theme::BODY_SIZE),
                            );
                        });
                    });
                });
                row.col(|ui| {
                    crate::inventory_view::cell(ui, |ui| {
                        ui.label(
                            egui::RichText::new(
                                candidate
                                    .wear
                                    .map(|wear| format!("{wear:.6}"))
                                    .unwrap_or_else(|| "-".to_owned()),
                            )
                            .size(theme::MUTED_SIZE)
                            .color(theme::MUTED),
                        );
                    });
                });
                row.col(|ui| {
                    let full = app.trade_up.contract.is_full();
                    if ui
                        .add_enabled(
                            !full,
                            egui::Button::new(egui_phosphor::regular::PLUS).frame(false),
                        )
                        .on_hover_text(if full {
                            "The contract already holds ten items"
                        } else {
                            "Add to the contract"
                        })
                        .clicked()
                    {
                        clicked = Some(candidate.clone());
                    }
                });
                let rect = row.response().rect;
                if !header_drawn {
                    header_drawn = true;
                    painter.hline(
                        header_rect.x_range(),
                        rect.top(),
                        egui::Stroke::new(1.0_f32, theme::SEPARATOR),
                    );
                }
                painter.hline(
                    header_rect.x_range(),
                    rect.bottom(),
                    egui::Stroke::new(1.0_f32, theme::SEPARATOR),
                );
            }
        });
    });
    if let Some(candidate) = clicked {
        let custom_name = custom_names.get(&candidate.id).cloned();
        app.add_trade_up_candidate(candidate, custom_name);
    }
}

fn product_cell(
    ui: &mut egui::Ui,
    images: &mut ImageCache,
    candidate: &domain::CandidateItem,
    custom_name: Option<&str>,
) {
    ui.horizontal(|ui| {
        // Legacy drew the rarity dot from the item's rarity class.
        let dot = egui::vec2(7.0, 7.0);
        let (rect, _) = ui.allocate_exact_size(dot, egui::Sense::hover());
        ui.painter().circle_filled(
            rect.center(),
            3.5,
            theme::rarity_color(gc_rarity(candidate.rarity_name)),
        );
        images::media_box(
            ui,
            images,
            candidate.image_url.as_deref(),
            egui::vec2(52.0, 40.0),
            egui::CornerRadius::same(3),
            egui_phosphor::regular::CUBE,
            18.0,
            theme::ICON,
        );
        ui.vertical(|ui| {
            ui.label(egui::RichText::new(&candidate.name).size(theme::BODY_SIZE));
            let mut detail = candidate.wear_name.clone().unwrap_or_default();
            if let Some(name) = custom_name.filter(|name| !name.is_empty()) {
                detail.push_str(&format!(" · {name}"));
            }
            if !detail.is_empty() {
                ui.label(
                    egui::RichText::new(detail)
                        .size(10.0)
                        .color(theme::MUTED),
                );
            }
        });
    });
}

fn sidebar_panel(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    // Slot row of ten thumbnail circles, legacy's `productsToUse`.
    let mut remove: Option<String> = None;
    ui.horizontal_wrapped(|ui| {
        for index in 0..Contract::CAPACITY {
            match app.trade_up.contract.items.get(index) {
                Some(item) => {
                    let response = ui
                        .push_id(("slot", index), |ui| {
                            images::media_box(
                                ui,
                                &mut app.images,
                                item.image_url.as_deref(),
                                egui::vec2(28.0, 28.0),
                                egui::CornerRadius::same(14),
                                egui_phosphor::regular::CUBE,
                                12.0,
                                theme::ICON,
                            )
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_text(format!(
                            "{}\n{}",
                            item.name,
                            item.wear.map(|wear| format!("{wear:.9}")).unwrap_or_default()
                        ));
                    if response.clicked() {
                        remove = Some(item.id.clone());
                    }
                }
                None => {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                    ui.painter().circle_stroke(
                        rect.center(),
                        13.5,
                        egui::Stroke::new(1.0_f32, theme::BORDER),
                    );
                }
            }
        }
    });
    if let Some(id) = remove {
        app.remove_trade_up_item(&id);
    }
    ui.add_space(10.0);

    let total_price: f64 = app
        .trade_up
        .contract
        .items
        .iter()
        .map(|item| item.price.unwrap_or(0.0))
        .sum();
    let expected_value: f64 = app
        .trade_up
        .outcomes
        .iter()
        .map(|outcome| trade_up_outcome_price(outcome).unwrap_or(0.0) * (outcome.percentage / 100.0))
        .sum();
    summary_row(ui, "Cost", &app.money(total_price), theme::ERROR_STRONG);
    summary_row(ui, "Expected value", &app.money(expected_value), theme::GREEN);
    summary_row(ui, "Profit", &app.money(expected_value - total_price), theme::STATUS_TEXT);
    let roi = if total_price > 0.0 {
        expected_value / total_price * 100.0
    } else {
        0.0
    };
    summary_row(ui, "Return", &format!("{roi:.2} %"), theme::STATUS_TEXT);
    ui.add_space(10.0);

    ui.label(
        egui::RichText::new("Possible outcomes")
            .size(theme::MUTED_SIZE)
            .color(theme::MUTED),
    );
    ui.add_space(4.0);
    if app.trade_up.outcomes.is_empty() {
        ui.label(
            egui::RichText::new("Add items to see the results.")
                .size(theme::BODY_SIZE)
                .color(theme::MUTED),
        );
        return;
    }
    let mut outcomes = app.trade_up.outcomes.clone();
    // Legacy sorts by profit per outcome (`profit_cal`), best first.
    outcomes.sort_by(|left, right| {
        let profit = |outcome: &Outcome| {
            let price = trade_up_outcome_price(outcome).unwrap_or(0.0);
            if total_price > 0.0 {
                price / total_price * 100.0
            } else {
                0.0
            }
        };
        profit(right)
            .partial_cmp(&profit(left))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    egui::ScrollArea::vertical().id_salt("trade-up-outcomes").show(ui, |ui| {
        for outcome in &outcomes {
            outcome_row(ui, &mut app.images, outcome, total_price);
        }
    });
}

fn trade_up_outcome_price(outcome: &Outcome) -> Option<f64> {
    pricing::resolve(
        &outcome.name,
        Some(outcome.wear_name),
        pricing::PriceSource::SteamListing,
        None,
    )
}

fn summary_row(ui: &mut egui::Ui, label: &str, value: &str, color: egui::Color32) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(theme::MUTED_SIZE).color(theme::MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).size(theme::BODY_SIZE).color(color));
        });
    });
}

fn outcome_row(ui: &mut egui::Ui, images: &mut ImageCache, outcome: &Outcome, cost: f64) {
    let price = trade_up_outcome_price(outcome).unwrap_or(0.0);
    let profit = if cost > 0.0 { price / cost * 100.0 } else { 0.0 };
    egui::Frame::NONE
        .fill(theme::CARD)
        .corner_radius(6)
        .stroke(egui::Stroke::new(1.0_f32, theme::SEPARATOR))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                images::media_box(
                    ui,
                    images,
                    outcome.image_url.as_deref(),
                    egui::vec2(52.0, 40.0),
                    egui::CornerRadius::same(3),
                    egui_phosphor::regular::CUBE,
                    18.0,
                    theme::ICON,
                );
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(&outcome.name).size(theme::BODY_SIZE));
                    ui.label(
                        egui::RichText::new(format!(
                            "{} % | {} | {:.8}",
                            outcome.percentage,
                            wear_short(outcome.wear_name),
                            outcome.float
                        ))
                        .size(theme::MUTED_SIZE)
                        .color(theme::MUTED),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let dot = egui::vec2(7.0, 7.0);
                    let (rect, _) = ui.allocate_exact_size(dot, egui::Sense::hover());
                    let color = if profit > 100.0 { theme::GREEN } else { theme::ERROR_STRONG };
                    ui.painter().circle_filled(rect.center(), 3.5, color);
                    ui.label(
                        egui::RichText::new(format!("{profit:.2} %"))
                            .size(theme::MUTED_SIZE)
                            .color(theme::MUTED),
                    );
                });
            });
        });
    ui.add_space(4.0);
}

fn wear_short(wear: &str) -> &'static str {
    match wear {
        "Factory New" => "FN",
        "Minimal Wear" => "MW",
        "Field-Tested" => "FT",
        "Well-Worn" => "WW",
        _ => "BS",
    }
}

/// GC rarity for a legacy rarity label (only for the row dot's colour).
fn gc_rarity(name: &str) -> u32 {
    match name {
        "Consumer Grade" => 1,
        "Industrial Grade" => 2,
        "Mil-Spec" => 3,
        "Restricted" => 4,
        "Classified" => 5,
        _ => 6,
    }
}
