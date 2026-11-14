use crate::Core;
use item_schema::{ImageCatalogKind, ItemSchema};
use std::sync::atomic::Ordering;
use std::time::Duration;

const ITEMS_GAME_URL: &str = "https://files.skinledger.com/counterstrike/items_game.txt";
const TRANSLATIONS_URL: &str = "https://files.skinledger.com/counterstrike/csgo_english.txt";
const MAX_SCHEMA_BYTES: usize = 32 * 1024 * 1024;
const SCHEMA_CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);
const IMAGE_CATALOG_URLS: [(&str, ImageCatalogKind, &str); 9] = [
    (
        "skins",
        ImageCatalogKind::Skins,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/skins.json",
    ),
    (
        "stickers",
        ImageCatalogKind::Stickers,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/stickers.json",
    ),
    (
        "patches",
        ImageCatalogKind::Stickers,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/patches.json",
    ),
    (
        "agents",
        ImageCatalogKind::Definitions,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/agents.json",
    ),
    (
        "collectibles",
        ImageCatalogKind::Definitions,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/collectibles.json",
    ),
    (
        "graffiti",
        ImageCatalogKind::Graffiti,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/graffiti.json",
    ),
    (
        "music kits",
        ImageCatalogKind::MusicKits,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/music_kits.json",
    ),
    (
        "crates",
        ImageCatalogKind::Definitions,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/crates.json",
    ),
    (
        "keys",
        ImageCatalogKind::Definitions,
        "https://raw.githubusercontent.com/ByMykel/CSGO-API/main/public/api/en/keys.json",
    ),
];

#[derive(Debug)]
pub struct SchemaRefreshReport {
    pub cache_used: bool,
    pub image_entries: usize,
    pub warnings: Vec<String>,
}

async fn fetch_text(client: &reqwest::Client, url: &str) -> Result<String, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| format!("Request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Server rejected the request: {error}"))?;
    let response_url = response.url();
    if response_url.scheme() != "https"
        || !matches!(
            response_url.host_str(),
            Some("files.skinledger.com" | "raw.githubusercontent.com")
        )
    {
        return Err("The schema source redirected to an untrusted host.".into());
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_SCHEMA_BYTES as u64)
    {
        return Err("The schema response exceeded the size limit.".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("Could not read the schema response: {error}"))?;
    if bytes.len() > MAX_SCHEMA_BYTES {
        return Err("The schema response exceeded the size limit.".into());
    }
    String::from_utf8(bytes.to_vec())
        .map_err(|error| format!("The schema response was not UTF-8: {error}"))
}

impl Core {
    pub async fn refresh_item_schema(&self) -> Result<SchemaRefreshReport, String> {
        if self.schema_cache_valid.load(Ordering::Relaxed)
            && std::fs::metadata(&*self.schema_cache_path)
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age < SCHEMA_CACHE_TTL)
        {
            return Ok(SchemaRefreshReport {
                cache_used: true,
                image_entries: 0,
                warnings: Vec::new(),
            });
        }

