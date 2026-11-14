use crate::messages::{Message, Reply};
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
    rt: Handle,
    reply: Reply,
}

impl ImageCache {
    pub fn new(rt: Handle, reply: Reply) -> Self {
        Self {
            entries: HashMap::new(),
            rt,
            reply,
        }
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

async fn fetch_image(url: &str) -> Result<egui::ColorImage, ()> {
    let bytes = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let response = reqwest::get(url)
            .await
            .map_err(|_| ())?
            .error_for_status()
            .map_err(|_| ())?;
        if response
            .content_length()
            .is_some_and(|length| length > 16 * 1024 * 1024)
        {
            return Err(());
        }
        let bytes = response.bytes().await.map_err(|_| ())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err(());
        }
        Ok(bytes)
    })
    .await
    .map_err(|_| ())??;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires public Internet access"]
    fn live_case_sticker_and_graffiti_images_decode() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            for catalog in ["crates", "stickers", "graffiti"] {
                let response = reqwest::get(format!(
                    "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/{catalog}.json"
                ))
                .await
                .unwrap()
                .text()
                .await
                .unwrap();
                let entries: Vec<serde_json::Value> = serde_json::from_str(&response).unwrap();
                let entry = entries.iter().find(|entry| match catalog {
                    "crates" => entry["name"] == "Recoil Case",
                    "graffiti" => entry["color_index"] == 1,
                    _ => entry["market_hash_name"].is_string(),
                }).unwrap();
                let url = entry["image"].as_str().unwrap().replace(
                    "community.akamai.steamstatic.com",
                    "community.steamstatic.com",
                );
                let image = fetch_image(&url).await.unwrap_or_else(|()| {
                    panic!("Could not download and decode {catalog} image")
                });
                assert!(image.size[0] > 0 && image.size[1] > 0);
            }
        });
    }
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
