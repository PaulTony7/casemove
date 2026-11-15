//! The shared price-source control. Every page that shows prices or a money
//! total draws it in the top-right of its header, so switching providers
//! looks and behaves the same everywhere.

use crate::app::CasemoveApp;
use crate::theme;
use eframe::egui;

/// Draws the picker for the app's current live price source. Right-to-left
/// layouts place it first, i.e. at the far right of the header row.
pub fn show(ui: &mut egui::Ui, app: &CasemoveApp) -> Option<pricing::Market> {
    let current = app.price_market;
    let mut chosen = None;
    let button = ui
        .add(
            egui::Button::new(
                egui::RichText::new(format!(
                    "{}  {}",
                    egui_phosphor::regular::TAG,
                    current.label()
                ))
                .size(12.0),
            )
            .fill(theme::RAISED),
        )
        .on_hover_text(format!("Prices come from {}", current.basis()));
    let _ = egui::Popup::menu(&button)
        .id(egui::Id::new("price-source"))
        .show(|ui| {
            ui.set_min_width(200.0);
            for option in pricing::Market::ALL {
                let checked = option == current;
                let text = if checked {
                    format!("{}  {}", egui_phosphor::regular::CHECK, option.label())
                } else {
                    format!("    {}", option.label())
                };
                if ui
                    .selectable_label(checked, text)
                    .on_hover_text(option.basis())
                    .clicked()
                {
                    chosen = Some(option);
                }
            }
        });
    chosen
}

/// Applies the picker's result. Call after the page has finished borrowing
/// the app mutably.
pub fn apply(app: &mut CasemoveApp, chosen: Option<pricing::Market>) {
    if let Some(market) = chosen {
        app.set_price_market(market);
    }
}
