use app_commands::InventoryItem;
use std::collections::HashSet;

pub const PAGE_SIZE: usize = 50;

pub fn rarity_name(rarity: u32) -> &'static str {
    match rarity {
        1 => "Consumer",
        2 => "Industrial",
        3 => "Mil-Spec",
        4 => "Restricted",
        5 => "Classified",
        6 => "Covert",
        7 => "Contraband",
        _ => "Default",
    }
}

/// Legacy base-sort key: `item_name.replaceAll('★','').replaceAll(' ','')`.
fn normalized_name(name: &str) -> String {
    name.replace(['★', ' '], "")
}

/// A display row of equivalent items: the first item represents the group,
/// `combined_ids` retains every member and `combined_qty` is the group size
/// (legacy `combined_ids` / `combined_QTY`).
#[derive(Clone, Debug, PartialEq)]
pub struct GroupedItem {
    pub representative: InventoryItem,
    pub combined_ids: Vec<String>,
    pub combined_qty: usize,
}

/// Legacy `combineInventory` key: name, custom name, item URL (our resolved
/// image URL), wear name, trade unlock, moveable — and because this UI
/// always shows the Float column — the exact paint wear. Sticker fields join
/// the key once sticker metadata is modeled.
fn group_key(item: &InventoryItem) -> String {
    format!(
        "{}\u{1}{}\u{1}{}\u{1}{:?}\u{1}{}\u{1}{:?}\u{1}{}",
        item.name,
        item.custom_name.as_deref().unwrap_or(""),
        item.wear_name.as_deref().unwrap_or(""),
        item.wear,
        item.image_url.as_deref().unwrap_or(""),
        item.trade_unlock,
        item.moveable
    )
}

/// Groups equivalent items in first-seen order, like legacy `combineInventory`
/// (which runs once per inventory ingest, before filtering and sorting).
pub fn group_items(items: Vec<InventoryItem>) -> Vec<GroupedItem> {
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut groups: Vec<Vec<InventoryItem>> = Vec::new();
    for item in items {
        let key = group_key(&item);
        match index.get(&key) {
            Some(&position) => groups[position].push(item),
            None => {
                index.insert(key, groups.len());
                groups.push(vec![item]);
            }
        }
    }
    groups
        .into_iter()
        .map(|members| {
            let mut members = members.into_iter();
            let representative = members.next().expect("groups are never empty");
            let mut combined_ids = Vec::with_capacity(members.len() + 1);
            combined_ids.push(representative.id.clone());
            combined_ids.extend(members.map(|item| item.id));
            let combined_qty = combined_ids.len();
            GroupedItem {
                representative,
                combined_ids,
                combined_qty,
            }
        })
        .collect()
}

