//! Category and subcategory classification, ported from legacy
//! `getCategory` + `categories.tsx`.

/// Category definitions in legacy `itemCategories` order: an item belongs to
/// the first category whose value occurs in its item URL (econ path), and
/// items matching no category stay uncategorized (legacy's leftover bucket).
pub const CATEGORIES: [(&str, &str); 8] = [
    ("characters", "Agents"),
    ("status_icons", "Collectibles & Passes"),
    ("weapon_cases", "Containers"),
    ("patches", "Patches"),
    ("music_kits", "Music kits"),
    ("default_generated", "Skins & Knives"),
    ("stickers", "Stickers"),
    ("tools", "Tools"),
];

/// Tournament names from legacy `itemSubCategories.majors`, in legacy order
/// (order only breaks ties between names at the same position).
pub const MAJORS: [&str; 17] = [
    "Antwerp 2022",
    "Stockholm 2021",
    "2020 RMR",
    "Berlin 2019",
    "Katowice 2019",
    "London 2018",
    "Boston 2018",
    "Krakow 2017",
    "Atlanta 2017",
    "Cologne 2016",
    "Columbus 2016",
    "Cluj-Napoca 2015",
    "Cologne 2015",
    "Katowice 2015",
    "DreamHack 2014",
    "Cologne 2014",
    "Katowice 2014",
];

pub fn classify_category(item_url: &str) -> Option<&'static str> {
    CATEGORIES
        .iter()
        .find(|(value, _)| item_url.contains(value))
        .map(|(_, name)| *name)
}

/// Legacy builds one alternation regex over the major keys and takes the
/// match at the lowest position in the item name (JS `String.match`), with
/// pattern order breaking ties.
pub fn classify_major(item_name: &str) -> Option<&'static str> {
    MAJORS
        .iter()
        .enumerate()
        .filter_map(|(index, major)| {
            item_name
                .find(major)
                .map(|position| (position, index, *major))
        })
        .min_by_key(|(position, index, _)| (*position, *index))
        .map(|(_, _, major)| major)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_every_legacy_category_by_econ_path() {
        for (value, name) in CATEGORIES {
            let path = format!("econ/something/{value}/item.png");
            assert_eq!(classify_category(&path), Some(name), "path {path}");
        }
        assert_eq!(
            classify_category("econ/weapons/base_weapons/weapon_ak47"),
            None
        );
        assert_eq!(classify_category(""), None);
    }

    #[test]
    fn first_category_in_legacy_order_wins_when_multiple_match() {
        // 'weapon_cases' (3rd) precedes 'tools' (8th), mirroring the legacy
        // loop order that assigns categories first-match-first.
        assert_eq!(
            classify_category("econ/tools/weapon_cases/odd_item.png"),
            Some("Containers")
        );
    }

    #[test]
    fn major_matches_at_the_lowest_position_in_the_name() {
        assert_eq!(
            classify_major("Souvenir Package | Katowice 2015"),
            Some("Katowice 2015")
        );
        // "Berlin 2019" appears earlier in the name than pattern-order "Antwerp 2022".
        assert_eq!(
            classify_major("Berlin 2019 and Antwerp 2022 capsule"),
            Some("Berlin 2019")
        );
        assert_eq!(
            classify_major("Stockholm 2021 stickers"),
            Some("Stockholm 2021")
        );
        assert_eq!(classify_major("AK-47 | Redline"), None);
    }
}
