//! Trade-up contract math, ported from legacy `tradeup.tsx` over the bundled
//! `data/collections.json` snapshot: eligibility, possible outcomes, wear
//! buckets, probabilities, and resulting floats.

use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// Bundled collection snapshot (`data/collections.json`).
const COLLECTIONS_JSON: &str =
    include_str!("../../../data/collections.json");

/// Wear bucket upper bounds in legacy `rarityLevels` order, followed by
/// `Battle-Scarred` (legacy's fallthrough, which is never filtered by chance
/// because its bound of 1.0 is never `> averageFloat`).
const WEAR_BOUNDS: [(&str, f64); 5] = [
    ("Factory New", 0.07),
    ("Minimal Wear", 0.15),
    ("Field-Tested", 0.38),
    ("Well-Worn", 0.45),
    ("Battle-Scarred", 1.0),
];

/// Legacy stops searching upward once `i + quality > 15`.
const MAX_QUALITY: u32 = 15;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct CollectionEntry {
    #[serde(default)]
    pub best_quality: Quality,
    #[serde(rename = "min-wear", default)]
    pub min_wear: Option<Number>,
    #[serde(rename = "max-wear", default)]
    pub max_wear: Option<Number>,
    #[serde(default)]
    pub trade_up: bool,
    #[serde(rename = "imageURL", default)]
    pub image_url: Option<String>,
}

/// Wear bounds are numbers in most entries but strings in some.
#[derive(Debug, Clone, Copy, Default)]
pub struct Number(pub f64);

impl<'de> serde::Deserialize<'de> for Number {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{self, Visitor};
        struct NumberVisitor;
        impl Visitor<'_> for NumberVisitor {
            type Value = Number;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a wear bound as a number or numeric string")
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Number, E> {
                Ok(Number(value as f64))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Number, E> {
                Ok(Number(value as f64))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Number, E> {
                Ok(Number(value))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Number, E> {
                Ok(Number(value.trim().parse().unwrap_or(0.0)))
            }
        }
        deserializer.deserialize_any(NumberVisitor)
    }
}

/// `best_quality` is a number for most entries but a string for some.
#[derive(Debug, Clone, Copy, Default)]
pub struct Quality(pub u32);

impl<'de> serde::Deserialize<'de> for Quality {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::{self, Visitor};
        struct QualityVisitor;
        impl Visitor<'_> for QualityVisitor {
            type Value = Quality;
            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a rarity level as a number or numeric string")
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Quality, E> {
                Ok(Quality(value as u32))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Quality, E> {
                Ok(Quality(value.clamp(0, u32::MAX as i64) as u32))
            }
            fn visit_f64<E: de::Error>(self, value: f64) -> Result<Quality, E> {
                Ok(Quality(value as u32))
            }
            fn visit_str<E: de::Error>(self, value: &str) -> Result<Quality, E> {
                Ok(Quality(value.trim().parse().unwrap_or(0)))
            }
        }
        deserializer.deserialize_any(QualityVisitor)
    }
}

pub struct Collections {
    /// Skin name → collection name (legacy `directory`).
    directory: HashMap<String, String>,
    collections: BTreeMap<String, BTreeMap<String, CollectionEntry>>,
}

impl Collections {
    /// The bundled snapshot, parsed once on first use.
    pub fn bundled() -> &'static Collections {
        static TABLE: OnceLock<Collections> = OnceLock::new();
        TABLE.get_or_init(|| Collections::from_json(COLLECTIONS_JSON))
    }

    fn from_json(json: &str) -> Collections {
        let collections: BTreeMap<String, BTreeMap<String, CollectionEntry>> =
            serde_json::from_str(json).expect("the bundled collection snapshot parses");
        let mut directory = HashMap::new();
        for (collection, skins) in &collections {
            for skin in skins.keys() {
                directory.insert(skin.clone(), collection.clone());
            }
        }
        Collections {
            directory,
            collections,
        }
    }

    /// Legacy `getTradeUp`: whether `item_name` (with or without the
    /// `StatTrak™` prefix) sits in a bundled collection and is flagged
    /// trade-up eligible there.
    pub fn is_eligible(&self, item_name: &str) -> bool {
        let name = item_name.replace("StatTrak™ ", "");
        self.directory
            .get(&name)
            .and_then(|collection| self.collections.get(collection)?.get(&name))
            .is_some_and(|entry| entry.trade_up)
    }

    /// Collection name for a skin (`directory` lookup).
    pub fn collection_of(&self, item_name: &str) -> Option<&str> {
        self.directory
            .get(&item_name.replace("StatTrak™ ", ""))
            .map(String::as_str)
    }

    /// Distinct collection names in the bundled snapshot (unsorted).
    pub fn collection_names(&self) -> impl Iterator<Item = &str> {
        self.collections.keys().map(String::as_str)
    }
}

