//! Settings page: display currency and the Steam-close prompt preference.
//! Values persist through the same dot-path store the legacy app used.

use crate::app::CasemoveApp;
use crate::theme;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, app: &mut CasemoveApp) {
    let mut currency = app.currency_code.clone();
    let mut steam_login = app.show_steam_login;
    let mut dev_mode = app.dev_mode;

    egui::Frame::NONE
        .inner_margin(egui::Margin {
            left: theme::PAGE_MARGIN,
            right: theme::PAGE_MARGIN,
            top: 14,
            bottom: 10,
        })
        .show(ui, |ui| {
            ui.label(egui::RichText::new("Settings").size(theme::TITLE_SIZE).strong());
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
                |ui| {
                    let current = currency.clone();
                    egui::ComboBox::from_id_salt("settings-currency")
                        .selected_text(&current)
                        .width(120.0)
                        .show_ui(ui, |ui| {
                            for code in pricing::currency_codes() {
                                ui.selectable_value(&mut currency, code.to_owned(), code);
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
            row(
                ui,
                "Dev mode",
                "Gives additional dev features (legacy kept this preference only).",
                |ui| {
                    ui.checkbox(&mut dev_mode, "");
                },
            );
        });

    if currency != app.currency_code {
        app.set_currency(&currency);
    }
    if steam_login != app.show_steam_login {
        app.show_steam_login = steam_login;
        app.save_steam_login();
    }
    if dev_mode != app.dev_mode {
        app.set_dev_mode(dev_mode);
    }
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
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
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
