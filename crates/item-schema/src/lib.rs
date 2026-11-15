//! Item schema: items_game/csgo_english loading and inventory conversion.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

const SCHEMA_CACHE_VERSION: u32 = 3;

pub struct ItemSchema {
    schema: Value,
    translations: HashMap<String, String>,
    image_urls: HashMap<String, String>,
    item_image_urls: HashMap<u32, String>,
    skin_image_urls: HashMap<String, String>,
    sticker_image_urls: HashMap<u32, String>,
    graffiti_image_urls: HashMap<String, String>,
    collections: HashMap<String, CollectionMetadata>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CollectionMetadata {
    pub name: String,
    pub image_url: Option<String>,
    pub trade_up: Option<bool>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StickerDescription {
    pub name: String,
    pub kind: String,
    pub image_path: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MusicKitDescription {
    pub name: String,
    pub image_path: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, PartialEq)]
pub struct ItemDescription {
    pub name: String,
    pub image_path: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageCatalogKind {
    Skins,
    Stickers,
    Graffiti,
    MusicKits,
    Definitions,
}

#[derive(Serialize, Deserialize)]
struct SchemaCache {
    version: u32,
    schema: Value,
    translations: HashMap<String, String>,
    image_urls: HashMap<String, String>,
    item_image_urls: HashMap<u32, String>,
    skin_image_urls: HashMap<String, String>,
    sticker_image_urls: HashMap<u32, String>,
    #[serde(default)]
    graffiti_image_urls: HashMap<String, String>,
    collections: HashMap<String, CollectionMetadata>,
}

impl ItemSchema {
    pub fn bundled() -> Result<Self, serde_json::Error> {
        Self::from_json_with_collections(
            include_str!(
                "../../../data/items_game.json"
            ),
            include_str!(
                "../../../data/csgo_english.json"
            ),
            include_str!("../../../data/collections.json"),
        )
    }

    fn from_json(schema: &str, translations: &str) -> Result<Self, serde_json::Error> {
        Ok(Self {
            schema: serde_json::from_str(schema)?,
            translations: serde_json::from_str(translations)?,
            image_urls: HashMap::new(),
            item_image_urls: HashMap::new(),
            skin_image_urls: HashMap::new(),
            sticker_image_urls: HashMap::new(),
            graffiti_image_urls: HashMap::new(),
            collections: HashMap::new(),
        })
    }

    pub fn from_json_with_collections(
        schema: &str,
        translations: &str,
        collections: &str,
    ) -> Result<Self, serde_json::Error> {
        let mut item_schema = Self::from_json(schema, translations)?;
        let collections: HashMap<String, HashMap<String, Value>> =
            serde_json::from_str(collections)?;
        item_schema.add_collections(collections);
        Ok(item_schema)
    }

    fn add_collections(&mut self, collections: HashMap<String, HashMap<String, Value>>) {
        for (collection_name, entries) in collections {
            for (item_name, entry) in entries {
                let image_url =
                    Self::secure_image_url(entry.get("imageURL").and_then(Value::as_str));
                if let Some(url) = image_url.as_ref() {
                    self.image_urls.insert(item_name.clone(), url.clone());
                }
                self.collections.insert(
                    item_name,
                    CollectionMetadata {
                        name: collection_name.clone(),
                        image_url,
                        trade_up: entry.get("trade_up").and_then(Value::as_bool),
                    },
                );
            }
        }
    }

    /// Loads a previously validated live schema cache. A missing file is not
    /// an error; an invalid cache is reported so the caller can use bundled data.
    pub fn load_cache(path: &Path) -> Result<Option<Self>, String> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("Could not read item schema cache: {error}")),
        };
        let cache: SchemaCache = serde_json::from_slice(&bytes)
            .map_err(|error| format!("Could not parse item schema cache: {error}"))?;
        if cache.version != SCHEMA_CACHE_VERSION
            || !cache
                .schema
                .get("items")
                .and_then(Value::as_object)
                .is_some_and(|items| !items.is_empty())
        {
            return Err("The item schema cache is incomplete or unsupported.".into());
        }
        let mut schema = Self {
            schema: cache.schema,
            translations: cache.translations,
            image_urls: cache.image_urls,
            item_image_urls: cache.item_image_urls,
            skin_image_urls: cache.skin_image_urls,
            sticker_image_urls: cache.sticker_image_urls,
            graffiti_image_urls: cache.graffiti_image_urls,
            collections: cache.collections,
        };
        schema.sanitize_image_urls();
        Ok(Some(schema))
    }

