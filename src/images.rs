use crate::messages::{Message, Reply};
use app_commands::InventoryItem;
use eframe::egui;
use std::collections::HashMap;
use tokio::runtime::Handle;

enum Entry {
    Pending,
    Ready(egui::TextureHandle),
    Failed,
}

/// URL → texture cache for avatars and item thumbnails, fetched off-thread.
pub struct ImageCache {
    entries: HashMap<String, Entry>,
    catalog: HashMap<String, String>,
    rt: Handle,
    reply: Reply,
}

impl ImageCache {
    pub fn new(rt: Handle, reply: Reply) -> Self {
        Self {
            entries: HashMap::new(),
            catalog: HashMap::new(),
            rt,
            reply,
        }
    }

    /// Installs the fetched econ-path → Steam CDN catalog.
    pub fn set_catalog(&mut self, catalog: HashMap<String, String>) {
        self.catalog = catalog;
    }

    pub fn item_url(&self, item: &InventoryItem) -> Option<String> {
        item_url(item, &self.catalog)
    }

    /// Returns the cached texture, spawning a fetch on first use.
    pub fn get(&mut self, url: &str) -> Option<egui::TextureHandle> {
        if !self.entries.contains_key(url) {
            self.entries.insert(url.to_owned(), Entry::Pending);
            let url = url.to_owned();
            let reply = self.reply.clone();
            self.rt.spawn(async move {
                let result = fetch_image(&url).await;
                reply.send(Message::Image { url, result });
            });
        }
        match self.entries.get(url) {
            Some(Entry::Ready(texture)) => Some(texture.clone()),
            _ => None,
        }
    }

    pub fn insert(
        &mut self,
        ctx: &egui::Context,
        url: String,
        result: Result<egui::ColorImage, ()>,
    ) {
        match result {
            Ok(image) => {
                let texture = ctx.load_texture(url.clone(), image, egui::TextureOptions::LINEAR);
                self.entries.insert(url, Entry::Ready(texture));
            }
            Err(()) => {
                self.entries.insert(url, Entry::Failed);
            }
        }
    }
}

/// The render URL for an item: the bundled collections hit first, then the
/// fetched econ-path catalog. Skin renders live under `_light` there — the
/// `_light_large` suffix has no entry.
pub fn item_url(item: &InventoryItem, catalog: &HashMap<String, String>) -> Option<String> {
    if let Some(url) = item.image_url.as_deref().filter(|url| !url.is_empty()) {
        return Some(url.to_owned());
    }
    let path = item.image_path.as_deref()?;
    if let Some(url) = catalog.get(path) {
        return Some(url.clone());
    }
    let alternate = path.strip_suffix("_light_large")?;
    catalog.get(&format!("{alternate}_light")).cloned()
}

async fn fetch_image(url: &str) -> Result<egui::ColorImage, ()> {
    let response = reqwest::get(url)
        .await
        .map_err(|_| ())?
        .error_for_status()
        .map_err(|_| ())?;
    let bytes = response.bytes().await.map_err(|_| ())?;
    tokio::task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes).map_err(|_| ())?.to_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        Ok(egui::ColorImage::from_rgba_unmultiplied(
            size,
            &image.into_raw(),
        ))
    })
    .await
    .map_err(|_| ())?
}

/// Paints `url` into a fixed box (contain-fit), or a fallback glyph when no
/// texture is available yet.
#[allow(clippy::too_many_arguments)]
pub fn media_box(
    ui: &mut egui::Ui,
    cache: &mut ImageCache,
    url: Option<&str>,
    size: egui::Vec2,
    rounding: egui::CornerRadius,
    fallback: &str,
    fallback_size: f32,
    fallback_color: egui::Color32,
) {
    let texture = url
        .filter(|url| !url.is_empty())
        .and_then(|url| cache.get(url));
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, rounding, crate::theme::RAISED);
    match texture {
        Some(texture) => {
            let natural = egui::vec2(texture.size()[0] as f32, texture.size()[1] as f32);
            let scale = (rect.width() / natural.x)
                .min(rect.height() / natural.y)
                .min(1.0);
            let fitted = natural * scale;
            let image = egui::Image::new((texture.id(), fitted)).fit_to_exact_size(fitted);
            image.paint_at(ui, egui::Rect::from_center_size(rect.center(), fitted));
        }
        None => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                fallback,
                egui::FontId::proportional(fallback_size),
                fallback_color,
            );
        }
    }
}

/// Round profile avatar (sidebar, session details, saved accounts).
pub fn avatar(ui: &mut egui::Ui, cache: &mut ImageCache, url: Option<&str>, size: f32) {
    media_box(
        ui,
        cache,
        url,
        egui::vec2(size, size),
        egui::CornerRadius::same((size / 2.0) as u8),
        egui_phosphor::regular::USER,
        size * 0.55,
        crate::theme::ICON,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(image_url: Option<&str>, image_path: Option<&str>) -> InventoryItem {
        InventoryItem {
            id: "1".into(),
            definition_index: 7,
            name: "AK-47".into(),
            custom_name: None,
            image_url: image_url.map(str::to_owned),
            image_path: image_path.map(str::to_owned),
            quantity: 1,
            rarity: 5,
            position: 1,
            wear: None,
            wear_name: None,
            paint_seed: None,
            stat_trak: false,
            is_storage_unit: false,
            storage_item_count: None,
            category: None,
            major: None,
            trade_unlock: None,
            moveable: true,
            equipped_ct: false,
            equipped_t: false,
            price: None,
        }
    }

    fn catalog(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(path, url)| ((*path).to_owned(), (*url).to_owned()))
            .collect()
    }

    #[test]
    fn item_urls_prefer_the_bundled_hit_then_the_catalog() {
        let catalog = catalog(&[
            ("econ/tools/casket", "https://cdn/casket.png"),
            (
                "econ/default_generated/weapon_ak47_cu_ak47_cobra_light",
                "https://cdn/skin.png",
            ),
        ]);

        // The bundled collections URL wins when present.
        let bundled = item(Some("https://cdn/bundled.png"), Some("econ/tools/casket"));
        assert_eq!(
            item_url(&bundled, &catalog).as_deref(),
            Some("https://cdn/bundled.png")
        );

        // Otherwise the econ path resolves through the catalog.
        let catalogued = item(None, Some("econ/tools/casket"));
        assert_eq!(
            item_url(&catalogued, &catalog).as_deref(),
            Some("https://cdn/casket.png")
        );

        // `_light_large` renders are stored as `_light` in the catalog.
        let skin = item(
            None,
            Some("econ/default_generated/weapon_ak47_cu_ak47_cobra_light_large"),
        );
        assert_eq!(
            item_url(&skin, &catalog).as_deref(),
            Some("https://cdn/skin.png")
        );

        // Nothing available → the placeholder glyph.
        assert_eq!(item_url(&item(None, None), &catalog), None);
        assert_eq!(item_url(&item(None, Some("econ/missing")), &catalog), None);
    }
}