/// Sort parity with legacy `sortDataFunction`: every mode sits on top of the
/// star-stripped, space-stripped name comparison (the legacy base sort, which
/// JS stability turns into the tie-breaker), rarity ascends (missing-first in
/// legacy ascending order), wear descends with unknown wear last, and the
/// quantity sort descends over combined quantities.
/// Generic over `GroupedItem` and `&GroupedItem` so the view can sort its
/// filtered rows without cloning them.
pub fn sort_rows<T: std::borrow::Borrow<GroupedItem>>(items: &mut [T], mode: &str) {
    fn by_name(left: &GroupedItem, right: &GroupedItem) -> std::cmp::Ordering {
        normalized_name(&left.representative.name).cmp(&normalized_name(&right.representative.name))
    }
    match mode {
        "name" => items.sort_by(|left, right| {
            by_name(left.borrow(), right.borrow()).then_with(|| {
                left.borrow()
                    .representative
                    .id
                    .cmp(&right.borrow().representative.id)
            })
        }),
        "rarity" => items.sort_by(|left, right| {
            left.borrow()
                .representative
                .rarity
                .cmp(&right.borrow().representative.rarity)
                .then_with(|| by_name(left.borrow(), right.borrow()))
        }),
        "wear" => items.sort_by(|left, right| {
            right
                .borrow()
                .representative
                .wear
                .unwrap_or(f64::NEG_INFINITY)
                .total_cmp(
                    &left
                        .borrow()
                        .representative
                        .wear
                        .unwrap_or(f64::NEG_INFINITY),
                )
                .then_with(|| by_name(left.borrow(), right.borrow()))
        }),
        "qty" => items.sort_by(|left, right| {
            right
                .borrow()
                .combined_qty
                .cmp(&left.borrow().combined_qty)
                .then_with(|| by_name(left.borrow(), right.borrow()))
        }),
        "category" => items.sort_by(|left, right| {
            // Legacy `sortRun(category…)`: uncategorized (undefined) sorts
            // first ascending; empty string reproduces that.
            left.borrow()
                .representative
                .category
                .as_deref()
                .unwrap_or_default()
                .cmp(
                    right
                        .borrow()
                        .representative
                        .category
                        .as_deref()
                        .unwrap_or_default(),
                )
                .then_with(|| by_name(left.borrow(), right.borrow()))
        }),
        "tradehold" => items.sort_by(|left, right| {
            // Legacy tradehold sort: items without a tradehold order first
            // (the JS NaN branch returns -1), then soonest unlock first.
            let order = match (
                left.borrow().representative.trade_unlock,
                right.borrow().representative.trade_unlock,
            ) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.cmp(&b),
            };
            order.then_with(|| by_name(left.borrow(), right.borrow()))
        }),
        "price" => items.sort_by(|left, right| {
            // Legacy price sort (`sortRunAlt`): unpriced rows (NaN →
            // -9e11) first, then ascending price × combined quantity.
            let price = |row: &GroupedItem| {
                row.representative
                    .price
                    .map(|price| price * row.combined_qty as f64)
            };
            let (left, right) = (left.borrow(), right.borrow());
            let order = match (price(left), price(right)) {
                (None, None) => std::cmp::Ordering::Equal,
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (Some(a), Some(b)) => a.total_cmp(&b),
            };
            order.then_with(|| by_name(left, right))
        }),
        _ => items.sort_by(|left, right| {
            left.borrow()
                .representative
                .position
                .cmp(&right.borrow().representative.position)
                .then_with(|| by_name(left.borrow(), right.borrow()))
        }),
    }
}

pub fn toggle_filter(set: &mut HashSet<String>, value: String) {
    if !set.remove(&value) {
        set.insert(value);
    }
}

/// Legacy filter semantics: OR within a filter group (union), AND across
/// groups — search, storage-units-only, category, rarity, majors, and the
/// boolean item properties.
pub fn matches_filters(
    row: &GroupedItem,
    query: &str,
    units_only: bool,
    categories: &HashSet<String>,
    rarities: &HashSet<String>,
    majors: &HashSet<String>,
    properties: &HashSet<String>,
) -> bool {
    let item = &row.representative;
    (!units_only || item.is_storage_unit)
        && (query.is_empty()
            || item.name.to_lowercase().contains(query)
            || item.id.contains(query)
            || item
                .custom_name
                .as_ref()
                .is_some_and(|name| name.to_lowercase().contains(query)))
        && (categories.is_empty()
            || item
                .category
                .as_ref()
                .is_some_and(|category| categories.contains(category)))
        && (rarities.is_empty() || rarities.contains(rarity_name(item.rarity)))
        && (majors.is_empty()
            || item
                .major
                .as_ref()
                .is_some_and(|major| majors.contains(major)))
        && (properties.is_empty()
            || properties
                .iter()
                .any(|property| matches_property(item, property)))
}