    /// Writes cache data to a sibling temporary file before replacing the
    /// previous cache, so interrupted downloads never become the active cache.
    pub fn save_cache(&self, path: &Path) -> Result<(), String> {
        let cache = SchemaCache {
            version: SCHEMA_CACHE_VERSION,
            schema: self.schema.clone(),
            translations: self.translations.clone(),
            image_urls: self.image_urls.clone(),
            item_image_urls: self.item_image_urls.clone(),
            skin_image_urls: self.skin_image_urls.clone(),
            sticker_image_urls: self.sticker_image_urls.clone(),
            graffiti_image_urls: self.graffiti_image_urls.clone(),
            collections: self.collections.clone(),
        };
        let bytes = serde_json::to_vec(&cache)
            .map_err(|error| format!("Could not serialize item schema cache: {error}"))?;
        let parent = path
            .parent()
            .ok_or_else(|| "Item schema cache path has no parent directory.".to_owned())?;
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create item schema cache directory: {error}"))?;
        let temporary = path.with_extension("json.tmp");
        std::fs::write(&temporary, bytes)
            .map_err(|error| format!("Could not write item schema cache: {error}"))?;
        if path.exists() {
            std::fs::remove_file(path)
                .map_err(|error| format!("Could not replace item schema cache: {error}"))?;
        }
        std::fs::rename(&temporary, path)
            .map_err(|error| format!("Could not replace item schema cache: {error}"))
    }

