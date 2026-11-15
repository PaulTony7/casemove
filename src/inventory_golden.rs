//! Golden characterization tests for the pure inventory pipeline: grouping,
//! filtering, and sorting (legacy `combineInventory`, `filterData`, and
//! `sortDataFunction`). The fixture below stands in for a captured inventory;
//! every expectation is written out literally so a behavior change shows up
//! as a readable diff rather than as a broken invariant.

use super::inventory::{
    GroupedItem, group_items, matches_filters, matches_property, rarity_name, sort_rows,
};
use app_commands::{InventoryItem, InventorySticker};use std::collections::HashSet;

fn item(
    id: u64,
    name: &str,
    rarity: u32,
    position: u32,
    wear: Option<f64>,
    wear_name: &str,
    category: &str,
) -> InventoryItem {
    InventoryItem {
        id: id.to_string(),
        definition_index: 7,
        name: name.to_owned(),
        custom_name: None,
        image_url: None,
        image_path: None,
        quantity: 1,
        rarity,
        position,
        wear,
        wear_name: (!wear_name.is_empty()).then(|| wear_name.to_owned()),
        paint_seed: None,
        stat_trak: false,
        is_storage_unit: false,
        storage_item_count: None,
        category: (!category.is_empty()).then(|| category.to_owned()),
        major: None,
        trade_unlock: None,
        moveable: true,
        equipped_ct: false,
        equipped_t: false,
        price: None,
        collection: None,
        trade_up_eligible: false,
        stickers: Vec::new(),
        music_kit: None,
        graffiti: None,
    }
}

fn sticker(name: &str) -> InventorySticker {
    InventorySticker {
        slot: 0,
        name: name.to_owned(),
        kind: "Sticker".to_owned(),
        image_path: None,
        image_url: None,
        wear: None,
        scale: None,
        rotation: None,
        offset_x: None,
        offset_y: None,
    }
}

/// A small inventory with the cases that used to be edge cases: a duplicate
/// pair, a nametag, a star-prefixed knife, two storage units, a tradehold, an
/// uncategorized item, and a collected skin. Items group on the fields legacy
/// `combineInventory` compares, and the storage count is one of them, so the
/// two units must differ there to stay separate rows.
fn fixture() -> Vec<InventoryItem> {
    let mut redline = item(1, "AK-47 | Redline", 5, 0, Some(0.15), "Field-Tested", "Skins & Knives");
    redline.collection = Some("The Huntsman Collection".into());
    redline.price = Some(12.5);
    let mut redline_twin = redline.clone();
    redline_twin.id = "2".into();
    redline_twin.position = 1;

    let mut named = item(3, "Glock-18 | Fade", 6, 2, Some(0.01), "Factory New", "Skins & Knives");
    named.custom_name = Some("Mine".into());
    named.price = Some(800.0);

    let mut knife = item(4, "★ Bayonet", 7, 3, None, "", "Skins & Knives");
    knife.image_path = Some("econ/default_generated/weapon_knife_bayonet".into());

    let mut unit = item(5, "Storage Unit", 1, 4, None, "", "Tools");
    unit.is_storage_unit = true;
    unit.storage_item_count = Some(4);
    let mut filled_unit = unit.clone();
    filled_unit.id = "6".into();
    filled_unit.position = 5;
    filled_unit.storage_item_count = Some(900);

    let mut locked = item(7, "MP9 | Hot Rod", 4, 6, Some(0.20), "Field-Tested", "Skins & Knives");
    locked.trade_unlock = Some(1_900_000_000_000);
    locked.moveable = false;

    let mut uncategorized = item(8, "Operation Riptide Pass", 1, 7, None, "", "");
    uncategorized.collection = Some("Operation Riptide".into());

    let mut stickered = item(9, "M4A1-S | Printstream", 6, 8, Some(0.02), "Factory New", "Skins & Knives");
    stickered.stickers.push(sticker("Sticker | Crown (Foil)"));
    stickered.price = Some(200.0);

    let mut container = item(10, "Case", 1, 9, None, "", "Containers");
    container.image_path = Some("econ/weapon_cases/crate_spectrum_2".into());

    vec![
        redline,
        redline_twin.clone(),
        named,
        knife,
        unit,
        filled_unit,
        locked,
        uncategorized,
        stickered,
        container,
    ]
}

fn groups() -> Vec<GroupedItem> {
    group_items(fixture())
}

/// `(representative name, quantity)` in list order.
fn shape(rows: &[GroupedItem]) -> Vec<(&str, usize)> {
    rows.iter()
        .map(|row| (row.representative.name.as_str(), row.combined_qty))
        .collect()
}