/// Boolean item-property toggles (legacy `characteristics`).
fn matches_property(item: &InventoryItem, property: &str) -> bool {
    match property {
        "Tradehold" => item.trade_unlock.is_some(),
        "Nametag" => item.custom_name.is_some(),
        "Moveable" => item.moveable,
        "Equipped CT" => item.equipped_ct,
        "Equipped T" => item.equipped_t,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: u64, name: &str, position: u32, rarity: u32, wear: Option<f64>) -> InventoryItem {
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

    fn grouped(item: InventoryItem) -> GroupedItem {
        let id = item.id.clone();
        GroupedItem {
            representative: item,
            combined_ids: vec![id],
            combined_qty: 1,
        }
    }

    fn names(items: &[GroupedItem]) -> Vec<&str> {
        items
            .iter()
            .map(|row| row.representative.name.as_str())
            .collect()
    }

    #[test]
    fn name_sort_strips_stars_and_spaces_then_breaks_ties_by_id() {
        let mut items = vec![
            grouped(row(9, "★ Bayonet", 0, 5, None)),
            grouped(row(2, "AK-47 Redline", 0, 5, None)),
            grouped(row(1, "AK 47", 0, 5, None)),
        ];
        sort_rows(&mut items, "name");
        // Star/space stripping matches legacy; remaining compare is plain
        // byte order ("AK-47…" < "AK47" because '-' < '4').
        assert_eq!(names(&items), vec!["AK-47 Redline", "AK 47", "★ Bayonet"]);
        // Equal normalized names fall back to id ordering (legacy Default).
        let mut tied = vec![
            grouped(row(7, "★ Awp", 0, 5, None)),
            grouped(row(3, "Awp", 0, 5, None)),
        ];
        sort_rows(&mut tied, "name");
        assert_eq!(tied[0].representative.id, "3");
        assert_eq!(tied[1].representative.id, "7");
    }

    #[test]
    fn position_sort_is_ascending_with_normalized_name_tie_breaks() {
        let mut items = vec![
            grouped(row(1, "Zebra", 5, 5, None)),
            grouped(row(2, "★ Apple", 5, 5, None)),
            grouped(row(3, "Mango", 1, 5, None)),
        ];
        sort_rows(&mut items, "position");
        assert_eq!(names(&items), vec!["Mango", "★ Apple", "Zebra"]);
    }

    #[test]
    fn rarity_sorts_ascending_like_legacy() {
        let mut items = vec![
            grouped(row(1, "Covert", 0, 6, None)),
            grouped(row(2, "Consumer", 0, 1, None)),
            grouped(row(3, "Restricted", 0, 4, None)),
        ];
        sort_rows(&mut items, "rarity");
        assert_eq!(names(&items), vec!["Consumer", "Restricted", "Covert"]);
    }

    #[test]
    fn wear_sorts_descending_with_unknown_wear_last() {
        let mut items = vec![
            grouped(row(1, "Factory New", 0, 5, Some(0.03))),
            grouped(row(2, "No wear data", 0, 5, None)),
            grouped(row(3, "Battle", 0, 5, Some(0.61))),
            grouped(row(4, "Well-Worn", 0, 5, Some(0.42))),
        ];
        sort_rows(&mut items, "wear");
        assert_eq!(
            names(&items),
            vec!["Battle", "Well-Worn", "Factory New", "No wear data"]
        );
    }

    #[test]
    fn equivalent_items_group_with_retained_ids_and_counted_quantity() {
        let items = vec![
            row(1, "AK-47 | Redline", 0, 5, Some(0.15)),
            row(2, "AK-47 | Redline", 1, 5, Some(0.15)),
            row(3, "AK-47 | Redline", 2, 5, Some(0.30)),
            row(4, "AK-47 | Redline", 3, 5, None),
            row(5, "AK-47 | Blue Laminate", 4, 5, Some(0.15)),
        ];
        let grouped = group_items(items);
        assert_eq!(grouped.len(), 4);
        // Same name + wear groups; different float and missing float split.
        assert_eq!(grouped[0].combined_ids, vec!["1", "2"]);
        assert_eq!(grouped[0].combined_qty, 2);
        assert_eq!(grouped[0].representative.id, "1");
        assert_eq!(grouped[1].representative.wear, Some(0.30));
        assert_eq!(grouped[2].representative.wear, None);

        // Nametags split groups, matching custom name in the legacy key.
        let mut first = row(6, "Glock-18", 0, 3, None);
        first.custom_name = Some("Mine".into());
        let mut second = row(7, "Glock-18", 0, 3, None);
        second.custom_name = Some("Mine".into());
        let mut third = row(8, "Glock-18", 0, 3, None);
        third.custom_name = Some("Yours".into());
        let grouped = group_items(vec![first, second, third]);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].combined_qty, 2);
        assert_eq!(grouped[1].combined_qty, 1);
    }

    #[test]
    fn quantity_sort_descends_over_combined_quantities() {
        let single = group_items(vec![row(1, "Lonely", 0, 5, None)]);
        let triple = group_items(vec![
            row(2, "Stacked", 1, 5, None),
            row(3, "Stacked", 2, 5, None),
            row(4, "Stacked", 3, 5, None),
        ]);
        let mut items = vec![single[0].clone(), triple[0].clone()];
        sort_rows(&mut items, "qty");
        assert_eq!(items[0].combined_qty, 3);
        assert_eq!(items[1].combined_qty, 1);

        // Ties fall back to the normalized name.
        let a = group_items(vec![
            row(10, "Same", 0, 5, None),
            row(11, "Same", 1, 5, None),
        ]);
        let b = group_items(vec![row(12, "Aaa", 2, 5, None), row(13, "Aaa", 3, 5, None)]);
        let mut items = vec![a[0].clone(), b[0].clone()];
        sort_rows(&mut items, "qty");
        assert_eq!(names(&items), vec!["Aaa", "Same"]);
    }

    #[test]
    fn category_sort_puts_uncategorized_first_then_orders_by_category() {
        let mut tools = row(1, "Name Tag", 0, 2, None);
        tools.category = Some("Tools".into());
        let mut skins = row(2, "AK-47 | Redline", 1, 5, None);
        skins.category = Some("Skins & Knives".into());
        let mut items = vec![
            grouped(tools),
            grouped(row(3, "Plain", 2, 1, None)),
            grouped(skins.clone()),
        ];
        sort_rows(&mut items, "category");
        // Uncategorized (legacy undefined) first, then alphabetical categories.
        assert_eq!(names(&items), vec!["Plain", "AK-47 | Redline", "Name Tag"]);
        // Equal categories fall back to the normalized name.
        let mut skins2 = row(4, "AWP | Asiimov", 3, 5, None);
        skins2.category = Some("Skins & Knives".into());
        let mut items = vec![grouped(skins), grouped(skins2)];
        sort_rows(&mut items, "category");
        assert_eq!(names(&items), vec!["AK-47 | Redline", "AWP | Asiimov"]);
    }

    #[test]
    fn filter_groups_or_within_themselves_and_and_across_groups() {
        let mut tools = row(1, "Name Tag", 0, 2, None);
        tools.category = Some("Tools".into());
        let mut skins = row(2, "AK-47 | Redline", 1, 5, None);
        skins.category = Some("Skins & Knives".into());
        let mut major = row(3, "Capsule", 2, 3, None);
        major.major = Some("Katowice 2015".into());
        let rows = [grouped(tools), grouped(skins), grouped(major)];

        let empty = HashSet::new();
        let cats: HashSet<String> = ["Tools", "Skins & Knives"]
            .into_iter()
            .map(String::from)
            .collect();
        // OR within categories: both matching rows pass, the major row doesn't.
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &cats, &empty, &empty, &empty))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["Name Tag", "AK-47 | Redline"]);

        // AND across groups: category AND rarity together.
        let rarities: HashSet<String> = ["Mil-Spec"].into_iter().map(String::from).collect();
        let none: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &cats, &rarities, &empty, &empty))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert!(none.is_empty());

        // Majors filter independently.
        let majors: HashSet<String> = ["Katowice 2015".into()].into_iter().collect();
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &empty, &empty, &majors, &empty))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["Capsule"]);

        // Search still composes with the group filters.
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "redline", false, &cats, &empty, &empty, &empty))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["AK-47 | Redline"]);
    }

    #[test]
    fn property_filters_or_within_the_group() {
        let mut locked = row(1, "Locked", 0, 5, None);
        locked.trade_unlock = Some(1_800_000_000_000);
        locked.moveable = false;
        let mut tagged = row(2, "Tagged", 1, 5, None);
        tagged.custom_name = Some("Mine".into());
        tagged.moveable = false;
        let mut plain = row(3, "Plain", 2, 5, None);
        plain.moveable = true;
        let mut equipped = row(4, "Equipped", 3, 5, None);
        equipped.equipped_ct = true;
        equipped.moveable = false;
        let rows = [
            grouped(locked),
            grouped(tagged),
            grouped(plain),
            grouped(equipped),
        ];

        let empty = HashSet::new();
        // OR within properties: tradehold OR nametag keeps both first two.
        let properties: HashSet<String> = ["Tradehold", "Nametag"]
            .into_iter()
            .map(String::from)
            .collect();
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &empty, &empty, &empty, &properties))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["Locked", "Tagged"]);

        // Moveable + equipped CT select distinct rows.
        let properties: HashSet<String> = ["Moveable"].into_iter().map(String::from).collect();
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &empty, &empty, &empty, &properties))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["Plain"]);
        let properties: HashSet<String> = ["Equipped CT"].into_iter().map(String::from).collect();
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &empty, &empty, &empty, &properties))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["Equipped"]);
    }

    #[test]
    fn tradehold_sort_puts_unlocked_items_first_then_soonest_unlock() {
        let mut later = row(1, "Later", 0, 5, None);
        later.trade_unlock = Some(1_900_000_000_000);
        let mut sooner = row(2, "Sooner", 1, 5, None);
        sooner.trade_unlock = Some(1_800_000_000_000);
        let mut items = vec![
            grouped(later),
            grouped(row(3, "Free", 2, 5, None)),
            grouped(sooner),
        ];
        sort_rows(&mut items, "tradehold");
        assert_eq!(names(&items), vec!["Free", "Sooner", "Later"]);
    }

    #[test]
    fn grouping_splits_on_trade_unlock_and_moveable() {
        let mut locked = row(1, "AK-47 | Redline", 0, 5, Some(0.2));
        locked.trade_unlock = Some(1_900_000_000_000);
        let mut unlocked = row(2, "AK-47 | Redline", 1, 5, Some(0.2));
        unlocked.trade_unlock = None;
        let grouped = group_items(vec![locked, unlocked]);
        assert_eq!(grouped.len(), 2);

        let mut base = row(3, "Glock-18", 0, 1, None);
        base.moveable = false;
        let mut skin = row(4, "Glock-18", 1, 1, None);
        skin.moveable = true;
        let grouped = group_items(vec![base, skin]);
        assert_eq!(grouped.len(), 2);
    }

    #[test]
    fn price_sort_puts_unpriced_rows_first_then_ascending_total_value() {
        let mut expensive = row(1, "Expensive", 0, 5, None);
        expensive.price = Some(10.0);
        let mut cheap = row(2, "Cheap", 1, 5, None);
        cheap.price = Some(1.0);
        let mut items = vec![
            grouped(expensive),
            grouped(row(3, "Unpriced", 2, 5, None)),
            grouped(cheap),
        ];
        sort_rows(&mut items, "price");
        // Legacy sortRunAlt: NaN (unpriced) → -9e11 → first, then ascending.
        assert_eq!(names(&items), vec!["Unpriced", "Cheap", "Expensive"]);

        // Price compares as price × combined quantity.
        let mut big_member = row(4, "Big", 0, 5, None);
        big_member.price = Some(2.0);
        let big_group = group_items(vec![
            big_member,
            row(5, "Big", 1, 5, None),
            row(6, "Big", 2, 5, None),
        ]);
        assert_eq!(big_group.len(), 1);
        assert_eq!(big_group[0].combined_qty, 3);
        let mut single = row(7, "Single", 3, 5, None);
        single.price = Some(5.0);
        let mut items = vec![
            big_group.into_iter().next().unwrap(),
            grouped(single),
        ];
        sort_rows(&mut items, "price");
        // $5×1 ranks before $2×3=$6.
        assert_eq!(names(&items), vec!["Single", "Big"]);
    }
}
