use eframe::egui;

// Slate palette taken from the table reference: a blue-grey page, a darker
// card floating on it, and near-black rows separated by soft hairlines.
pub const BG: egui::Color32 = egui::Color32::from_rgb(0x22, 0x29, 0x37);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x19, 0x1d, 0x27);
pub const CARD: egui::Color32 = egui::Color32::from_rgb(0x19, 0x1d, 0x27);
pub const RAISED: egui::Color32 = egui::Color32::from_rgb(0x23, 0x29, 0x36);
pub const ROW: egui::Color32 = egui::Color32::from_rgb(0x17, 0x1b, 0x25);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(0x8b, 0x93, 0xa7);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x45, 0x4f, 0x62);
pub const SEPARATOR: egui::Color32 = egui::Color32::from_rgb(0x2c, 0x33, 0x42);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xd6, 0xd3, 0xcd);
pub const TEXT_STRONG: egui::Color32 = egui::Color32::from_rgb(0xff, 0xff, 0xff);
pub const ERROR: egui::Color32 = egui::Color32::from_rgb(0xfc, 0xa5, 0xa5);
pub const ERROR_STRONG: egui::Color32 = egui::Color32::from_rgb(0xf8, 0x71, 0x71);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(0x86, 0xef, 0xac);
/// The reference's "+" accent: white text on near-black buttons.
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0xd6, 0xd3, 0xcd);
pub const STATUS_TEXT: egui::Color32 = egui::Color32::from_rgb(0xb6, 0xb5, 0xba);
pub const ICON: egui::Color32 = egui::Color32::from_rgb(0xc5, 0xc6, 0xca);
pub const CHIP_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(0xb9, 0x1c, 0x1c);
/// Row selection wash and the filter-button "active" fill.
pub const SELECTED_ROW: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x23, 0x33);
/// The active page button in the pager.
pub const PAGE_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0x5f, 0x66, 0x70);
/// State-badge fills from the reference (soft grey and mint).
pub const BADGE_CARD: egui::Color32 = egui::Color32::from_rgb(0x23, 0x27, 0x33);
pub const BADGE_MINT_BG: egui::Color32 = egui::Color32::from_rgb(0x1a, 0x3a, 0x2c);
pub const BADGE_MINT_FG: egui::Color32 = egui::Color32::from_rgb(0xb4, 0xff, 0xe5);

// Density tokens: one place to tune how much fits on screen.
pub const PAGE_MARGIN: i8 = 20;
pub const TITLE_SIZE: f32 = 22.0;
pub const HEADING_SIZE: f32 = 20.0;
pub const BODY_SIZE: f32 = 12.0;
pub const MUTED_SIZE: f32 = 11.0;
pub const ROW_HEIGHT: f32 = 72.0;
pub const HEADER_HEIGHT: f32 = 30.0;
/// Card corner radius, shared by the table, panels, and dialogs.
pub const CARD_RADIUS: u8 = 10;
/// Horizontal gap between table columns, shared by every table.
pub const COLUMN_GAP: f32 = 28.0;

pub fn rarity_color(rarity: u32) -> egui::Color32 {
    match rarity {
        2 => egui::Color32::from_rgb(0x8d, 0xb4, 0xd5),
        3 => egui::Color32::from_rgb(0x8c, 0x9d, 0xeb),
        4 => egui::Color32::from_rgb(0xc5, 0x97, 0xf5),
        5 => egui::Color32::from_rgb(0xe1, 0x9a, 0xe4),
        6 => egui::Color32::from_rgb(0xf0, 0x8e, 0x8e),
        7 => egui::Color32::from_rgb(0xe0, 0xb1, 0x5e),
        _ => egui::Color32::from_rgb(0xb0, 0xb0, 0xb0),
    }
}

/// A soft mint state badge, as used for the reference's "Processed" pills.
pub fn mint_badge(ui: &mut egui::Ui, text: &str) -> egui::Response {
    badge(ui, text, BADGE_MINT_BG, BADGE_MINT_FG)
}

/// A neutral state badge: soft slate fill, light text.
pub fn state_badge(ui: &mut egui::Ui, text: &str) -> egui::Response {
    badge(ui, text, BADGE_CARD, TEXT)
}

fn badge(ui: &mut egui::Ui, text: &str, fill: egui::Color32, color: egui::Color32) -> egui::Response {
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(3)
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(11.0).color(color))
        })
        .inner
}

/// Fonts (phosphor icon fallback) and the slate palette taken from the table
/// reference, applied to the whole app.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = CARD;
    visuals.extreme_bg_color = SURFACE;
    visuals.faint_bg_color = ROW;
    visuals.selection.bg_fill = PAGE_ACTIVE;
    visuals.error_fg_color = ERROR;
    visuals.hyperlink_color = egui::Color32::from_rgb(0xa5, 0xb4, 0xfc);
    visuals.window_stroke = egui::Stroke::new(1.0_f32, SEPARATOR);
    visuals.window_corner_radius = egui::CornerRadius::same(CARD_RADIUS);
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 0,
        color: egui::Color32::from_black_alpha(80),
    };
    visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: egui::Color32::from_black_alpha(96),
    };
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, SEPARATOR);
    widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, MUTED);
    widgets.noninteractive.bg_fill = CARD;
    widgets.noninteractive.weak_bg_fill = CARD;
    widgets.inactive.bg_fill = RAISED;
    widgets.inactive.weak_bg_fill = RAISED;
    widgets.inactive.bg_stroke = egui::Stroke::NONE;
    widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    widgets.hovered.bg_fill = egui::Color32::from_rgb(0x2b, 0x33, 0x42);
    widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(0x2b, 0x33, 0x42);
    widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    widgets.hovered.fg_stroke = egui::Stroke::new(1.0_f32, TEXT_STRONG);
    widgets.active.bg_fill = PAGE_ACTIVE;
    widgets.active.weak_bg_fill = PAGE_ACTIVE;
    widgets.active.bg_stroke = egui::Stroke::NONE;
    widgets.active.fg_stroke = egui::Stroke::new(1.0_f32, TEXT_STRONG);
    let rounding = egui::CornerRadius::same(6);
    widgets.noninteractive.corner_radius = rounding;
    widgets.inactive.corner_radius = rounding;
    widgets.hovered.corner_radius = rounding;
    widgets.active.corner_radius = rounding;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(10.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 5.0);
    ctx.set_style(style);
}

/// The floating card every page's content sits in: rounded, with a soft
/// drop shadow that lifts it off the page like the reference's table.
pub fn card() -> egui::Frame {
    egui::Frame::NONE
        .fill(CARD)
        .corner_radius(egui::CornerRadius::same(CARD_RADIUS))
        .shadow(egui::epaint::Shadow {
            offset: [0, 18],
            blur: 40,
            spread: 0,
            color: egui::Color32::from_black_alpha(70),
        })
        .inner_margin(egui::Margin::same(20))
}

/// Inset margin so the card's shadow has room inside the page frame.
pub fn page() -> egui::Frame {
    egui::Frame::NONE.inner_margin(egui::Margin {
        left: PAGE_MARGIN,
        right: PAGE_MARGIN,
        top: PAGE_MARGIN,
        bottom: PAGE_MARGIN + 6,
    })
}