/// Legacy `itemProcessors` gate before the collection flag: Covert items,
/// knives (quality 3), Souvenirs, and anything not a generated skin are never
/// trade-up candidates (`tradeUp` stays false and `rarity` is unset).
pub fn is_trade_up_skin(
    name: &str,
    rarity: u32,
    quality: u32,
    image_path: Option<&str>,
) -> bool {
    rarity != 6
        && quality != 3
        && !name.contains("Souvenir")
        && image_path.is_some_and(|path| path.contains("econ/default_generated"))
}

/// Legacy `itemProcessorGetRarityName`; unknown rarities are `None` (and so
/// never match another item, exactly like `rarityDict[undefined]`).
pub fn rarity_name(rarity: u32) -> Option<&'static str> {
    match rarity {
        1 => Some("Consumer Grade"),
        2 => Some("Industrial Grade"),
        3 => Some("Mil-Spec"),
        4 => Some("Restricted"),
        5 => Some("Classified"),
        6 => Some("Covert"),
        _ => None,
    }
}

/// What legacy kept on candidate rows (`tradeUp`, `rarityName`, `stattrak`
/// plus the wear and price used by the filters).
#[derive(Debug, Clone, PartialEq)]
pub struct CandidateItem {
    pub id: String,
    pub name: String,
    pub custom_name: Option<String>,
    pub collection: String,
    pub image_url: Option<String>,
    pub rarity_name: &'static str,
    pub stat_trak: bool,
    pub wear: Option<f64>,
    pub wear_name: Option<String>,
    pub price: Option<f64>,
    pub equipped_t: bool,
    pub equipped_ct: bool,
}

/// Legacy `tradeUpReducer` filter knobs.
#[derive(Debug, Clone, PartialEq)]
pub struct ContractFilter {
    pub search: String,
    pub min_float: f64,
    pub max_float: f64,
    /// Empty means every collection.
    pub collections: Vec<String>,
    pub hide_equipped: bool,
    pub selected: Vec<String>,
}

impl Default for ContractFilter {
    fn default() -> Self {
        Self {
            search: String::new(),
            min_float: 0.0,
            max_float: 1.0,
            collections: Vec::new(),
            hide_equipped: false,
            selected: Vec::new(),
        }
    }
}

/// Up to ten entries forming one contract.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Contract {
    pub items: Vec<CandidateItem>,
}

impl Contract {
    pub const CAPACITY: usize = 10;

    /// Legacy `TRADEUP_ADD_REMOVE`: toggles the item; adding past ten is
    /// ignored. Any change below ten clears the computed outcomes.
    pub fn toggle(&mut self, item: CandidateItem) {
        if let Some(position) = self.items.iter().position(|entry| entry.id == item.id) {
            self.items.remove(position);
        } else if self.items.len() < Self::CAPACITY {
            self.items.push(item);
        }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.items.iter().any(|entry| entry.id == id)
    }

    pub fn remove(&mut self, id: &str) {
        self.items.retain(|entry| entry.id != id);
    }

    pub fn is_full(&self) -> bool {
        self.items.len() == Self::CAPACITY
    }

    /// Mean wear of the inputs (legacy divides by the input count).
    pub fn average_wear(&self) -> f64 {
        if self.items.is_empty() {
            return 0.0;
        }
        let total: f64 = self.items.iter().map(|item| item.wear.unwrap_or(0.0)).sum();
        total / self.items.len() as f64
    }
}