    /// Builds a schema from the same public VDF sources used by the legacy app.
    pub fn from_vdf(items_game: &str, csgo_english: &str) -> Result<Self, String> {
        let items_game = parse_vdf(items_game)
            .map_err(|error| format!("Could not parse items_game VDF: {error}"))?;
        let items_game = items_game
            .get("items_game")
            .ok_or_else(|| "Live item schema has no items_game section.".to_owned())?;
        let mut schema = serde_json::Map::new();
        for (source, target) in [
            ("items", "items"),
            ("paint_kits", "paint_kits"),
            ("prefabs", "prefabs"),
            ("sticker_kits", "sticker_kits"),
            ("music_definitions", "music_kits"),
            ("graffiti_tints", "graffiti_tints"),
        ] {
            if let Some(value) = items_game.get(source) {
                schema.insert(target.to_owned(), value.clone());
            }
        }
        if let Some(casket_icons) = items_game
            .get("alternate_icons2")
            .and_then(|value| value.get("casket_icons"))
        {
            schema.insert("casket_icons".to_owned(), casket_icons.clone());
        }
        if !schema
            .get("items")
            .and_then(Value::as_object)
            .is_some_and(|items| !items.is_empty())
        {
            return Err("Live item schema contains no item definitions.".into());
        }
        let translations = parse_vdf(csgo_english)
            .map_err(|error| format!("Could not parse csgo_english VDF: {error}"))?;
        let tokens = translations
            .get("lang")
            .and_then(|lang| lang.get("Tokens"))
            .and_then(Value::as_object)
            .ok_or_else(|| "Live item translations have no Tokens section.".to_owned())?;
        let translations: HashMap<String, String> = tokens
            .iter()
            .filter_map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.to_lowercase(), value.trim_matches('"').to_owned()))
            })
            .collect();
        if translations.is_empty() {
            return Err("Live item translations are empty.".into());
        }

        let mut item_schema = Self {
            schema: Value::Object(schema),
            translations,
            image_urls: HashMap::new(),
            item_image_urls: HashMap::new(),
            skin_image_urls: HashMap::new(),
            sticker_image_urls: HashMap::new(),
            graffiti_image_urls: HashMap::new(),
            collections: HashMap::new(),
        };
        let collections = serde_json::from_str(include_str!(
            "../../../data/collections.json"
        ))
        .map_err(|error| format!("Could not parse bundled collections: {error}"))?;
        item_schema.add_collections(collections);
        Ok(item_schema)
    }

    /// Adds one live JSON image catalog. Image URLs are kept only for the
    /// explicitly allowlisted Steam CDN hosts.
    pub fn add_image_catalog(
        &mut self,
        kind: ImageCatalogKind,
        json: &str,
    ) -> Result<usize, String> {
        let entries: Value = serde_json::from_str(json)
            .map_err(|error| format!("Could not parse item image catalog: {error}"))?;
        let entries = entries
            .as_array()
            .ok_or_else(|| "Item image catalog must be a JSON array.".to_owned())?;
        let mut added = 0;
        for entry in entries {
            let Some(image_url) =
                Self::secure_image_url(entry.get("image").and_then(Value::as_str))
            else {
                continue;
            };
            match kind {
                ImageCatalogKind::Skins => {
                    let Some(definition_index) = entry
                        .get("weapon")
                        .and_then(|weapon| weapon.get("weapon_id"))
                        .and_then(Self::value_u32)
                    else {
                        continue;
                    };
                    let Some(paint_index) = entry.get("paint_index").and_then(Self::value_u32)
                    else {
                        continue;
                    };
                    self.skin_image_urls
                        .insert(format!("{definition_index}:{paint_index}"), image_url);
                    added += 1;
                }
                ImageCatalogKind::Stickers => {
                    let Some(definition_index) = entry.get("def_index").and_then(Self::value_u32)
                    else {
                        continue;
                    };
                    self.sticker_image_urls.insert(definition_index, image_url);
                    added += 1;
                }
                ImageCatalogKind::Graffiti => {
                    let Some(kit_id) = entry.get("def_index").and_then(Self::value_u32) else {
                        continue;
                    };
                    let tint = entry
                        .get("color_index")
                        .and_then(Self::value_u32)
                        .unwrap_or(0);
                    self.graffiti_image_urls
                        .insert(format!("{kit_id}:{tint}"), image_url);
                    added += 1;
                }
                ImageCatalogKind::MusicKits => {
                    let Some(name) = entry.get("name").and_then(Value::as_str) else {
                        continue;
                    };
                    self.image_urls
                        .insert(format!("Music Kit | {name}"), image_url);
                    added += 1;
                }
                ImageCatalogKind::Definitions => {
                    let Some(definition_index) = entry.get("def_index").and_then(Self::value_u32)
                    else {
                        continue;
                    };
                    self.item_image_urls.insert(definition_index, image_url);
                    added += 1;
                }
            }
        }
        if added == 0 {
            return Err("Item image catalog contains no usable Steam image URLs.".into());
        }
        Ok(added)
    }

    /// Carries forward still-valid images when an optional live catalog is
    /// unavailable, without overwriting newer entries.
    pub fn merge_missing_images(&mut self, previous: &Self) {
        for (key, value) in &previous.image_urls {
            self.image_urls
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &previous.item_image_urls {
            self.item_image_urls
                .entry(*key)
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &previous.skin_image_urls {
            self.skin_image_urls
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &previous.sticker_image_urls {
            self.sticker_image_urls
                .entry(*key)
                .or_insert_with(|| value.clone());
        }
        for (key, value) in &previous.graffiti_image_urls {
            self.graffiti_image_urls
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
    }

    fn value_u32(value: &Value) -> Option<u32> {
        value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .or_else(|| value.as_str()?.parse().ok())
    }

    fn sanitize_image_urls(&mut self) {
        self.image_urls
            .retain(|_, url| Self::secure_image_url(Some(url)).is_some());
        self.item_image_urls
            .retain(|_, url| Self::secure_image_url(Some(url)).is_some());
        self.skin_image_urls
            .retain(|_, url| Self::secure_image_url(Some(url)).is_some());
        self.sticker_image_urls
            .retain(|_, url| Self::secure_image_url(Some(url)).is_some());
        self.graffiti_image_urls
            .retain(|_, url| Self::secure_image_url(Some(url)).is_some());
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
        let image_url = if let Some(paint_id) = paint_kit.filter(|paint| *paint != 0) {
            self.skin_image_urls
                .get(&format!("{definition_index}:{paint_id}"))
                .cloned()
                .or_else(|| self.image_urls.get(&name).cloned())
        } else {
            self.item_image_urls
                .get(&definition_index)
                .cloned()
                .or_else(|| self.image_urls.get(&name).cloned())
        };
        ItemDescription {
            name,
            image_path,
            image_url,
        }
    }

    pub fn collection(&self, item_name: &str) -> Option<&CollectionMetadata> {
        self.collections.get(item_name)
    }

    pub fn sticker(&self, sticker_id: u32) -> Option<StickerDescription> {
        let sticker = self
            .schema
            .get("sticker_kits")?
            .get(sticker_id.to_string())?;
        let token = sticker.get("item_name").and_then(Value::as_str)?;
        let name = self.localize(token).or_else(|| {
            sticker
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })?;
        let (kind, material) =
            if let Some(material) = sticker.get("patch_material").and_then(Value::as_str) {
                ("Patch", format!("econ/patches/{material}"))
            } else if let Some(material) = sticker.get("sticker_material").and_then(Value::as_str) {
                ("Sticker", format!("econ/stickers/{material}"))
            } else {
                ("Sticker", String::new())
            };
        Some(StickerDescription {
            name,
            kind: kind.to_owned(),
            image_path: (!material.is_empty()).then_some(material),
            image_url: self.sticker_image_urls.get(&sticker_id).cloned(),
        })
    }

    pub fn music_kit(&self, music_id: u32) -> Option<MusicKitDescription> {
        let kit = self.schema.get("music_kits")?.get(music_id.to_string())?;
        let token = kit.get("loc_name").and_then(Value::as_str)?;
        Some(MusicKitDescription {
            name: format!("Music Kit | {}", self.localize(token)?),
            image_path: kit
                .get("image_inventory")
                .and_then(Value::as_str)
                .map(str::to_owned),
            image_url: self
                .image_urls
                .get(&format!("Music Kit | {}", self.localize(token)?))
                .cloned(),
        })
    }

    pub fn graffiti_image(&self, kit_id: u32, tint_id: Option<u32>) -> Option<String> {
        self.graffiti_image_urls
            .get(&format!("{kit_id}:{}", tint_id.unwrap_or(0)))
            .cloned()
    }

    pub fn graffiti_name(&self, tint_id: u32) -> Option<String> {
        let tints = self.schema.get("graffiti_tints")?.as_object()?;
        let (name, _) = tints
            .iter()
            .find(|(_, value)| value.get("id").and_then(Self::value_u32) == Some(tint_id))?;
        Some(
            name.replace('_', " ")
                .split_whitespace()
                .map(Self::capitalize_word)
                .collect::<Vec<_>>()
                .join(" "),
        )
    }

    pub fn casket_icon(&self, icon_id: u32) -> Option<String> {
        self.schema
            .get("casket_icons")?
            .get(icon_id.to_string())?
            .get("icon_path")?
            .as_str()
            .map(str::to_owned)
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

    fn secure_image_url(value: Option<&str>) -> Option<String> {
        let mut url = url::Url::parse(value?).ok()?;
        if !url.username().is_empty() || url.password().is_some() || url.port().is_some() {
            return None;
        }
        match url.host_str()? {
            "media.steampowered.com" | "cdn.steamstatic.com" | "community.steamstatic.com" => {}
            "community.akamai.steamstatic.com" => {
                url.set_host(Some("community.steamstatic.com")).ok()?;
            }
            _ => return None,
        }
        url.set_scheme("https").ok()?;
        Some(url.to_string())
    }

    fn capitalize_word(value: &str) -> String {
        let mut chars = value.chars();
        chars
            .next()
            .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
            .unwrap_or_default()
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
}

fn parse_vdf(text: &str) -> Result<Value, String> {
    const ESCAPED_QUOTE: char = '\u{e000}';

    fn normalize(text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        let mut normalized = String::with_capacity(text.len());
        let mut index = 0;
        while index < chars.len() {
            if chars[index] == '\\' {
                match chars.get(index + 1) {
                    Some('\\') => {
                        normalized.push('\\');
                        normalized.push('\\');
                        index += 2;
                        continue;
                    }
                    Some('"') => {
                        normalized.push(ESCAPED_QUOTE);
                        index += 2;
                        continue;
                    }
                    Some('x') if index + 6 <= chars.len() => {
                        let hex: String = chars[index + 2..index + 6].iter().collect();
                        if let Ok(codepoint) = u32::from_str_radix(&hex, 16)
                            && let Some(character) = char::from_u32(codepoint)
                        {
                            normalized.push(character);
                            index += 6;
                            continue;
                        }
                    }
                    _ => {}
                }
            }
            normalized.push(chars[index]);
            index += 1;
        }
        normalized
    }

    fn unescape(value: String) -> String {
        let mut output = String::with_capacity(value.len());
        let mut chars = value.chars();
        while let Some(character) = chars.next() {
            if character == ESCAPED_QUOTE {
                output.push('"');
                continue;
            }
            if character != '\\' {
                output.push(character);
                continue;
            }
            match chars.next() {
                Some('n') => output.push('\n'),
                Some('r') => output.push('\r'),
                Some('t') => output.push('\t'),
                Some('\\') => output.push('\\'),
                Some(other) => {
                    output.push('\\');
                    output.push(other);
                }
                None => output.push('\\'),
            }
        }
        output
    }

    fn convert(value: keyvalues_parser::Value<'_>) -> Value {
        match value {
            keyvalues_parser::Value::Str(value) => Value::String(unescape(value.into_owned())),
            keyvalues_parser::Value::Obj(object) => {
                let mut converted = serde_json::Map::new();
                for (key, values) in object.into_inner() {
                    let mut merged = None;
                    for value in values {
                        let value = convert(value);
                        if let Some(existing) = &mut merged {
                            merge_values(existing, value);
                        } else {
                            merged = Some(value);
                        }
                    }
                    if let Some(value) = merged {
                        converted.insert(unescape(key.into_owned()), value);
                    }
                }
                Value::Object(converted)
            }
        }
    }

    fn merge_values(existing: &mut Value, incoming: Value) {
        if let (Some(existing), Some(incoming)) = (existing.as_object_mut(), incoming.as_object()) {
            for (key, value) in incoming {
                if let Some(existing) = existing.get_mut(key) {
                    merge_values(existing, value.clone());
                } else {
                    existing.insert(key.clone(), value.clone());
                }
            }
        } else {
            *existing = incoming;
        }
    }

    let text = text.trim_start_matches('\u{feff}').trim_start();
    let text = normalize(text);
    let parsed = keyvalues_parser::Parser::new()
        .literal_special_chars(true)
        .parse(&text)
        .map_err(|error| format!("Could not parse live item schema VDF: {error}"))?;
    let parsed = parsed.into_vdf();
    Ok(Value::Object(
        [(unescape(parsed.key.into_owned()), convert(parsed.value))]
            .into_iter()
            .collect(),
    ))
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
    fn parses_hex_codes_without_changing_standard_vdf_escapes() {
        let parsed =
            parse_vdf(r#""root" { "entry" "A \"quoted\" word\nnext \x00A2 unknown \S" }"#).unwrap();
        assert_eq!(
            parsed["root"]["entry"],
            "A \"quoted\" word\nnext ¢ unknown \\S"
        );
    }

    #[test]
    fn merges_repeated_vdf_sections_and_item_definitions() {
        let parsed = parse_vdf(
            r##""root" {
                "items" { "7" { "name" "weapon_ak47" "rarity" "3" } }
                "items" { "8" { "name" "weapon_awp" } "7" { "item_name" "#AK" } }
            }"##,
        )
        .unwrap();
        assert_eq!(parsed["root"]["items"].as_object().unwrap().len(), 2);
        assert_eq!(parsed["root"]["items"]["7"]["name"], "weapon_ak47");
        assert_eq!(parsed["root"]["items"]["7"]["item_name"], "#AK");
        assert_eq!(parsed["root"]["items"]["7"]["rarity"], "3");
    }

    #[test]
    fn resolves_collection_sticker_music_graffiti_and_casket_metadata() {
        let schema = ItemSchema::from_json_with_collections(
            r##"{
                "items":{"7":{"name":"weapon_ak47","item_name":"#AK"}},
                "sticker_kits":{
                    "12":{"item_name":"#PATCH","patch_material":"test_patch"},
                    "13":{"item_name":"#STICKER","sticker_material":"test_sticker"}
                },
                "music_kits":{"4":{"loc_name":"#MUSIC","image_inventory":"econ/music/test"}},
                "graffiti_tints":{"bright_blue":{"id":9}},
                "casket_icons":{"3":{"icon_path":"econ/tools/casket_test"}}
            }"##,
            r##"{"ak":"\"AK-47\"","patch":"\"Test Patch\"","sticker":"\"Test Sticker\"","music":"\"Test Music\""}"##,
            r##"{
                "The Test Collection":{
                    "AK-47 | Redline":{
                        "trade_up":true,
                        "imageURL":"http://media.steampowered.com/apps/730/test.png"
                    }
                }
            }"##,
        )
        .unwrap();
        assert_eq!(
            schema.collection("AK-47 | Redline"),
            Some(&CollectionMetadata {
                name: "The Test Collection".into(),
                image_url: Some("https://media.steampowered.com/apps/730/test.png".into()),
                trade_up: Some(true),
            })
        );
        assert_eq!(
            schema.sticker(12),
            Some(StickerDescription {
                name: "Test Patch".into(),
                kind: "Patch".into(),
                image_path: Some("econ/patches/test_patch".into()),
                image_url: None,
            })
        );
        assert_eq!(
            schema.sticker(13).unwrap().image_path.as_deref(),
            Some("econ/stickers/test_sticker")
        );
        assert_eq!(
            schema.music_kit(4),
            Some(MusicKitDescription {
                name: "Music Kit | Test Music".into(),
                image_path: Some("econ/music/test".into()),
                image_url: None,
            })
        );
        assert_eq!(schema.graffiti_name(9).as_deref(), Some("Bright Blue"));
        assert_eq!(
            schema.casket_icon(3).as_deref(),
            Some("econ/tools/casket_test")
        );
    }

    #[test]
    fn ignores_collection_image_urls_outside_the_steam_allowlist() {
        let schema = ItemSchema::from_json_with_collections(
            "{}",
            "{}",
            r#"{"Test":{"Item":{"imageURL":"https://example.com/image.png","trade_up":false}}}"#,
        )
        .unwrap();
        assert_eq!(
            schema.collection("Item"),
            Some(&CollectionMetadata {
                name: "Test".into(),
                image_url: None,
                trade_up: Some(false),
            })
        );
    }

    #[test]
    fn parses_live_vdf_and_uses_live_skin_and_sticker_images() {
        let items_game = r##"
            "items_game" {
                "items" {
                    "7" { "name" "weapon_ak47" "prefab" "rifle" }
                    "1201" { "name" "casket" "item_name" "#Storage" }
                }
                "prefabs" { "rifle" { "item_name" "#AK" } }
                "paint_kits" {
                    "44" { "name" "redline" "description_tag" "#Finish" }
                }
                "sticker_kits" {
                    "12" {
                        "item_name" "#STICKER"
                        "sticker_material" "standard/thirteen"
                    }
                }
                "music_definitions" {}
                "graffiti_tints" {}
                "alternate_icons2" {
                    "casket_icons" {
                        "3" { "icon_path" "econ/tools/casket_test" }
                    }
                }
            }
        "##;
        let translations = r##"
            "lang" {
                "Tokens" {
                    "AK" "AK-47"
                    "Finish" "Redline"
                    "STICKER" "Test Sticker"
                }
            }
        "##;
        let mut schema = ItemSchema::from_vdf(items_game, translations).unwrap();
        schema
            .add_image_catalog(
                ImageCatalogKind::Skins,
                r#"[{
                    "weapon":{"weapon_id":7},
                    "paint_index":"44",
                    "image":"https://community.akamai.steamstatic.com/economy/image/skin"
                }]"#,
            )
            .unwrap();
        schema
            .add_image_catalog(
                ImageCatalogKind::Stickers,
                r#"[{
                    "def_index":"12",
                    "image":"https://cdn.steamstatic.com/apps/730/icons/econ/stickers/test.png"
                }]"#,
            )
            .unwrap();

        let item = schema.describe(7, Some(44));
        assert_eq!(item.name, "AK-47 | Redline");
        assert_eq!(
            item.image_url.as_deref(),
            Some("https://community.steamstatic.com/economy/image/skin")
        );
        assert_eq!(
            schema.sticker(12).unwrap().image_url.as_deref(),
            Some("https://cdn.steamstatic.com/apps/730/icons/econ/stickers/test.png")
        );
        assert_eq!(
            schema.casket_icon(3).as_deref(),
            Some("econ/tools/casket_test")
        );
    }

    #[test]
    fn persists_live_schema_cache_and_rejects_untrusted_image_hosts() {
        let mut schema =
            ItemSchema::from_json(r#"{"items":{"7":{"name":"weapon_ak47"}}}"#, "{}").unwrap();
        assert!(
            schema
                .add_image_catalog(
                    ImageCatalogKind::Definitions,
                    r#"[{"def_index":"7","image":"https://example.com/image.png"}]"#
                )
                .is_err()
        );
        schema
            .add_image_catalog(
                ImageCatalogKind::Definitions,
                r#"[{"def_index":"7","image":"https://cdn.steamstatic.com/apps/730/icons/item.png"}]"#,
            )
            .unwrap();
        schema
            .add_image_catalog(
                ImageCatalogKind::Graffiti,
                r#"[{"def_index":"7","color_index":1,"image":"https://cdn.steamstatic.com/red.png"}]"#,
            )
            .unwrap();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("casemove-schema-{unique}.json"));
        schema.save_cache(&path).unwrap();
        let restored = ItemSchema::load_cache(&path).unwrap().unwrap();
        let mut old_cache: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        old_cache["version"] = Value::from(2);
        std::fs::write(&path, serde_json::to_vec(&old_cache).unwrap()).unwrap();
        assert!(ItemSchema::load_cache(&path).is_err());
        let _ = std::fs::remove_file(path);
        assert_eq!(
            restored.describe(7, None).image_url.as_deref(),
            Some("https://cdn.steamstatic.com/apps/730/icons/item.png")
        );
        assert_eq!(
            restored.graffiti_image(7, Some(1)).as_deref(),
            Some("https://cdn.steamstatic.com/red.png")
        );
    }
}