        let client = reqwest::Client::builder()
            .user_agent(concat!("Casemove/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(8))
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .map_err(|error| format!("Could not create the schema HTTP client: {error}"))?;

        let (items_game, translations) = tokio::try_join!(
            fetch_text(&client, ITEMS_GAME_URL),
            fetch_text(&client, TRANSLATIONS_URL)
        )?;
        let mut schema = ItemSchema::from_vdf(&items_game, &translations)?;
        let previous = self.schema_snapshot()?;

        let (skins, stickers, patches, agents, collectibles, graffiti, music_kits, crates, keys) = tokio::join!(
            fetch_text(&client, IMAGE_CATALOG_URLS[0].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[1].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[2].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[3].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[4].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[5].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[6].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[7].2),
            fetch_text(&client, IMAGE_CATALOG_URLS[8].2),
        );
        let catalogs = [
            skins,
            stickers,
            patches,
            agents,
            collectibles,
            graffiti,
            music_kits,
            crates,
            keys,
        ];
        let mut image_entries = 0;
        let mut warnings = Vec::new();
        for ((name, kind, _), result) in IMAGE_CATALOG_URLS.into_iter().zip(catalogs) {
            match result {
                Ok(json) => match schema.add_image_catalog(kind, &json) {
                    Ok(count) => image_entries += count,
                    Err(error) => {
                        warnings.push(format!("Could not use the {name} image catalog: {error}"))
                    }
                },
                Err(error) => warnings.push(format!(
                    "Could not refresh the {name} image catalog: {error}"
                )),
            }
        }

        schema.merge_missing_images(&previous);

        let cache_saved = match schema.save_cache(&self.schema_cache_path) {
            Ok(()) => true,
            Err(error) => {
                warnings.push(error);
                false
            }
        };
        *self
            .schema
            .write()
            .map_err(|_| "The item schema is unavailable.".to_owned())? =
            std::sync::Arc::new(schema);
        self.schema_cache_valid
            .store(cache_saved, Ordering::Relaxed);

        Ok(SchemaRefreshReport {
            cache_used: false,
            image_entries,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires public Internet access"]
    fn live_schema_and_image_catalogs_parse() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let client = reqwest::Client::builder()
                    .connect_timeout(Duration::from_secs(8))
                    .timeout(Duration::from_secs(30))
                    .build()
                    .unwrap();
                let (items_game, translations) = tokio::try_join!(
                    fetch_text(&client, ITEMS_GAME_URL),
                    fetch_text(&client, TRANSLATIONS_URL)
                )
                .unwrap();
                let mut schema = ItemSchema::from_vdf(&items_game, &translations).unwrap();
                assert_eq!(schema.describe(7, None).name, "AK-47");
                assert_eq!(schema.describe(7, Some(44)).name, "AK-47 | Case Hardened");
                let (skins, stickers) = tokio::join!(
                    fetch_text(&client, IMAGE_CATALOG_URLS[0].2),
                    fetch_text(&client, IMAGE_CATALOG_URLS[1].2)
                );
                assert!(
                    schema
                        .add_image_catalog(ImageCatalogKind::Skins, &skins.unwrap())
                        .unwrap()
                        > 100
                );
                assert!(
                    schema
                        .describe(7, Some(44))
                        .image_url
                        .is_some_and(|url| url.starts_with("https://"))
                );
                let stickers = stickers.unwrap();
                assert!(
                    schema
                        .add_image_catalog(ImageCatalogKind::Stickers, &stickers)
                        .unwrap()
                        > 100
                );
                let sticker_catalog: serde_json::Value = serde_json::from_str(&stickers).unwrap();
                assert!(sticker_catalog.as_array().unwrap().iter().any(|entry| {
                    entry
                        .get("def_index")
                        .and_then(|value| {
                            value
                                .as_u64()
                                .and_then(|id| u32::try_from(id).ok())
                                .or_else(|| value.as_str()?.parse().ok())
                        })
                        .and_then(|id| schema.sticker(id))
                        .is_some_and(|sticker| sticker.image_url.is_some())
                }));
                for index in [5, 6, 7] {
                    let (_, kind, url) = IMAGE_CATALOG_URLS[index];
                    let json = fetch_text(&client, url).await.unwrap();
                    assert!(schema.add_image_catalog(kind, &json).unwrap() > 100);
                }
                assert_eq!(schema.describe(4818, None).name, "Dreams & Nightmares Case");
                assert!(schema.describe(4818, None).image_url.is_some());
                assert!(schema.graffiti_image(1653, None).is_some());
                let red = schema.graffiti_image(1697, Some(1)).unwrap();
                let other = schema.graffiti_image(1697, Some(2)).unwrap();
                assert_ne!(red, other);
                assert_eq!(schema.describe(7, None).name, "AK-47");
                assert!(schema.describe(7, None).image_url.is_none());
            });
    }
}