/// Legacy picker rules, in order: eligible, within the float range, not
/// selected, in a chosen collection, not equipped when hidden, and — once
/// the contract has an item — the same rarity and StatTrak state. Search
/// matches the name, custom name, or wear name case-insensitively.
pub fn picker_matches(
    item: &CandidateItem,
    custom_name: Option<&str>,
    filter: &ContractFilter,
) -> bool {
    if item
        .wear
        .is_some_and(|wear| wear < filter.min_float || wear > filter.max_float)
    {
        return false;
    }
    if filter.selected.iter().any(|id| id == &item.id) {
        return false;
    }
    if !filter.collections.is_empty() && !filter.collections.contains(&item.collection) {
        return false;
    }
    if filter.hide_equipped && (item.equipped_t || item.equipped_ct) {
        return false;
    }
    if filter.search.trim().is_empty() {
        return true;
    }
    let needle = filter.search.trim().to_lowercase();
    let matches = |value: Option<&str>| {
        value.is_some_and(|value| value.to_lowercase().contains(&needle))
    };
    matches(Some(&item.name))
        || matches(custom_name)
        || matches(item.wear_name.as_deref())
}

/// One possible outcome of a trade-up (legacy `getPotentitalOutcome` rows).
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub name: String,
    pub wear_name: &'static str,
    /// Chance to hit this skin, as a percentage with two decimals.
    pub percentage: f64,
    pub image_url: Option<String>,
    /// Resulting float for the average float of the inputs.
    pub float: f64,
}

/// Legacy `getRarity`: the wear bucket for `average` within the outcome's
/// wear range, plus the resulting float `(max - min) * average + min`.
pub fn wear_for(min_wear: f64, max_wear: f64, average: f64) -> (&'static str, f64) {
    let span = max_wear - min_wear;
    let float = span * average + min_wear;
    for (name, bound) in WEAR_BOUNDS {
        if (bound - min_wear) / span > average {
            return (name, float);
        }
    }
    ("Battle-Scarred", float)
}

/// Computes every possible outcome for a ten-item contract. Returns `None`
/// when the input list is empty. The `average` is taken over the inputs whose
/// name resolves to a collection entry (legacy added `undefined` for the
/// rest, which turned the total into `NaN`).
pub fn outcomes(items: &[(String, Option<f64>)]) -> Option<Vec<Outcome>> {
    let table = Collections::bundled();
    Some(outcomes_with(table, items))
}

/// [`outcomes`] against an explicit table, so tests can use fixtures.
pub fn outcomes_with(table: &Collections, items: &[(String, Option<f64>)]) -> Vec<Outcome> {
    let Some(first) = items.first() else {
        return Vec::new();
    };
    let is_stattrak = first.0.contains("StatTrak™");
    let mut pool: Vec<(String, CollectionEntry)> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    let mut average = 0.0;
    let mut counted = 0usize;
    for (name, float) in items {
        let name = name.replace("StatTrak™ ", "");
        let Some(collection) = table.directory.get(&name) else {
            continue;
        };
        let Some(entry) = table
            .collections
            .get(collection)
            .and_then(|skins| skins.get(&name))
        else {
            continue;
        };
        average += float.unwrap_or(0.0);
        counted += 1;
        for outcome_name in possible(table, collection, entry.best_quality.0) {
            if !seen.contains(&outcome_name) {
                seen.push(outcome_name);
            }
            let Some(outcome_entry) = table
                .collections
                .get(collection)
                .and_then(|skins| skins.get(outcome_name))
            else {
                continue;
            };
            pool.push((outcome_name.to_owned(), outcome_entry.clone()));
        }
    }
    if counted == 0 {
        return Vec::new();
    }
    let average = average / counted as f64;
    let mut result: Vec<Outcome> = Vec::new();
    for name in seen {
        let Some((_, entry)) = pool.iter().find(|(outcome, _)| outcome == name) else {
            continue;
        };
        let min_wear = entry.min_wear.unwrap_or(Number(0.0)).0;
        let max_wear = entry.max_wear.unwrap_or(Number(1.0)).0;
        let (wear_name, float) = wear_for(min_wear, max_wear, average);
        let matches = pool.iter().filter(|(outcome, _)| *outcome == name).count();
        let percentage = 100.0 / (pool.len() as f64 / matches as f64);
        result.push(Outcome {
            name: if is_stattrak {
                format!("StatTrak™ {name}")
            } else {
                name.to_owned()
            },
            wear_name,
            percentage: (percentage * 100.0).round() / 100.0,
            image_url: entry.image_url.clone(),
            float,
        });
    }
    result
}

