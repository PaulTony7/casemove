use app_commands::{InventoryItem, InventorySnapshot};
use leptos::prelude::*;
use leptos_icons::Icon;

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

#[component]
fn InventoryRow(item: InventoryItem) -> impl IntoView {
    let (image_failed, set_image_failed) = signal(false);
    let image = item.image_url.unwrap_or_default();
    let has_image = !image.is_empty();
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
            <td class="numeric">{item.quantity}</td>
            <td class="item-id">{item.id}</td>
        </tr>
    }
}

#[component]
pub fn InventoryView(
    snapshot: ReadSignal<Option<InventorySnapshot>>,
    busy: ReadSignal<bool>,
    error: ReadSignal<String>,
    on_refresh: Callback<()>,
    on_sign_out: Callback<()>,
) -> impl IntoView {
    let (query, set_query) = signal(String::new());
    let (sort, set_sort) = signal("position".to_owned());
    let (units_only, set_units_only) = signal(false);
    let (page, set_page) = signal(0usize);
    let filtered = Memo::new(move |_| {
        let query = query.get().trim().to_lowercase();
        let units_only = units_only.get();
        let mut items: Vec<_> = snapshot
            .get()
            .map(|snapshot| snapshot.items)
            .unwrap_or_default()
            .into_iter()
            .filter(|item| {
                (!units_only || item.is_storage_unit)
                    && (query.is_empty()
                        || item.name.to_lowercase().contains(&query)
                        || item.id.contains(&query)
                        || item
                            .custom_name
                            .as_ref()
                            .is_some_and(|name| name.to_lowercase().contains(&query)))
            })
            .collect();
        match sort.get().as_str() {
            "name" => {
                items.sort_by(|left, right| left.name.cmp(&right.name).then(left.id.cmp(&right.id)))
            }
            "rarity" => items.sort_by(|left, right| {
                right
                    .rarity
                    .cmp(&left.rarity)
                    .then(left.name.cmp(&right.name))
            }),
            "wear" => items.sort_by(|left, right| {
                left.wear
                    .unwrap_or(f64::INFINITY)
                    .total_cmp(&right.wear.unwrap_or(f64::INFINITY))
            }),
            _ => items.sort_by_key(|item| item.position),
        }
        items
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
                <div><h1 id="inventory-heading">"Inventory"</h1>
                    <p class="inventory-status" role="status">{move || if busy.get() {
                        "Waiting for the CS2 Game Coordinator...".into()
                    } else { snapshot.with(|value| value.as_ref().map(|snapshot| format!("{} items loaded", snapshot.items.len())).unwrap_or_else(|| "Not loaded".into())) }}</p>
                </div>
                <div class="inventory-actions">
                    <button class="inventory-command primary-command" disabled=move || busy.get() on:click=move |_| on_refresh.run(())>
                        <Icon icon=icondata::LuRefreshCw/>{move || if busy.get() { "Loading..." } else if snapshot.get().is_some() { "Refresh" } else { "Connect to CS2" }}
                    </button>
                    <button class="icon-button" title="Sign out" aria-label="Sign out" on:click=move |_| on_sign_out.run(())><Icon icon=icondata::LuLogOut/></button>
                </div>
            </header>
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
                    </select>
                </div>
                <Show when=move || !filtered.with(|items| items.is_empty()) fallback=move || view! {
                    <div class="inventory-empty"><Icon icon=icondata::LuArchive/>
                        <h2>{move || if snapshot.with(|value| value.as_ref().is_some_and(|snapshot| snapshot.items.is_empty())) { "Inventory empty" } else { "No matching items" }}</h2>
                    </div>
                }>
                    <div class="inventory-table-scroll" tabindex="0" role="region" aria-label="Inventory items">
                        <table class="inventory-table"><thead><tr>
                            <th scope="col">"Item"</th><th scope="col">"Rarity"</th><th scope="col">"Wear / Float"</th>
                            <th scope="col">"Seed"</th><th scope="col">"Qty"</th><th scope="col">"Item ID"</th>
                        </tr></thead><tbody>
                            <For each=move || filtered.with(|items| items.iter().skip(page.get().min(pages.get() - 1) * PAGE_SIZE).take(PAGE_SIZE).cloned().collect::<Vec<_>>())
                                key=|item| (item.id.clone(), item.name.clone(), item.custom_name.clone(), item.image_url.clone(),
                                    (item.quantity, item.rarity, item.wear.map(f64::to_bits), item.wear_name.clone(), item.paint_seed, item.stat_trak, item.is_storage_unit))
                                children=move |item| view! { <InventoryRow item=item/> }/>
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
        </main>
    }
}
