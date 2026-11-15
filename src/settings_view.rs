//! Settings page: display currency, UI scale, and the Steam-close prompt.
//! Values persist through the same dot-path store the legacy app used.

use crate::app::CasemoveApp;
use crate::theme;
use eframe::egui;

/// Zoom steps offered in percent, 100–200% in tens.
const ZOOM_MIN: u32 = 100;
const ZOOM_MAX: u32 = 200;
const ZOOM_STEP: u32 = 10;

/// Where the currency popup keeps its search text between frames.
fn currency_search_id() -> egui::Id {
    egui::Id::new("settings-currency-search")
}

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut currency = app.currency_code.clone();
    let mut market = app.price_market;
    let mut steam_login = app.show_steam_login;
    let mut zoom = app.saved_zoom;

    theme::page().show(ui, |ui| {
        ui.label(
            egui::RichText::new("Settings")
                .size(theme::HEADING_SIZE)
                .color(theme::TEXT_STRONG),
        );
        ui.label(
            egui::RichText::new("Application and pricing preferences")
                .size(theme::MUTED_SIZE)
                .color(theme::MUTED),
        );
        ui.add_space(16.0);

        section(ui, "Pricing");
        row(
            ui,
            "Currency",
            "Prices are converted with bundled USD rates.",
            |ui| currency_picker(ui, &mut currency),
        );
        row(
            ui,
            "Price source",
            "Where live quotes come from; also switchable from every page header.",
            |ui| {
                if let Some(chosen) = crate::price_source::show(ui, &*app) {
                    market = chosen;
                }
            },
        );

        ui.add_space(16.0);
        section(ui, "Display");
        row(
            ui,
            "Zoom",
            "Scales the whole interface; Ctrl + and − also work.",
            |ui| {
                let current = (zoom * 100.0).round() as u32;
                egui::ComboBox::from_id_salt("settings-zoom")
                    .selected_text(format!("{current}%"))
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for percent in (ZOOM_MIN..=ZOOM_MAX).step_by(ZOOM_STEP as usize) {
                            let value = percent as f32 / 100.0;
                            ui.selectable_value(&mut zoom, value, format!("{percent}%"));
                        }
                    });
            },
        );

        ui.add_space(16.0);
        section(ui, "General");
        row(
            ui,
            "Show close popup",
            "Shows a popup when you sign in while Steam is open.",
            |ui| {
                ui.checkbox(&mut steam_login, "");
            },
        );
    });

    if currency != app.currency_code {
        app.set_currency(&currency);
    }
    app.set_price_market(market);
    if (zoom - app.saved_zoom).abs() > 0.001 {
        app.set_zoom(ui.ctx(), zoom);
    }
    if steam_login != app.show_steam_login {
        app.show_steam_login = steam_login;
        app.save_steam_login();
    }
}

/// The currency list: the common currencies first, then every other code the
/// rate table covers, with a search box that narrows both as you type.
fn currency_picker(ui: &mut egui::Ui, currency: &mut String) {
    let common = pricing::currency_common();
    let others: Vec<&str> = pricing::currency_codes()
        .into_iter()
        .filter(|code| !common.iter().any(|(known, _)| known == code))
        .collect();
    let combo = egui::ComboBox::from_id_salt("settings-currency");
    let open = egui::ComboBox::is_open(ui.ctx(), egui::Id::new("settings-currency"));
    let mut query = ui
        .data(|data| data.get_temp::<String>(currency_search_id()))
        .unwrap_or_default();
    // A fresh popup starts from the whole list, not the last search.
    if !open {
        query.clear();
    }

    combo.selected_text(currency.clone()).width(120.0).show_ui(ui, |ui| {
        ui.set_max_height(320.0);
        ui.add(
            egui::TextEdit::singleline(&mut query)
                .hint_text("Search currencies")
                .desired_width(200.0),
        );
        let needle = query.trim().to_uppercase();
        let matches = |code: &str, symbol: &str| {
            needle.is_empty()
                || code.to_uppercase().contains(&needle)
                || symbol.to_uppercase().contains(&needle)
        };
        let shown: Vec<(&str, &str)> = common
            .iter()
            .copied()
            .filter(|(code, symbol)| matches(code, symbol))
            .collect();
        let rest: Vec<&str> = others.iter().copied().filter(|code| matches(code, "")).collect();
        if shown.is_empty() && rest.is_empty() {
            ui.label(
                egui::RichText::new("No matching currencies")
                    .size(theme::MUTED_SIZE)
                    .color(theme::MUTED),
            );
            return;
        }
        egui::ScrollArea::vertical()
            .id_salt("settings-currency-list")
            .max_height(320.0)
            .show(ui, |ui| {
                if !shown.is_empty() {
                    group_label(ui, "Common");
                }
                for (code, symbol) in shown {
                    let label = match drawable_symbol(ui.ctx(), symbol) {
                        Some(symbol) => format!("{code}  {symbol}"),
                        None => code.to_owned(),
                    };
                    if ui.selectable_label(currency == code, label).clicked() {
                        *currency = code.to_owned();
                    }
                }
                if !rest.is_empty() {
                    if !common.is_empty() {
                        ui.separator();
                    }
                    group_label(ui, "More");
                }
                for code in rest {
                    if ui.selectable_label(currency == code, code).clicked() {
                        *currency = code.to_owned();
                    }
                }
            });
    });
    ui.data_mut(|data| data.insert_temp(currency_search_id(), query));
}

/// A currency symbol, but only when the bundled font can actually draw it;
/// otherwise the currency is listed by code alone rather than showing boxes.
fn drawable_symbol<'a>(ctx: &egui::Context, symbol: &'a str) -> Option<&'a str> {
    if symbol.trim().is_empty() {
        return None;
    }
    let font = egui::FontId::proportional(theme::MUTED_SIZE);
    ctx.fonts_mut(|fonts| fonts.has_glyphs(&font, symbol))
        .then_some(symbol.trim())
}

fn group_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme::MUTED_SIZE)
            .color(theme::MUTED),
    );
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(15.0).strong());
    ui.add_space(4.0);
}

fn row(
    ui: &mut egui::Ui,
    title: &str,
    hint: &str,
    control: impl FnOnce(&mut egui::Ui),
) {
    let control_width = 200.0;
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            // Leave the control its own space so long hints never run under it.
            ui.set_max_width((ui.available_width() - control_width).max(120.0));
            ui.label(egui::RichText::new(title).size(theme::BODY_SIZE));
            ui.label(
                egui::RichText::new(hint)
                    .size(theme::MUTED_SIZE)
                    .color(theme::MUTED),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            control(ui);
        });
    });
    ui.add_space(8.0);
}