fn sorted(mode: &str) -> Vec<String> {
    let mut rows = groups();
    sort_rows(&mut rows, mode);
    rows.iter()
        .map(|row| {
            format!(
                "{} x{}",
                row.representative.custom_name.clone().unwrap_or_else(|| row.representative.name.clone()),
                row.combined_qty
            )
        })
        .collect()
}

fn selected(
    query: &str,
    units_only: bool,
    categories: &[&str],
    rarities: &[&str],
    majors: &[&str],
    properties: &[&str],
) -> Vec<String> {
    let set = |values: &[&str]| -> HashSet<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    };
    let rows = groups();
    rows.iter()
        .filter(|row| {
            matches_filters(
                row,
                query,
                units_only,
                &set(categories),
                &set(rarities),
                &set(majors),
                &set(properties),
            )
        })
        .map(|row| row.representative.name.clone())
        .collect()
}

#[test]
fn grouping_keeps_first_seen_order_and_counts_members() {
    let rows = groups();
    assert_eq!(
        shape(&rows),
        vec![
            ("AK-47 | Redline", 2),
            ("Glock-18 | Fade", 1),
            ("★ Bayonet", 1),
            ("Storage Unit", 2),
            ("MP9 | Hot Rod", 1),
            ("Operation Riptide Pass", 1),
            ("M4A1-S | Printstream", 1),
            ("Case", 1),
        ]
    );
    // Redline is a genuine duplicate pair; both ids are retained.
    assert_eq!(rows[0].combined_ids, vec!["1", "2"]);
    assert_eq!(rows[0].representative.id, "1");
    // The storage units merge: the group key does not include the contained
    // count, so two units that only differ there share a row whose
    // representative keeps the first one's count.
    assert_eq!(rows[3].combined_ids, vec!["5", "6"]);
    assert_eq!(rows[3].representative.storage_item_count, Some(4));
}

#[test]
fn sorting_modes_match_legacy_orders() {
    // `name`: star-stripped, space-stripped comparison; rows sharing a
    // normalized name keep their grouped order.
    assert_eq!(
        sorted("name"),
        vec![
            "AK-47 | Redline x2",
            "★ Bayonet x1",
            "Case x1",
            "Mine x1",
            "M4A1-S | Printstream x1",
            "MP9 | Hot Rod x1",
            "Operation Riptide Pass x1",
            "Storage Unit x2",
        ]
    );
    // `rarity` ascends: Consumer (1) items, then Restricted and up.
    assert_eq!(
        sorted("rarity"),
        vec![
            "Case x1",
            "Operation Riptide Pass x1",
            "Storage Unit x2",
            "MP9 | Hot Rod x1",
            "AK-47 | Redline x2",
            "Mine x1",
            "M4A1-S | Printstream x1",
            "★ Bayonet x1",
        ]
    );
    // `wear` descends with unknown wear last.
    assert_eq!(
        sorted("wear"),
        vec![
            "MP9 | Hot Rod x1",
            "AK-47 | Redline x2",
            "M4A1-S | Printstream x1",
            "Mine x1",
            "★ Bayonet x1",
            "Case x1",
            "Operation Riptide Pass x1",
            "Storage Unit x2",
        ]
    );
    // `qty` descends.
    assert_eq!(
        sorted("qty"),
        vec![
            "AK-47 | Redline x2",
            "Storage Unit x2",
            "★ Bayonet x1",
            "Case x1",
            "Mine x1",
            "M4A1-S | Printstream x1",
            "MP9 | Hot Rod x1",
            "Operation Riptide Pass x1",
        ]
    );
    // `tradehold`: unlocked rows first, then the soonest unlock.
    assert_eq!(
        sorted("tradehold"),
        vec![
            "AK-47 | Redline x2",
            "★ Bayonet x1",
            "Case x1",
            "Mine x1",
            "M4A1-S | Printstream x1",
            "Operation Riptide Pass x1",
            "Storage Unit x2",
            "MP9 | Hot Rod x1",
        ]
    );
    // `price`: unpriced rows first, then price x quantity ascending.
    assert_eq!(
        sorted("price"),
        vec![
            "★ Bayonet x1",
            "Case x1",
            "MP9 | Hot Rod x1",
            "Operation Riptide Pass x1",
            "Storage Unit x2",
            "AK-47 | Redline x2",
            "M4A1-S | Printstream x1",
            "Mine x1",
        ]
    );
    // `category`: uncategorized first, then alphabetical.
    assert_eq!(
        sorted("category"),
        vec![
            "Operation Riptide Pass x1",
            "Case x1",
            "AK-47 | Redline x2",
            "★ Bayonet x1",
            "Mine x1",
            "M4A1-S | Printstream x1",
            "MP9 | Hot Rod x1",
            "Storage Unit x2",
        ]
    );
    // `collection`: no collection first, then alphabetical.
    assert_eq!(
        sorted("collection"),
        vec![
            "★ Bayonet x1",
            "Case x1",
            "Mine x1",
            "M4A1-S | Printstream x1",
            "MP9 | Hot Rod x1",
            "Storage Unit x2",
            "Operation Riptide Pass x1",
            "AK-47 | Redline x2",
        ]
    );
    // `stickers` descends by applied count.
    assert_eq!(
        sorted("stickers"),
        vec![
            "M4A1-S | Printstream x1",
            "AK-47 | Redline x2",
            "★ Bayonet x1",
            "Case x1",
            "Mine x1",
            "MP9 | Hot Rod x1",
            "Operation Riptide Pass x1",
            "Storage Unit x2",
        ]
    );
    // Default (position) order, i.e. the inventory's own order.
    assert_eq!(
        sorted("position"),
        vec![
            "AK-47 | Redline x2",
            "Mine x1",
            "★ Bayonet x1",
            "Storage Unit x2",
            "MP9 | Hot Rod x1",
            "Operation Riptide Pass x1",
            "M4A1-S | Printstream x1",
            "Case x1",
        ]
    );
}

