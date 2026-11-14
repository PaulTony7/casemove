//! Item schema: items_game/csgo_english loading and inventory conversion.

mod vdf;

use serde_json::Value;
use std::collections::HashMap;

pub struct ItemSchema {
    schema: Value,
    translations: HashMap<String, String>,
    image_urls: HashMap<String, String>,
}

#[derive(Debug, PartialEq)]
pub struct ItemDescription {
    pub name: String,
    pub image_path: Option<String>,
    pub image_url: Option<String>,
}

impl ItemSchema {
    pub fn bundled() -> Result<Self, serde_json::Error> {
        let mut schema = Self::from_json(
            include_str!(
                "../../../legacy/src/main/helpers/classes/steam/items/itemsBackupFiles/items_game.json"
            ),
            include_str!(
                "../../../legacy/src/main/helpers/classes/steam/items/itemsBackupFiles/csgo_english.json"
            ),
        )?;
        schema.image_urls = collection_images()?;
        Ok(schema)
    }

    /// The live skinledger dump: the same section layout as the bundled
    /// JSON, extracted exactly like legacy's `updateItems`. Any failure
    /// leaves the caller on the bundled fallback.
    pub fn from_vdf(items_vdf: &str, translations_vdf: &str) -> Result<Self, String> {
        let root = vdf::parse(items_vdf, true)?;
        let items_game = root
            .get("items_game")
            .ok_or_else(|| "the live schema has no items_game section".to_owned())?;
        let section = |key: &str| items_game.get(key).cloned().unwrap_or(Value::Null);
        let casket_icons = items_game
            .get("alternate_icons2")
            .and_then(|icons| icons.get("casket_icons"))
            .cloned()
            .unwrap_or(Value::Null);
        let schema = Value::Object(
            [
                ("items", section("items")),
                ("paint_kits", section("paint_kits")),
                ("prefabs", section("prefabs")),
                ("sticker_kits", section("sticker_kits")),
                ("music_kits", section("music_definitions")),
                ("graffiti_tints", section("graffiti_tints")),
                ("casket_icons", casket_icons),
            ]
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
        );
        // Legacy validates the pull the same way (`returnValue['items'][1209]`).
        if schema.get("items").and_then(|items| items.get("7")).is_none() {
            return Err("the live schema is missing core item definitions".to_owned());
        }

        let english = vdf::parse(translations_vdf, false)?;
        let mut translations = HashMap::new();
        if let Some(tokens) = english.get("lang").and_then(|lang| lang.get("Tokens"))
            && let Some(tokens) = tokens.as_object()
        {
            for (key, value) in tokens {
                if let Some(text) = value.as_str() {
                    translations.insert(
                        key.trim_start_matches('#').to_lowercase(),
                        text.to_owned(),
                    );
                }
            }
        }
        if translations.is_empty() {
            return Err("the live translation file has no tokens".to_owned());
        }

        Ok(Self {
            schema,
            translations,
            image_urls: collection_images()
                .map_err(|error| format!("could not load the image catalog: {error}"))?,
        })
    }

    pub fn from_json(schema: &str, translations: &str) -> Result<Self, serde_json::Error> {
        Ok(Self {
            schema: serde_json::from_str(schema)?,
            translations: serde_json::from_str(translations)?,
            image_urls: HashMap::new(),
        })
    }

    fn property<'a>(&'a self, entry: &'a Value, key: &str, depth: usize) -> Option<&'a str> {
        if depth > 16 {
            return None;
        }
        if let Some(value) = entry.get(key).and_then(Value::as_str) {
            return Some(value);
        }
        for prefab in entry
            .get("prefab")
            .and_then(Value::as_str)?
            .split_whitespace()
        {
            if let Some(value) = self
                .schema
                .get("prefabs")
                .and_then(|prefabs| prefabs.get(prefab))
                .and_then(|parent| self.property(parent, key, depth + 1))
            {
                return Some(value);
            }
        }
        None
    }

    fn localize(&self, token: &str) -> Option<String> {
        self.translations
            .get(&token.trim_start_matches('#').to_lowercase())
            .map(|value| value.trim_matches('"').to_owned())
    }

