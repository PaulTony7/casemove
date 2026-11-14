use app_commands::{InventoryItem, InventorySnapshot};
use leptos::prelude::*;
use leptos_icons::Icon;
use std::collections::HashSet;

const PAGE_SIZE: usize = 50;

fn rarity_name(rarity: u32) -> &'static str {
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
    name.replace('★', "").replace(' ', "")
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
/// image URL), wear name, and — because this UI always shows the Float
/// column — the exact paint wear. Trade-unlock/moveable/sticker fields join
/// the key once those are modeled.
fn group_key(item: &InventoryItem) -> String {
    format!(
        "{}\u{1}{}\u{1}{}\u{1}{:?}\u{1}{}",
        item.name,
        item.custom_name.as_deref().unwrap_or(""),
        item.wear_name.as_deref().unwrap_or(""),
        item.wear,
        item.image_url.as_deref().unwrap_or("")
    )
}

/// Groups equivalent items in first-seen order, like legacy `combineInventory`
/// (which runs once per inventory ingest, before filtering and sorting).
fn group_items(items: Vec<InventoryItem>) -> Vec<GroupedItem> {
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
fn sort_rows(items: &mut [GroupedItem], mode: &str) {
    fn by_name(left: &GroupedItem, right: &GroupedItem) -> std::cmp::Ordering {
        normalized_name(&left.representative.name)
            .cmp(&normalized_name(&right.representative.name))
    }
    match mode {
        "name" => items.sort_by(|left, right| {
            by_name(left, right).then_with(|| left.representative.id.cmp(&right.representative.id))
        }),
        "rarity" => items.sort_by(|left, right| {
            left.representative
                .rarity
                .cmp(&right.representative.rarity)
                .then_with(|| by_name(left, right))
        }),
        "wear" => items.sort_by(|left, right| {
            right
                .representative
                .wear
                .unwrap_or(f64::NEG_INFINITY)
                .total_cmp(&left.representative.wear.unwrap_or(f64::NEG_INFINITY))
                .then_with(|| by_name(left, right))
        }),
        "qty" => items.sort_by(|left, right| {
            right
                .combined_qty
                .cmp(&left.combined_qty)
                .then_with(|| by_name(left, right))
        }),
        "category" => items.sort_by(|left, right| {
            // Legacy `sortRun(category…)`: uncategorized (undefined) sorts
            // first ascending; empty string reproduces that.
            left.representative
                .category
                .as_deref()
                .unwrap_or_default()
                .cmp(
                    right
                        .representative
                        .category
                        .as_deref()
                        .unwrap_or_default(),
                )
                .then_with(|| by_name(left, right))
        }),
        _ => items.sort_by(|left, right| {
            left.representative
                .position
                .cmp(&right.representative.position)
                .then_with(|| by_name(left, right))
        }),
    }
}

fn toggle_filter(set: &mut HashSet<String>, value: String) {
    if !set.remove(&value) {
        set.insert(value);
    }
}

/// Legacy filter semantics: OR within a filter group (union), AND across
/// groups — search, storage-units-only, category, rarity, and majors.
fn matches_filters(
    row: &GroupedItem,
    query: &str,
    units_only: bool,
    categories: &HashSet<String>,
    rarities: &HashSet<String>,
    majors: &HashSet<String>,
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
            || item.major.as_ref().is_some_and(|major| majors.contains(major)))
}

#[component]
fn InventoryRow(
    item: InventoryItem,
    display_qty: usize,
    on_open_storage: Option<Callback<(String, String)>>,
) -> impl IntoView {
    let (image_failed, set_image_failed) = signal(false);
    let image = item.image_url.unwrap_or_default();
    let has_image = !image.is_empty();
    let storage_action = on_open_storage
        .filter(|_| item.is_storage_unit)
        .map(|callback| {
            let id = item.id.clone();
            let name = item.name.clone();
            view! {
                <button class="storage-open" aria-label=format!("View contents of {}", name)
                    on:click=move |_| callback.run((id.clone(), name.clone()))>
                    <Icon icon=icondata::LuFolderOpen/>"Contents"
                </button>
            }
        });
    view! {
        <tr>
            <td class="inventory-item-cell">
                <div class="item-identity">
                    <div class="item-image">
                        <Show when=move || has_image && !image_failed.get() fallback=move || view! { <Icon icon=icondata::LuPackage/> }>
                            <img src=image.clone() alt="" loading="lazy" on:error=move |_| set_image_failed.set(true)/>
                        </Show>
                    </div>
                    <div class="item-description"><strong>{item.name}</strong>
                        {item.custom_name.map(|name| view! { <small class="nametag">{format!("\"{name}\"")}</small> })}
                        <span class="item-tags">
                            {item.stat_trak.then(|| view! { <small>"StatTrak"</small> })}
                            {item.is_storage_unit.then(|| view! { <small>"Storage unit"</small> })}
                        </span>
                    </div>
                </div>
            </td>
            <td><span class=format!("rarity rarity-{}", item.rarity.min(7))>{rarity_name(item.rarity)}</span></td>
            <td><span>{item.wear_name.unwrap_or_else(|| "-".into())}</span>
                {item.wear.map(|wear| view! { <small class="wear-value">{format!("{wear:.6}")}</small> })}
            </td>
            <td class="numeric">{item.paint_seed.map(|seed| seed.to_string()).unwrap_or_else(|| "-".into())}</td>
            <td class="numeric">{display_qty}</td>
            <td class="item-id">{item.id}</td>
            <td>{storage_action}</td>
        </tr>
    }
}