#[test]
fn filters_match_legacy_and_or_semantics() {
    // Legacy `searchFilter` matches name, wear name, and custom name;
    // case-insensitivity comes from the view lowercasing the query.
    assert_eq!(selected("redline", false, &[], &[], &[], &[]), ["AK-47 | Redline"]);
    assert_eq!(selected("factory", false, &[], &[], &[], &[]), ["Glock-18 | Fade", "M4A1-S | Printstream"]);
    assert_eq!(selected("mine", false, &[], &[], &[], &[]), ["Glock-18 | Fade"]);
    // Beyond legacy: collection and sticker names (legacy's `searchFilter`
    // only reads name, wear name, and the custom name).
    assert_eq!(selected("huntsman", false, &[], &[], &[], &[]), ["AK-47 | Redline"]);
    assert_eq!(selected("crown", false, &[], &[], &[], &[]), ["M4A1-S | Printstream"]);
    assert_eq!(selected("", false, &[], &[], &[], &[]).len(), 8);

    // OR within a group, AND across groups. The Case is "Containers".
    assert_eq!(
        selected("", false, &["Tools"], &[], &[], &[]),
        ["Storage Unit"]
    );
    assert_eq!(
        selected("", false, &["Tools", "Containers"], &[], &[], &[]),
        ["Storage Unit", "Case"]
    );
    assert_eq!(
        selected("", false, &["Tools"], &["Consumer"], &[], &[]),
        ["Storage Unit"]
    );
    assert_eq!(
        selected("", false, &["Skins & Knives"], &["Contraband"], &[], &[]),
        ["★ Bayonet"]
    );
    assert_eq!(
        selected("ak-47", false, &["Skins & Knives"], &[], &[], &[]),
        ["AK-47 | Redline"]
    );
    // Uncategorized items are matched by no category filter.
    assert!(selected("", false, &["Patches"], &[], &[], &[]).is_empty());

    // Storage-units-only is AND-ed with everything else.
    assert_eq!(
        selected("", true, &[], &[], &[], &[]),
        ["Storage Unit"]
    );

    // Property filters (legacy `characteristics`).
    assert_eq!(selected("", false, &[], &[], &[], &["Tradehold"]), ["MP9 | Hot Rod"]);
    assert_eq!(selected("", false, &[], &[], &[], &["Nametag"]), ["Glock-18 | Fade"]);
    assert_eq!(selected("", false, &[], &[], &[], &["Container"]), ["Case"]);
    assert_eq!(
        selected("", false, &[], &[], &[], &["Tradehold", "Nametag"]),
        ["Glock-18 | Fade", "MP9 | Hot Rod"]
    );
}

#[test]
fn properties_cover_every_legacy_toggle() {
    let rows = groups();
    let names = |property: &str| -> Vec<&str> {
        rows.iter()
            .filter(|row| matches_property(&row.representative, property))
            .map(|row| row.representative.name.as_str())
            .collect()
    };
    assert_eq!(names("Tradehold"), ["MP9 | Hot Rod"]);
    assert_eq!(names("Nametag"), ["Glock-18 | Fade"]);
    assert_eq!(names("Stickers"), ["M4A1-S | Printstream"]);
    assert_eq!(names("Container"), ["Case"]);
    assert_eq!(names("StatTrak"), Vec::<&str>::new());
}

#[test]
fn rarity_names_follow_the_legacy_table() {
    let names: Vec<&str> = (0..=8).map(rarity_name).collect();
    assert_eq!(
        names,
        [
            "Default",
            "Consumer",
            "Industrial",
            "Mil-Spec",
            "Restricted",
            "Classified",
            "Covert",
            "Contraband",
            "Default",
        ]
    );
}