    pub fn describe(&self, definition_index: u32, paint_kit: Option<u32>) -> ItemDescription {
        let Some(item) = self
            .schema
            .get("items")
            .and_then(|items| items.get(definition_index.to_string()))
        else {
            return ItemDescription {
                name: format!("Unknown item ({definition_index})"),
                image_path: None,
                image_url: None,
            };
        };
        let mut name = self
            .property(item, "item_name", 0)
            .and_then(|token| self.localize(token))
            .or_else(|| item.get("name").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| format!("Item {definition_index}"));
        let weapon = item.get("name").and_then(Value::as_str);
        let mut image_path = self
            .property(item, "image_inventory", 0)
            .map(str::to_owned)
            .or_else(|| {
                weapon
                    .filter(|name| name.starts_with("weapon_"))
                    .map(|name| format!("econ/weapons/base_weapons/{name}"))
            });
        if let Some(paint_id) = paint_kit.filter(|paint| *paint != 0) {
            let paint = self
                .schema
                .get("paint_kits")
                .and_then(|paints| paints.get(paint_id.to_string()));
            let finish = paint
                .and_then(|paint| paint.get("description_tag"))
                .and_then(Value::as_str)
                .and_then(|token| self.localize(token))
                .unwrap_or_else(|| format!("Paint kit {paint_id}"));
            name.push_str(" | ");
            name.push_str(&finish);
            if let Some((weapon, paint)) = weapon.zip(
                paint
                    .and_then(|paint| paint.get("name"))
                    .and_then(Value::as_str),
            ) {
                image_path = Some(format!(
                    "econ/default_generated/{weapon}_{paint}_light_large"
                ));
            }
        }
        let image_url = self.image_urls.get(&name).cloned();
        ItemDescription {
            name,
            image_path,
            image_url,
        }
    }

    /// Direct `prefab` field on the item definition (no prefab inheritance —
    /// legacy `itemProcessorCanBeMoved` reads it directly).
    pub fn prefab(&self, definition_index: u32) -> Option<&str> {
        self.schema
            .get("items")?
            .get(definition_index.to_string())?
            .get("prefab")
            .and_then(Value::as_str)
    }

    /// Direct schema attribute on the item definition, e.g.
    /// `attributes["cannot trade"]`.
    pub fn item_attribute(&self, definition_index: u32, key: &str) -> Option<&Value> {
        self.schema
            .get("items")?
            .get(definition_index.to_string())?
            .get("attributes")?
            .get(key)
    }

    /// Image path for a music kit definition (attribute 166), the legacy
    /// music-kit image override.
    pub fn music_image(&self, music_index: u32) -> Option<String> {
        self.schema
            .get("music_kits")?
            .get(music_index.to_string())?
            .get("image_inventory")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }

    /// Icon path for a storage-unit icon (attribute 70), the legacy casket
    /// image override.
    pub fn casket_icon(&self, icon_index: u32) -> Option<String> {
        self.schema
            .get("casket_icons")?
            .get(icon_index.to_string())?
            .get("icon_path")
            .and_then(Value::as_str)
            .map(str::to_owned)
    }
}

