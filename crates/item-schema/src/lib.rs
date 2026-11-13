//! Item schema: items_game/csgo_english loading and inventory conversion.

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
        let collections: HashMap<String, HashMap<String, Value>> = serde_json::from_str(
            include_str!("../../../legacy/src/main/helpers/classes/steam/backup/collections.json"),
        )?;
        for collection in collections.into_values() {
            for (name, entry) in collection {
                if let Some(mut url) = entry
                    .get("imageURL")
                    .and_then(Value::as_str)
                    .and_then(|value| url::Url::parse(value).ok())
                    && url.host_str() == Some("media.steampowered.com")
                    && url.set_scheme("https").is_ok()
                {
                    schema.image_urls.insert(name, url.to_string());
                }
            }
        }
        Ok(schema)
    }

    fn from_json(schema: &str, translations: &str) -> Result<Self, serde_json::Error> {
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
}
