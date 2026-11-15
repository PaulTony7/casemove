use eframe::egui;

/// Window icon for the OS titlebar/decorations.
pub fn window_icon() -> Option<egui::IconData> {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/casemove.png")).ok()?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let width = image.width();
    let height = image.height();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

/// Rasterized Casemove brand mark used in the sidebar header.
pub fn load_brand(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/casemove.png")).ok()?;
    let image = image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, &image.into_raw());
    Some(ctx.load_texture("casemove-brand", image, egui::TextureOptions::LINEAR))
}