/// Bundled collection thumbnails by item name (legacy `collections.json`),
/// upgraded to HTTPS. Every known image host is accepted — the file ships
/// with the app.
fn collection_images() -> Result<HashMap<String, String>, serde_json::Error> {
    let collections: HashMap<String, HashMap<String, Value>> = serde_json::from_str(include_str!(
        "../../../legacy/src/main/helpers/classes/steam/backup/collections.json"
    ))?;
    let mut image_urls = HashMap::new();
    for collection in collections.into_values() {
        for (name, entry) in collection {
            if let Some(mut url) = entry
                .get("imageURL")
                .and_then(Value::as_str)
                .and_then(|value| url::Url::parse(value).ok())
                && matches!(url.scheme(), "http" | "https")
                && url.set_scheme("https").is_ok()
            {
                image_urls.insert(name, url.to_string());
            }
        }
    }
    Ok(image_urls)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_prefabs_translation_and_painted_weapon_images() {
        let schema = ItemSchema::from_json(
            r##"{
			"items":{"7":{"name":"weapon_ak47","prefab":"rifle"}},
			"prefabs":{"rifle":{"prefab":"weapon"},"weapon":{"item_name":"#AK"}},
			"paint_kits":{"44":{"name":"test_paint","description_tag":"#Finish"}}
		}"##,
            r#"{"ak":"\"AK-47\"","finish":"\"Test Finish\""}"#,
        )
        .unwrap();
        let item = schema.describe(7, Some(44));
        assert_eq!(item.name, "AK-47 | Test Finish");
        assert_eq!(
            item.image_path.as_deref(),
            Some("econ/default_generated/weapon_ak47_test_paint_light_large")
        );
        assert_eq!(schema.describe(7, None).name, "AK-47");
    }

    #[test]
    fn handles_unknown_definitions_paints_and_cyclic_prefabs() {
        let schema = ItemSchema::from_json(
            r#"{"items":{"1":{"prefab":"cycle"}},"prefabs":{"cycle":{"prefab":"cycle"}}}"#,
            "{}",
        )
        .unwrap();
        assert_eq!(schema.describe(123456, None).name, "Unknown item (123456)");
        assert_eq!(schema.describe(1, Some(999)).name, "Item 1 | Paint kit 999");
    }

    #[test]
    fn bundled_schema_contains_legacy_rifle_and_storage_unit_names() {
        let schema = ItemSchema::bundled().unwrap();
        assert_eq!(schema.describe(7, None).name, "AK-47");
        assert_eq!(schema.describe(1201, None).name, "Storage Unit");
        assert!(schema.describe(7, None).image_url.is_none());
        let paint = schema.schema["paint_kits"]
            .as_object()
            .unwrap()
            .iter()
            .find(|(_, value)| value["name"] == "hy_mesh_safetyorange")
            .unwrap()
            .0
            .parse()
            .unwrap();
        let painted = schema.describe(7, Some(paint));
        assert_eq!(painted.name, "AK-47 | Safety Net");
        assert!(
            painted
                .image_url
                .unwrap()
                .ends_with(".f4ee68bc3bbd972ded65ec36b66259199b7fafb7.png")
        );
    }

    #[test]
    fn live_vdf_schema_matches_the_bundled_json_shape() {
        let live = ItemSchema::from_vdf(
            r##"
            "items_game"
            {
                "items"
                {
                    "7" { "name" "weapon_ak47" "prefab" "rifle" "baseitem" "1" }
                    "1" { "attributes" { "cannot trade" "1" } }
                }
                "prefabs"
                {
                    "rifle" { "prefab" "weapon" "item_name" "#AK" }
                    "weapon" { "item_name" "#AK" }
                }
                "paint_kits" { "44" { "name" "test_paint" "description_tag" "#Finish" } }
                "music_definitions" { "3" { "image_inventory" "econ/music_kits/noisia_01" } }
                "alternate_icons2" { "casket_icons" { "5" { "icon_path" "econ/tools/casket" } } }
            }"##,
            r##""lang" { "Tokens" { "#AK" "AK-47" "#Finish" "Test Finish" } }"##,
        )
        .unwrap();

        let bundled_shape = ItemSchema::from_json(
            r##"{
			"items":{"7":{"name":"weapon_ak47","prefab":"rifle","baseitem":1},"1":{"attributes":{"cannot trade":1}}},
			"prefabs":{"rifle":{"prefab":"weapon","item_name":"#AK"},"weapon":{"item_name":"#AK"}},
			"paint_kits":{"44":{"name":"test_paint","description_tag":"#Finish"}}
			}"##,
            r#"{"ak":"\"AK-47\"","finish":"\"Test Finish\""}"#,
        )
        .unwrap();

        assert_eq!(live.describe(7, Some(44)), bundled_shape.describe(7, Some(44)));
        assert_eq!(live.describe(7, Some(44)).name, "AK-47 | Test Finish");
        assert_eq!(
            live.describe(7, None).image_path.as_deref(),
            Some("econ/weapons/base_weapons/weapon_ak47")
        );

        // Numeric coercion keeps the `u64` reads working (compute_moveable).
        assert_eq!(
            live.item_attribute(1, "cannot trade")
                .and_then(Value::as_u64),
            Some(1)
        );

        // Legacy image overrides come from the extracted sections.
        assert_eq!(
            live.music_image(3).as_deref(),
            Some("econ/music_kits/noisia_01")
        );
        assert_eq!(live.casket_icon(5).as_deref(), Some("econ/tools/casket"));
        assert_eq!(live.music_image(999), None);
        assert_eq!(live.casket_icon(999), None);
    }

    #[test]
    fn live_schema_without_core_data_is_rejected_for_the_bundled_fallback() {
        assert!(
            ItemSchema::from_vdf(
                r#""items_game" { "items" { } }"#,
                r##""lang" { "Tokens" { "#A" "A" } }"##
            )
            .is_err()
        );
        assert!(
            ItemSchema::from_vdf(
                r#""items_game" { "items" { "7" { "name" "weapon_ak47" } } }"#,
                r#""lang" { "Tokens" { } }"#
            )
            .is_err()
        );
        assert!(
            ItemSchema::from_vdf("not vdf", r##""lang" { "Tokens" { "#A" "A" } }"##).is_err()
        );
    }
}