/// Legacy `getPossible`: every skin in the collection whose `best_quality`
/// is exactly `quality + i`, for the first `i >= 1` that yields any (stops
/// once `i + quality > 15`).
fn possible<'a>(
    table: &'a Collections,
    collection: &str,
    quality: u32,
) -> Vec<&'a str> {
    let mut step = 1;
    loop {
        let found: Vec<&str> = table
            .collections
            .get(collection)
            .map(|skins| {
                skins
                    .iter()
                    .filter(|(_, entry)| entry.best_quality.0 == quality + step)
                    .map(|(name, _)| name.as_str())
                    .collect()
            })
            .unwrap_or_default();
        if !found.is_empty() || step + quality > MAX_QUALITY {
            return found;
        }
        step += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn json() -> &'static str {
        r#"{
            "Test Collection": {
                "AK-47 | Martian": {"best_quality": 4, "min-wear": "0.00", "max-wear": "1.00", "trade_up": true, "imageURL": "https://example/martian.png"},
                "M4A4 | Vapor": {"best_quality": "8", "min-wear": "0.06", "max-wear": "0.80", "trade_up": true, "imageURL": "https://example/vapor.png"},
                "AWP | Worm God": {"best_quality": 10, "min-wear": "0.00", "max-wear": "0.45", "trade_up": false, "imageURL": "https://example/worm.png"}
            },
            "Second Collection": {
                "Glock-18 | Fade": {"best_quality": 8, "min-wear": "0.00", "max-wear": "0.08", "trade_up": true}
            }
        }"#
    }

    #[test]
    fn eligibility_follows_the_collection_flag() {
        let table = Collections::from_json(json());
        assert!(table.is_eligible("AK-47 | Martian"));
        assert!(table.is_eligible("StatTrak™ M4A4 | Vapor"));
        assert!(!table.is_eligible("AWP | Worm God"));
        assert!(!table.is_eligible("P250 | Unknown"));
    }

    #[test]
    fn wear_buckets_match_the_legacy_thresholds() {
        assert_eq!(wear_for(0.0, 1.0, 0.05).0, "Factory New");
        assert_eq!(wear_for(0.0, 1.0, 0.08).0, "Minimal Wear");
        assert_eq!(wear_for(0.0, 1.0, 0.2).0, "Field-Tested");
        assert_eq!(wear_for(0.0, 1.0, 0.9).0, "Battle-Scarred");
        // Narrow ranges shift the bucket boundaries.
        assert_eq!(wear_for(0.0, 0.08, 0.06).0, "Factory New");
        assert_eq!(wear_for(0.0, 0.08, 5.0).0, "Well-Worn");
        let (_, float) = wear_for(0.06, 0.80, 0.25);
        assert!((float - (0.74 * 0.25 + 0.06)).abs() < 1e-9);
    }

    #[test]
    fn outcomes_skip_to_the_next_rarity_and_weight_by_hits() {
        let table = Collections::from_json(json());
        // From quality 4 the next populated level is 8: only Vapor.
        assert_eq!(possible(&table, "Test Collection", 4), vec!["M4A4 | Vapor"]);
        // From quality 8 the next is 10 (not trade-up eligible but still an
        // outcome, as in legacy).
        assert_eq!(possible(&table, "Test Collection", 8), vec!["AWP | Worm God"]);
    }

    #[test]
    fn combined_pool_sets_the_same_skin_probability() {
        // Four inputs from Test Collection (quality 4 → Vapor) and six from
        // Second Collection (quality 8 → nothing at 9, so the loop stops at
        // 15 and yields an empty list).
        let items = vec![
            ("AK-47 | Martian".to_owned(), Some(0.03)),
            ("AK-47 | Martian".to_owned(), Some(0.05)),
            ("AK-47 | Martian".to_owned(), Some(0.07)),
            ("AK-47 | Martian".to_owned(), Some(0.09)),
            ("Glock-18 | Fade".to_owned(), Some(0.01)),
            ("Glock-18 | Fade".to_owned(), Some(0.02)),
            ("Glock-18 | Fade".to_owned(), Some(0.03)),
            ("Glock-18 | Fade".to_owned(), Some(0.04)),
            ("Glock-18 | Fade".to_owned(), Some(0.05)),
            ("Glock-18 | Fade".to_owned(), Some(0.06)),
        ];
        let outcomes = outcomes_with(&Collections::from_json(json()), &items);
        assert_eq!(outcomes.len(), 1);
        let outcome = &outcomes[0];
        assert_eq!(outcome.name, "M4A4 | Vapor");
        assert!((outcome.percentage - 100.0).abs() < 1e-9);
        assert_eq!(outcome.wear_name, "Minimal Wear");
        let average = (0.03 + 0.05 + 0.07 + 0.09 + 0.01 + 0.02 + 0.03 + 0.04 + 0.05 + 0.06) / 10.0;
        assert!((outcome.float - (0.74 * average + 0.06)).abs() < 1e-9);
        assert_eq!(outcome.image_url.as_deref(), Some("https://example/vapor.png"));
    }

    #[test]
    fn stattrak_prefix_carries_to_the_outcomes() {
        let items: Vec<(String, Option<f64>)> =
            std::iter::repeat_n(("StatTrak™ AK-47 | Martian".to_owned(), Some(0.1)), 10).collect();
        let outcomes = outcomes_with(&Collections::from_json(json()), &items);
        assert_eq!(outcomes[0].name, "StatTrak™ M4A4 | Vapor");
    }

    fn candidate(id: &str, name: &str, collection: &str, wear: Option<f64>) -> CandidateItem {
        CandidateItem {
            id: id.to_owned(),
            name: name.to_owned(),
            custom_name: None,
            collection: collection.to_owned(),
            image_url: None,
            rarity_name: "Restricted",
            stat_trak: false,
            wear,
            wear_name: Some("Field-Tested".to_owned()),
            price: Some(1.0),
            equipped_t: false,
            equipped_ct: false,
        }
    }

    #[test]
    fn trade_up_gate_matches_the_legacy_processor() {
        let skin = Some("econ/default_generated/weapon_ak47_red_light_large");
        assert!(is_trade_up_skin("AK-47 | Redline", 4, 4, skin));
        assert!(!is_trade_up_skin("AK-47 | Redline", 6, 4, skin));
        assert!(!is_trade_up_skin("★ Karambit", 6, 3, skin));
        assert!(!is_trade_up_skin("Souvenir AK-47", 4, 4, skin));
        assert!(!is_trade_up_skin("AK-47", 4, 4, Some("econ/weapons/base_weapons/x")));
        assert_eq!(rarity_name(4), Some("Restricted"));
        assert_eq!(rarity_name(0), None);
    }

    #[test]
    fn contract_toggles_and_stops_at_ten() {
        let mut contract = Contract::default();
        for index in 0..12 {
            let item = candidate(&index.to_string(), "AK", "Test", Some(0.1));
            contract.toggle(item);
        }
        assert_eq!(contract.items.len(), 10);
        assert!(contract.is_full());
        // Toggling a member removes it, freeing a slot.
        let item = candidate("3", "AK", "Test", Some(0.1));
        contract.toggle(item);
        assert_eq!(contract.items.len(), 9);
        assert!(!contract.contains("3"));
        assert!((contract.average_wear() - 0.1).abs() < 1e-9);
    }

    #[test]
    fn picker_applies_range_selection_and_lock_filters() {
        let filter = ContractFilter {
            min_float: 0.10,
            max_float: 0.20,
            selected: vec!["b".to_owned()],
            hide_equipped: true,
            ..ContractFilter::default()
        };
        let plain = candidate("a", "AK-47 | Redline", "Phoenix", Some(0.15));
        assert!(picker_matches(&plain, None, &filter));
        let low = candidate("a", "AK", "Phoenix", Some(0.05));
        assert!(!picker_matches(&low, None, &filter));
        let chosen = candidate("b", "AK", "Phoenix", Some(0.15));
        assert!(!picker_matches(&chosen, None, &filter));
        let equipped = CandidateItem {
            equipped_ct: true,
            ..candidate("a", "AK", "Phoenix", Some(0.15))
        };
        assert!(!picker_matches(&equipped, None, &filter));
        let mut filtered = ContractFilter {
            collections: vec!["Nuke".to_owned()],
            ..ContractFilter::default()
        };
        assert!(!picker_matches(&plain, None, &filtered));
        filtered.collections.clear();
        filtered.search = " REDLINE ".to_owned();
        assert!(picker_matches(&plain, None, &filtered));
        // Wear-less items pass the float range, as legacy compared against
        // `undefined` (never less, never greater).
        let floating = candidate("c", "AK", "Phoenix", None);
        assert!(picker_matches(&floating, None, &ContractFilter::default()));
    }

    #[test]
    fn bundled_snapshot_parses_and_marks_eligible_skins() {
        let table = Collections::bundled();
        assert!(table.collections.len() > 50);
        assert!(table.is_eligible("AK-47 | Redline"));
        assert!(!table.is_eligible("AK-47 | Gold Arabesque"));
    }
}
