use eframe::egui;

pub const BG: egui::Color32 = egui::Color32::from_rgb(0x12, 0x12, 0x12);
pub const SURFACE: egui::Color32 = egui::Color32::from_rgb(0x18, 0x18, 0x18);
pub const RAISED: egui::Color32 = egui::Color32::from_rgb(0x28, 0x28, 0x28);
pub const MUTED: egui::Color32 = egui::Color32::from_rgb(0x9c, 0xa3, 0xaf);
pub const BORDER: egui::Color32 = egui::Color32::from_rgb(0x41, 0x41, 0x41);
pub const SEPARATOR: egui::Color32 = egui::Color32::from_rgb(0x30, 0x30, 0x30);
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x4f, 0x46, 0xe5);
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xd6, 0xd3, 0xcd);
pub const ERROR: egui::Color32 = egui::Color32::from_rgb(0xfc, 0xa5, 0xa5);
pub const ERROR_STRONG: egui::Color32 = egui::Color32::from_rgb(0xf8, 0x71, 0x71);
pub const GREEN: egui::Color32 = egui::Color32::from_rgb(0x86, 0xef, 0xac);
pub const STATUS_TEXT: egui::Color32 = egui::Color32::from_rgb(0xb6, 0xb5, 0xba);
pub const ICON: egui::Color32 = egui::Color32::from_rgb(0xc5, 0xc6, 0xca);
pub const CHIP_ACTIVE: egui::Color32 = egui::Color32::from_rgb(0x3b, 0x82, 0xf6);
pub const DANGER: egui::Color32 = egui::Color32::from_rgb(0xb9, 0x1c, 0x1c);
/// Row selection wash and the filter-button "active" fill.
pub const SELECTED_ROW: egui::Color32 = egui::Color32::from_rgb(0x1e, 0x20, 0x33);
/// Metadata badges (StatTrak, unit markers).
pub const BADGE_GOLD: egui::Color32 = egui::Color32::from_rgb(0xd6, 0xc1, 0x8a);

// Density tokens: one place to tune how much fits on screen.
pub const PAGE_MARGIN: i8 = 16;
pub const TITLE_SIZE: f32 = 20.0;
pub const BODY_SIZE: f32 = 12.0;
pub const MUTED_SIZE: f32 = 11.0;
pub const ROW_HEIGHT: f32 = 48.0;
pub const HEADER_HEIGHT: f32 = 24.0;

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

/// Fonts (phosphor icon fallback) and the charcoal palette from the legacy
/// stylesheet.
pub fn install(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);

    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = SURFACE;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = BG;
    visuals.faint_bg_color = RAISED;
    visuals.selection.bg_fill = ACCENT;
    visuals.error_fg_color = ERROR;
    visuals.hyperlink_color = egui::Color32::from_rgb(0xa5, 0xb4, 0xfc);
    visuals.window_stroke = egui::Stroke::new(1.0_f32, BORDER);
    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    widgets.noninteractive.bg_fill = SURFACE;
    widgets.inactive.bg_fill = RAISED;
    widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
    widgets.inactive.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    widgets.inactive.fg_stroke = egui::Stroke::new(1.0_f32, TEXT);
    widgets.hovered.bg_fill = egui::Color32::from_rgb(0x33, 0x33, 0x33);
    widgets.hovered.weak_bg_fill = egui::Color32::from_rgb(0x33, 0x33, 0x33);
    widgets.hovered.bg_stroke = egui::Stroke::new(1.0_f32, BORDER);
    widgets.active.bg_fill = ACCENT;
    widgets.active.weak_bg_fill = ACCENT;
    widgets.active.bg_stroke = egui::Stroke::new(1.0_f32, ACCENT);
    let rounding = egui::CornerRadius::same(6);
    widgets.noninteractive.corner_radius = rounding;
    widgets.inactive.corner_radius = rounding;
    widgets.hovered.corner_radius = rounding;
    widgets.active.corner_radius = rounding;
    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    ctx.set_style(style);
}