#[component]
pub fn InventoryView(
    snapshot: ReadSignal<Option<InventorySnapshot>>,
    busy: ReadSignal<bool>,
    error: ReadSignal<String>,
    storage_selection: ReadSignal<Option<(String, String)>>,
    storage_contents: ReadSignal<Option<Vec<InventoryItem>>>,
    storage_busy: ReadSignal<bool>,
    storage_error: ReadSignal<String>,
    on_open_storage: Callback<(String, String)>,
    on_close_storage: Callback<()>,
    on_refresh: Callback<()>,
    on_sign_out: Callback<()>,
) -> impl IntoView {
    let (query, set_query) = signal(String::new());
    let (sort, set_sort) = signal("position".to_owned());
    let (units_only, set_units_only) = signal(false);
    let (page, set_page) = signal(0usize);
    let (selected_categories, set_selected_categories) = signal(HashSet::<String>::new());
    let (selected_rarities, set_selected_rarities) = signal(HashSet::<String>::new());
    let (selected_majors, set_selected_majors) = signal(HashSet::<String>::new());
    let grouped = Memo::new(move |_| {
        group_items(snapshot.get().map(|snapshot| snapshot.items).unwrap_or_default())
    });
    let available_majors = Memo::new(move |_| {
        let mut majors: Vec<String> = grouped
            .get()
            .into_iter()
            .filter_map(|row| row.representative.major)
            .collect();
        majors.sort();
        majors.dedup();
        majors
    });
    let filtered = Memo::new(move |_| {
        let query = query.get().trim().to_lowercase();
        let units_only = units_only.get();
        let categories = selected_categories.get();
        let rarities = selected_rarities.get();
        let majors = selected_majors.get();
        let mut rows: Vec<_> = grouped
            .get()
            .into_iter()
            .filter(|row| matches_filters(row, &query, units_only, &categories, &rarities, &majors))
            .collect();
        sort_rows(&mut rows, &sort.get());
        rows
    });
    let pages = Memo::new(move |_| filtered.with(|items| items.len().div_ceil(PAGE_SIZE).max(1)));
    Effect::new(move |_| {
        query.track();
        sort.track();
        units_only.track();
        snapshot.track();
        set_page.set(0);
    });

    view! {
        <main class="inventory-view" aria-labelledby="inventory-heading">
            <header class="inventory-header">
                <div><h1 id="inventory-heading">{move || if storage_selection.get().is_some() { "Storage contents" } else { "Inventory" }}</h1>
                    <p class="inventory-status" role="status">{move || if storage_selection.get().is_some() {
                        if storage_busy.get() { "Loading storage contents...".into() }
                        else { storage_contents.with(|value| value.as_ref().map(|items| format!("{} items", items.len())).unwrap_or_else(|| "Contents not loaded".into())) }
                    } else if busy.get() {
                        "Waiting for the CS2 Game Coordinator...".into()
                    } else { snapshot.with(|value| value.as_ref().map(|snapshot| format!("{} items loaded", snapshot.items.len())).unwrap_or_else(|| "Not loaded".into())) }}</p>
                </div>
                <div class="inventory-actions">
                    <Show when=move || storage_selection.get().is_none() fallback=move || view! {
                        <button class="inventory-command" on:click=move |_| on_close_storage.run(())>
                            <Icon icon=icondata::LuArrowLeft/>"Inventory"
                        </button>
                    }>
                        <button class="inventory-command primary-command" disabled=move || busy.get() on:click=move |_| on_refresh.run(())>
                            <Icon icon=icondata::LuRefreshCw/>{move || if busy.get() { "Loading..." } else if snapshot.get().is_some() { "Refresh" } else { "Connect to CS2" }}
                        </button>
                    </Show>
                    <button class="icon-button" title="Sign out" aria-label="Sign out" on:click=move |_| on_sign_out.run(())><Icon icon=icondata::LuLogOut/></button>
                </div>
            </header>
            <Show when=move || storage_selection.get().is_some()>
                <section class="storage-contents" aria-label="Storage unit contents">
                    <p class="storage-parent">{move || storage_selection.get().map(|(_, name)| name).unwrap_or_default()}</p>
                    <Show when=move || !storage_error.get().is_empty()>
                        <div class="inventory-error" role="alert"><strong>{move || storage_error.get()}</strong></div>
                    </Show>
                    <Show when=move || storage_contents.get().as_ref().is_some_and(Vec::is_empty)>
                        <div class="inventory-empty"><Icon icon=icondata::LuArchive/><h2>"Storage unit is empty"</h2></div>
                    </Show>
                    <Show when=move || storage_contents.get().as_ref().is_some_and(|items| !items.is_empty())>
                        <div class="inventory-table-scroll" tabindex="0" role="region" aria-label="Storage unit items">
                            <table class="inventory-table"><thead><tr>
                                <th scope="col">"Item"</th><th scope="col">"Rarity"</th><th scope="col">"Wear / Float"</th>
                                <th scope="col">"Seed"</th><th scope="col">"Qty"</th><th scope="col">"Item ID"</th><th scope="col">"Actions"</th>
                            </tr></thead><tbody>
                                <For each=move || storage_contents.with(|value| value.clone().unwrap_or_default())
                                    key=|item| (item.id.clone(), item.name.clone(), item.custom_name.clone())
                                    children=move |item| {
                                        let display_qty = item.quantity as usize;
                                        view! { <InventoryRow item=item display_qty=display_qty on_open_storage=None/> }
                                    }/>
                            </tbody></table>
                        </div>
                    </Show>
                </section>
            </Show>
            <Show when=move || storage_selection.get().is_none()>
            <Show when=move || snapshot.get().is_none()>
                <p class="gc-session-notice"><Icon icon=icondata::LuInfo/><span>"Close any running game before connecting. Steam will mark this account as playing CS2."</span></p>
            </Show>
            <Show when=move || !error.get().is_empty()>
                <div class="inventory-error" role="alert"><strong>{move || error.get()}</strong>
                    <Show when=move || snapshot.get().is_some()><span>"Showing the last loaded snapshot."</span></Show>
                </div>
            </Show>
            <Show when=move || snapshot.get().is_some()>
                <div class="inventory-toolbar">
                    <div class="inventory-search"><Icon icon=icondata::LuSearch/>
                        <label class="sr-only" for="inventory-search">"Search inventory"</label>
                        <input id="inventory-search" type="search" placeholder="Search inventory" prop:value=move || query.get()
                            on:input=move |event| set_query.set(event_target_value(&event))/>
                    </div>
                    <label class="inventory-filter"><input type="checkbox" prop:checked=move || units_only.get()
                        on:change=move |event| set_units_only.set(event_target_checked(&event))/>"Storage units only"</label>
                    <label class="sr-only" for="inventory-sort">"Sort inventory"</label>
                    <select id="inventory-sort" prop:value=move || sort.get() on:change=move |event| set_sort.set(event_target_value(&event))>
                        <option value="position">"Position"</option><option value="name">"Name"</option>
                        <option value="rarity">"Rarity"</option><option value="wear">"Float"</option>
                        <option value="qty">"Qty"</option><option value="category">"Category"</option>
                    </select>
                </div>
                <div class="inventory-filters" aria-label="Inventory filters">
                    <div class="filter-group">
                        <span class="filter-label">"Category"</span>
                        {domain::CATEGORIES.iter().map(|(_, category)| {
                            let category = (*category).to_owned();
                            let text = category.clone();
                            let active_key = category.clone();
                            view! {
                                <button type="button" class="filter-chip"
                                    class:active=move || selected_categories.get().contains(&active_key)
                                    on:click=move |_| set_selected_categories.update(|set| toggle_filter(set, category.clone()))>
                                    {text}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                    <div class="filter-group">
                        <span class="filter-label">"Rarity"</span>
                        {(1..=7u32).map(|rarity| {
                            let name = rarity_name(rarity).to_owned();
                            let text = name.clone();
                            let active_key = name.clone();
                            view! {
                                <button type="button" class="filter-chip"
                                    class:active=move || selected_rarities.get().contains(&active_key)
                                    on:click=move |_| set_selected_rarities.update(|set| toggle_filter(set, name.clone()))>
                                    {text}
                                </button>
                            }
                        }).collect_view()}
                    </div>
                    <Show when=move || !available_majors.get().is_empty()>
                        <div class="filter-group">
                            <span class="filter-label">"Majors"</span>
                            <For each=move || available_majors.get()
                                key=|major| major.clone()
                                children=move |major| {
                                    let text = major.clone();
                                    let active_key = major.clone();
                                    view! {
                                        <button type="button" class="filter-chip"
                                            class:active=move || selected_majors.get().contains(&active_key)
                                            on:click=move |_| set_selected_majors.update(|set| toggle_filter(set, major.clone()))>
                                            {text}
                                        </button>
                                    }
                                }/>
                        </div>
                    </Show>
                </div>
                <Show when=move || !filtered.with(|items| items.is_empty()) fallback=move || view! {
                    <div class="inventory-empty"><Icon icon=icondata::LuArchive/>
                        <h2>{move || if snapshot.with(|value| value.as_ref().is_some_and(|snapshot| snapshot.items.is_empty())) { "Inventory empty" } else { "No matching items" }}</h2>
                    </div>
                }>
                    <div class="inventory-table-scroll" tabindex="0" role="region" aria-label="Inventory items">
                        <table class="inventory-table"><thead><tr>
                            <th scope="col">"Item"</th><th scope="col">"Rarity"</th><th scope="col">"Wear / Float"</th>
                            <th scope="col">"Seed"</th><th scope="col">"Qty"</th><th scope="col">"Item ID"</th><th scope="col">"Actions"</th>
                        </tr></thead><tbody>
                            <For each=move || filtered.with(|rows| rows.iter().skip(page.get().min(pages.get() - 1) * PAGE_SIZE).take(PAGE_SIZE).cloned().collect::<Vec<_>>())
                                key=|row| row.combined_ids.clone()
                                children=move |row| {
                                    let GroupedItem { representative, combined_qty, .. } = row;
                                    view! { <InventoryRow item=representative display_qty=combined_qty on_open_storage=Some(on_open_storage)/> }
                                }/>
                        </tbody></table>
                    </div>
                    <footer class="inventory-pagination">
                        <span>{move || filtered.with(|items| format!("{}-{} of {}", page.get().min(pages.get() - 1) * PAGE_SIZE + 1, ((page.get().min(pages.get() - 1) + 1) * PAGE_SIZE).min(items.len()), items.len()))}</span>
                        <div><button class="icon-button" title="Previous page" aria-label="Previous page" disabled=move || page.get() == 0
                            on:click=move |_| set_page.update(|page| *page = page.saturating_sub(1))><Icon icon=icondata::LuChevronLeft/></button>
                            <button class="icon-button" title="Next page" aria-label="Next page" disabled={move || page.get() + 1 >= pages.get()}
                                on:click=move |_| set_page.update(|page| *page += 1)><Icon icon=icondata::LuChevronRight/></button></div>
                    </footer>
                </Show>
            </Show>
            <Show when=move || snapshot.get().is_none() && !busy.get() && error.get().is_empty()>
                <div class="inventory-empty"><Icon icon=icondata::LuArchive/><h2>"Inventory not loaded"</h2></div>
            </Show>
            </Show>
        </main>
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
            quantity: 1,
            rarity,
            position,
            wear,
            wear_name: None,
            paint_seed: None,
            stat_trak: false,
            is_storage_unit: false,
            category: None,
            major: None,
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
        items.iter()
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
        assert_eq!(
            names(&items),
            vec!["AK-47 Redline", "AK 47", "★ Bayonet"]
        );
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
        let a = group_items(vec![row(10, "Same", 0, 5, None), row(11, "Same", 1, 5, None)]);
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
        let rows = vec![grouped(tools), grouped(skins), grouped(major)];

        let empty = HashSet::new();
        let cats: HashSet<String> = ["Tools", "Skins & Knives"]
            .into_iter()
            .map(String::from)
            .collect();
        // OR within categories: both matching rows pass, the major row doesn't.
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| {
                matches_filters(row, "", false, &cats, &empty, &empty)
            })
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["Name Tag", "AK-47 | Redline"]);

        // AND across groups: category AND rarity together.
        let rarities: HashSet<String> = ["Mil-Spec"].into_iter().map(String::from).collect();
        let none: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &cats, &rarities, &empty))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert!(none.is_empty());

        // Majors filter independently.
        let majors: HashSet<String> = ["Katowice 2015".into()].into_iter().collect();
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "", false, &empty, &empty, &majors))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["Capsule"]);

        // Search still composes with the group filters.
        let kept: Vec<&str> = rows
            .iter()
            .filter(|row| matches_filters(row, "redline", false, &cats, &empty, &empty))
            .map(|row| row.representative.name.as_str())
            .collect();
        assert_eq!(kept, vec!["AK-47 | Redline"]);
    }
}
