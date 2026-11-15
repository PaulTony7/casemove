use crate::Core;
use app_commands::{InventoryChangeReason, InventoryItem, InventorySnapshot, InventorySticker};
use domain::trade_unlock_ms;
use gc_cs2::{EconItem, InventoryChange, InventoryClient, InventoryError};
use item_schema::ItemSchema;

/// Legacy storage units hold 1000 items.
const STORAGE_CAPACITY: usize = 1000;

#[derive(Default)]
pub struct InventoryState(tokio::sync::Mutex<Option<GcSlot<InventoryClient>>>);

struct GcSlot<Client> {
    generation: u64,
    client: Option<Client>,
    items: Vec<EconItem>,
}

fn clear_generation<Client>(slot: &mut Option<GcSlot<Client>>, generation: u64) {
    if slot
        .as_ref()
        .is_some_and(|slot| slot.generation == generation)
    {
        slot.take();
    }
}

fn prepare_slot<Client>(
    slot: &mut Option<GcSlot<Client>>,
    generation: u64,
) -> Result<bool, String> {
    if let Some(current) = slot.as_ref() {
        if current.generation > generation {
            return Err("The Steam session changed. Load inventory again.".into());
        }
        if current.generation == generation {
            return Ok(false);
        }
    }
    *slot = Some(GcSlot {
        generation,
        client: None,
        items: Vec::new(),
    });
    Ok(true)
}

impl InventoryState {
    pub async fn clear(&self, generation: u64) {
        clear_generation(&mut *self.0.lock().await, generation);
    }
}

fn is_owned_item(item: &EconItem) -> Result<bool, String> {
    let Some(attribute) = item
        .attributes
        .iter()
        .find(|attribute| attribute.definition_index == 277)
    else {
        return Ok(true);
    };
    let bytes = attribute
        .value
        .as_slice()
        .try_into()
        .map_err(|_| "CS2 returned a malformed reward-preview attribute.".to_owned())?;
    Ok(u32::from_le_bytes(bytes) != 1)
}

fn attribute_float(item: &EconItem, index: u32) -> Option<f32> {
    let bytes = &item
        .attributes
        .iter()
        .find(|attribute| attribute.definition_index == index)?
        .value;
    let value = f32::from_le_bytes(bytes.as_slice().try_into().ok()?);
    value.is_finite().then_some(value)
}

fn attribute_integer(item: &EconItem, index: u32) -> Option<u32> {
    let value = attribute_float(item, index)?;
    (value >= 0.0 && value < u32::MAX as f32 && value.fract() == 0.0).then_some(value as u32)
}

fn attribute_raw_integer(item: &EconItem, index: u32) -> Option<u32> {
    item.attributes
        .iter()
        .find(|attribute| attribute.definition_index == index)
        .and_then(|attribute| <[u8; 4]>::try_from(attribute.value.as_slice()).ok())
        .map(u32::from_le_bytes)
}

fn attribute_value(item: &EconItem, index: u32) -> Option<f64> {
    attribute_float(item, index).map(f64::from)
}

fn enrich_stickers(item: &EconItem, schema: &ItemSchema) -> Vec<InventorySticker> {
    (0..=5)
        .filter_map(|index| {
            let sticker_id = attribute_raw_integer(item, 113 + index * 4)?;
            if sticker_id == 0 {
                return None;
            }
            let details = schema.sticker(sticker_id)?;
            Some(InventorySticker {
                slot: attribute_raw_integer(item, 290 + index).unwrap_or(index),
                name: details.name,
                kind: details.kind,
                image_path: details.image_path,
                image_url: details.image_url,
                wear: attribute_value(item, 114 + index * 4),
                scale: attribute_value(item, 115 + index * 4),
                rotation: attribute_value(item, 116 + index * 4),
                offset_x: attribute_value(item, 278 + index * 2),
                offset_y: attribute_value(item, 279 + index * 2),
            })
        })
        .collect()
}

/// Legacy converter drops items stored inside storage units and two
/// placeholder IDs, in every conversion path.
const EXCLUDED_ITEM_IDS: [u64; 2] = [17293822569110896676, 17293822569102708641];

/// An item contained in a casket carries both halves of its casket ID
/// (attributes 272/273), matching globaloffensive's `casket_id` detection.
fn is_casket_contained(item: &EconItem) -> bool {
    item.attributes
        .iter()
        .any(|attribute| attribute.definition_index == 272)
        && item
            .attributes
            .iter()
            .any(|attribute| attribute.definition_index == 273)
}

fn wear_name(wear: f64) -> &'static str {
    if wear <= 0.07 {
        "Factory New"
    } else if wear <= 0.15 {
        "Minimal Wear"
    } else if wear <= 0.38 {
        "Field-Tested"
    } else if wear <= 0.45 {
        "Well-Worn"
    } else {
        "Battle-Scarred"
    }
}

/// Port of legacy `itemProcessorCanBeMoved` plus its post-overrides
/// (star quality forces moveable; promotional pins and music kits with
/// origin 0 are not).
fn compute_moveable(schema: &ItemSchema, item: &EconItem, item_url: &str, name: &str) -> bool {
    let excluded = schema.prefab(item.definition_index) == Some("collectible_untradable")
        || (item_url.contains("econ/status_icons/") && item.origin == Some(0))
        || item_url.contains("econ/status_icons/service_medal_")
        || item.definition_index == 987
        || item_url.contains("plusstars")
        || (schema
            .item_attribute(item.definition_index, "cannot trade")
            .and_then(|value| {
                value
                    .as_u64()
                    .or_else(|| value.as_str()?.parse::<u64>().ok())
            })
            == Some(1))
        || (item_url.contains("crate_key") && item.flags == Some(10))
        || item_url.contains("weapons/base_weapons");
    let mut moveable = !excluded;
    // Legacy overrides, in order.
    if item.quality == 3 {
        moveable = true;
    }
    if name.contains("Pin") && item.origin == Some(5) {
        moveable = false;
    }
    if item.music_index.is_some() && item.origin == Some(0) {
        moveable = false;
    }
    moveable
}

fn enrich_item(item: EconItem, schema: &ItemSchema) -> InventoryItem {
    let paint = attribute_integer(&item, 6).filter(|paint| *paint != 0);
    let wear = attribute_float(&item, 8)
        .map(f64::from)
        .filter(|wear| (0.0..=1.0).contains(wear));
    let paint_seed = attribute_integer(&item, 7);
    let stickers = enrich_stickers(&item, schema);
    let mut description = schema.describe(item.definition_index, paint);
    let base_name = description.name.clone();
    let collection_metadata = schema.collection(&base_name);
    let collection = collection_metadata.map(|metadata| metadata.name.clone());
    if let Some(music) = item.music_index.and_then(|index| schema.music_kit(index)) {
        description.name = music.name.clone();
        description.image_path = music.image_path;
        if music.image_url.is_some() {
            description.image_url = music.image_url;
        }
    } else if matches!(item.definition_index, 1209 | 1348 | 1349 | 4609)
        && let Some(sticker) = stickers.iter().find(|sticker| sticker.slot == 0)
    {
        description.name = format!("{base_name} | {}", sticker.name);
        if description.image_path.is_none() {
            description.image_path = sticker.image_path.clone();
        }
        if description.image_url.is_none() {
            description.image_url = sticker.image_url.clone();
        }
    }
    let graffiti_tint = attribute_raw_integer(&item, 233);
    if matches!(item.definition_index, 1209 | 1348 | 1349 | 4609)
        && let Some(kit_id) = attribute_raw_integer(&item, 113)
    {
        description.image_url = if matches!(item.definition_index, 1348 | 1349) {
            schema.graffiti_image(kit_id, graffiti_tint)
        } else {
            schema.sticker(kit_id).and_then(|sticker| sticker.image_url)
        };
    }
    let graffiti = graffiti_tint.and_then(|id| schema.graffiti_name(id));
    if let Some(graffiti_name) = &graffiti {
        description.name.push_str(&format!(" ({graffiti_name})"));
    }
    if description.image_url.is_none() {
        description.image_url = schema
            .collection(&base_name)
            .and_then(|metadata| metadata.image_url.clone());
    }
    let stat_trak = item
        .attributes
        .iter()
        .any(|attribute| attribute.definition_index == 80);
    let is_souvenir = item
        .attributes
        .iter()
        .any(|attribute| attribute.definition_index == 140);
    // Legacy naming order: Souvenir inside the name processor, then the
    // StatTrak prefix, then the star for unusual-quality (knife) items.
    let mut name = description.name;
    if is_souvenir && !name.contains("Souvenir") {
        name = format!("Souvenir {name}");
    }
    if stat_trak {
        name = format!("StatTrak™ {name}");
    }
    if item.quality == 3 {
        name = format!("★ {name}");
    }
    let position = item.legacy_position();
    let wear_name = wear.map(|wear| wear_name(wear).to_owned());
    if item.definition_index == 1201
        && let Some(icon_id) = attribute_raw_integer(&item, 70)
        && let Some(icon) = schema.casket_icon(icon_id)
    {
        description.image_path = Some(icon);
    }
    let item_url = description
        .image_path
        .as_deref()
        .or(description.image_url.as_deref())
        .unwrap_or_default()
        .to_owned();
    // Category from the econ path (legacy matched `item_url`), major from the
    // final display name exactly like legacy's post-prefix `getCategory`.
    let category = domain::classify_category(&item_url).map(str::to_owned);
    let major = domain::classify_major(&name).map(str::to_owned);
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    let trade_unlock = trade_unlock_ms(&name, item.tradeable_after_secs, now_ms);
    let moveable = compute_moveable(schema, &item, &item_url, &name);
    // Legacy `itemProcessorisEquipped`: new_class 2 → T, 3 → CT.
    let equipped_ct = item.equipped_classes.contains(&3);
    let equipped_t = item.equipped_classes.contains(&2);
    // Legacy prices only moveable rows (`handleItem`), from the bundled
    // snapshot with the default source (steam_listing) in USD — the
    // source/currency settings are not ported yet. Live quotes re-resolve
    // the same rows later through `pricing::resolve`.
    let price = moveable
        .then(|| {
            pricing::resolve(
                &name,
                wear_name.as_deref(),
                pricing::PriceSource::SteamListing,
                None,
            )
        })
        .flatten();
    // Storage units advertise their contained-item count (attribute 270).
    let storage_item_count = (item.definition_index == 1201)
        .then(|| {
            item.attributes
                .iter()
                .find(|attribute| attribute.definition_index == 270)
                .and_then(|attribute| <[u8; 4]>::try_from(attribute.value.as_slice()).ok())
                .map(u32::from_le_bytes)
        })
        .flatten();
    let trade_up_eligible = collection_metadata
        .and_then(|metadata| metadata.trade_up)
        .unwrap_or(true)
        && item.rarity < 6
        && item.quality != 3
        && !name.contains("Souvenir")
        && item_url.contains("econ/default_generated");
    InventoryItem {
        id: item.id.to_string(),
        definition_index: item.definition_index,
        name,
        custom_name: item.custom_name,
        image_url: description.image_url,
        image_path: description.image_path,
        quantity: item.quantity,
        rarity: item.rarity,
        position,
        wear,
        wear_name,
        paint_seed,
        stat_trak,
        is_storage_unit: item.definition_index == 1201,
        storage_item_count,
        category,
        major,
        trade_unlock,
        moveable,
        equipped_ct,
        equipped_t,
        price,
        collection,
        trade_up_eligible,
        stickers,
        music_kit: item
            .music_index
            .and_then(|index| schema.music_kit(index))
            .map(|kit| kit.name),
        graffiti,
    }
}

/// Legacy `item_url` check for containers (`econ/tools/weapon_cases` or a
/// `crate_key` path).
fn valid_container(row: &InventoryItem) -> bool {
    row.image_path
        .as_deref()
        .or(row.image_url.as_deref())
        .is_some_and(|path| path.contains("weapon_cases"))
}

/// Keys carry `crate_key` in their econ path and are never trade-held (the
/// trade unlock skips names with `Key`).
fn valid_key(row: &InventoryItem) -> bool {
    row.image_path
        .as_deref()
        .or(row.image_url.as_deref())
        .is_some_and(|path| path.contains("crate_key"))
}

/// Legacy `itemProcessors` trade-up gate for an enriched row: Covert items,
/// knives, Souvenirs, and non-skins are never trade-up inputs.
fn valid_trade_up_input(row: &InventoryItem) -> bool {
    let quality = if row.name.starts_with('★') { 3 } else { 0 };
    domain::is_trade_up_skin(
        &row.name,
        row.rarity,
        quality,
        row.image_path.as_deref(),
    ) && !row.is_storage_unit
}

/// Converts GC items into inventory rows, applying the legacy conversion
/// filters. `include_casket` keeps items contained in storage units, which the
/// storage-contents view needs but the main inventory must hide.
fn to_rows(
    items: Vec<EconItem>,
    schema: &ItemSchema,
    include_casket: bool,
) -> Result<Vec<InventoryItem>, String> {
    let mut rows = Vec::with_capacity(items.len());
    for item in items {
        if EXCLUDED_ITEM_IDS.contains(&item.id) {
            continue;
        }
        if !include_casket && is_casket_contained(&item) {
            continue;
        }
        if is_owned_item(&item)? {
            let mut row = enrich_item(item, schema);
            // Legacy `getTradeUp`: collection eligibility comes from the
            // bundled trade-up snapshot, not the live schema.
            row.trade_up_eligible = domain::Collections::bundled().is_eligible(&row.name);
            rows.push(row);
        }
    }
    Ok(rows)
}

fn apply_inventory_change(
    items: &mut Vec<EconItem>,
    change: InventoryChange,
) -> Option<(InventoryChangeReason, EconItem)> {
    match change {
        InventoryChange::Upsert(item) => {
            let existing = items.iter().position(|old| old.id == item.id);
            let reason = if let Some(index) = existing {
                items[index] = item.clone();
                InventoryChangeReason::ItemChanged
            } else {
                items.push(item.clone());
                InventoryChangeReason::ItemAcquired
            };
            items.sort_by_key(|item| (item.legacy_position(), item.id));
            Some((reason, item))
        }
        InventoryChange::Remove(id) => {
            let index = items.iter().position(|item| item.id == id)?;
            Some((InventoryChangeReason::ItemRemoved, items.remove(index)))
        }
    }
}

fn listen_inventory_changes(
    inventory: std::sync::Arc<InventoryState>,
    generation: u64,
    steam_id: String,
    schema: std::sync::Arc<std::sync::RwLock<std::sync::Arc<ItemSchema>>>,
    events: crate::EventSink,
    mut changes: tokio::sync::mpsc::UnboundedReceiver<Result<InventoryChange, InventoryError>>,
) {
    tokio::spawn(async move {
        while let Some(change) = changes.recv().await {
            let change = match change {
                Ok(change) => change,
                Err(error) => {
                    events.emit(crate::AppEvent::InventoryError(error.to_string()));
                    return;
                }
            };
            let mut state = inventory.0.lock().await;
            let Some(slot) = state.as_mut().filter(|slot| slot.generation == generation) else {
                return;
            };
            let Some((reason, changed)) = apply_inventory_change(&mut slot.items, change) else {
                continue;
            };
            let item_id = changed.id.to_string();
            let schema = match schema.read() {
                Ok(schema) => schema.clone(),
                Err(_) => {
                    events.emit(crate::AppEvent::InventoryError(
                        "The item schema is unavailable.".into(),
                    ));
                    return;
                }
            };
            let item = match to_rows(vec![changed], &schema, false) {
                Ok(mut rows) => rows.pop(),
                Err(error) => {
                    events.emit(crate::AppEvent::InventoryError(error));
                    return;
                }
            };
            events.emit(crate::AppEvent::InventoryChanged {
                steam_id: steam_id.clone(),
                reason,
                item_id,
                item: item.map(Box::new),
            });
        }
    });
}

fn parse_storage_id(value: &str) -> Result<u64, String> {
    let id = value
        .parse::<u64>()
        .map_err(|_| "Invalid storage-unit ID.".to_owned())?;
    if id == 0 || id.to_string() != value {
        return Err("Invalid storage-unit ID.".into());
    }
    Ok(id)
}

fn storage_item_count(items: &[EconItem], casket_id: u64) -> Result<u32, String> {
    let storage = items
        .iter()
        .find(|item| item.id == casket_id && item.definition_index == 1201)
        .ok_or_else(|| "The requested storage unit is not in the current inventory.".to_owned())?;
    let Some(attribute) = storage
        .attributes
        .iter()
        .find(|attribute| attribute.definition_index == 270)
    else {
        return Ok(0);
    };
    <[u8; 4]>::try_from(attribute.value.as_slice())
        .map(u32::from_le_bytes)
        .map_err(|_| "CS2 returned a malformed storage-unit contents count.".into())
}

fn storage_contents_error(error: InventoryError) -> String {
    match error {
        InventoryError::InvalidRequest => "Invalid storage-unit request.".into(),
        InventoryError::ContentsMismatch => {
            "CS2 returned incomplete or inconsistent storage-unit contents.".into()
        }
        error => error.to_string(),
    }
}

impl Core {
    pub async fn refresh_inventory(&self) -> Result<InventorySnapshot, String> {
        let session = &*self.session;
        let inventory = &*self.inventory;
        let inventory_state = self.inventory.clone();
        let schema_state = self.schema.clone();
        let events = self.events.clone();
        session
            .with_connection(|connection, generation| async move {
                let mut state = inventory
                    .0
                    .try_lock()
                    .map_err(|_| "An inventory request is already in progress.".to_owned())?;
                let attach = session
                    .in_generation(generation, || prepare_slot(&mut state, generation))
                    .map_err(|error| error.to_string())??;
                let items = if attach {
                    let connected = if let Some(browser_client) = connection.browser_client() {
                        InventoryClient::connect_browser(browser_client).await
                    } else if let Some(steam_vent) = connection.steam_vent() {
                        InventoryClient::connect(steam_vent).await
                    } else {
                        return Err("The Steam connection is unavailable.".into());
                    };
                    let (mut client, items) = connected.map_err(|error| {
                        format!("{error} Sign out of Steam and sign back in before reconnecting.")
                    })?;
                    let changes = client.take_changes();
                    session
                        .in_generation(generation, || {
                            *state = Some(GcSlot {
                                generation,
                                client: Some(client),
                                items: items.clone(),
                            });
                        })
                        .map_err(|error| error.to_string())?;
                    if let Some(changes) = changes {
                        listen_inventory_changes(
                            inventory_state.clone(),
                            generation,
                            connection.steam_id(),
                            schema_state.clone(),
                            events.clone(),
                            changes,
                        );
                    }
                    items
                } else {
                    let client = state.as_ref().and_then(|slot| slot.client.as_ref()).ok_or_else(|| {
                        "The CS2 attach failed. Sign out of Steam and sign back in before reconnecting."
                            .to_owned()
                    })?;
                    let items = client.refresh().await.map_err(|error| error.to_string())?;
                    session
                        .in_generation(generation, || {
                            state
                                .as_mut()
                                .filter(|slot| slot.generation == generation)
                                .map(|slot| slot.items = items.clone())
                        })
                        .map_err(|error| error.to_string())?
                        .ok_or_else(|| {
                            "The Steam session changed. Load inventory again.".to_owned()
                        })?;
                    items
                };
                let schema = self.schema_snapshot()?;
                let rows = to_rows(items, &schema, false)?;
                Ok(InventorySnapshot {
                    steam_id: connection.steam_id(),
                    items: rows,
                })
            })
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn refresh_inventory_metadata(&self) -> Result<Option<InventorySnapshot>, String> {
        let session = &*self.session;
        let inventory = &*self.inventory;
        let schema = self.schema_snapshot()?;
        session
            .with_connection(|connection, generation| async move {
                let state = inventory.0.lock().await;
                let Some(slot) = state.as_ref().filter(|slot| slot.generation == generation) else {
                    return Ok(None);
                };
                let items = to_rows(slot.items.clone(), &schema, false)?;
                Ok(Some(InventorySnapshot {
                    steam_id: connection.steam_id(),
                    items,
                }))
            })
            .await
            .map_err(|error| error.to_string())?
    }

    pub async fn storage_contents(&self, casket_id: String) -> Result<Vec<InventoryItem>, String> {
        let casket_id = parse_storage_id(&casket_id)?;
        let session = &*self.session;
        let inventory = &*self.inventory;
        session
            .with_connection(|_connection, generation| async move {
                let state = inventory.0.try_lock().map_err(|_| {
                    "An inventory or storage request is already in progress.".to_owned()
                })?;
                let slot = state
                    .as_ref()
                    .filter(|slot| slot.generation == generation)
                    .ok_or_else(|| "Connect to CS2 and load inventory first.".to_owned())?;
                let expected_count = storage_item_count(&slot.items, casket_id)?;
                let client = slot.client.as_ref().ok_or_else(|| {
                    "The CS2 attach failed. Sign out of Steam and sign back in before reconnecting."
                        .to_owned()
                })?;
                let items = client
                    .storage_contents(casket_id, expected_count)
                    .await
                    .map_err(storage_contents_error)?;
                let schema = self.schema_snapshot()?;
                to_rows(items, &schema, true)
            })
            .await
            .map_err(|error| error.to_string())?
    }

    /// Reads every storage unit's contents, one GC round-trip at a time
    /// (the slot lock rejects overlapping requests, so they cannot race).
    /// Results stay paired with their unit so the management view can show
    /// per-unit counts and values.
    pub async fn all_storage_contents(
        &self,
        casket_ids: Vec<String>,
    ) -> Result<Vec<(String, Vec<InventoryItem>)>, String> {
        let mut contents = Vec::with_capacity(casket_ids.len());
        for casket_id in casket_ids {
            let items = self.storage_contents(casket_id.clone()).await?;
            contents.push((casket_id, items));
        }
        Ok(contents)
    }

    /// Moves one inventory item into a storage unit and waits for the GC
    /// acknowledgement. Ownership, moveability, and free capacity are checked
    /// before the request so an invalid move never reaches the GC.
    pub async fn move_to_storage(
        &self,
        casket_id: String,
        item_id: String,
    ) -> Result<(), String> {
        self.casket_move(casket_id, item_id, true).await
    }

    /// Takes one item out of a storage unit into the inventory.
    pub async fn move_from_storage(
        &self,
        casket_id: String,
        item_id: String,
    ) -> Result<(), String> {
        self.casket_move(casket_id, item_id, false).await
    }

    /// Renames a storage unit, waiting for the GC's `NameItem` acknowledgement.
    pub async fn rename_storage(&self, casket_id: String, name: String) -> Result<(), String> {
        let casket = parse_storage_id(&casket_id)?;
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Err("Enter a name for the storage unit.".to_owned());
        }
        let session = &*self.session;
        let inventory = &*self.inventory;
        session
            .with_connection(|_connection, generation| async move {
                let state = inventory.0.try_lock().map_err(|_| {
                    "An inventory or storage request is already in progress.".to_owned()
                })?;
                let slot = state
                    .as_ref()
                    .filter(|slot| slot.generation == generation)
                    .ok_or_else(|| "Connect to CS2 and load inventory first.".to_owned())?;
                storage_item_count(&slot.items, casket)?;
                let client = slot.client.as_ref().ok_or_else(|| {
                    "The CS2 attach failed. Sign out of Steam and sign back in before reconnecting."
                        .to_owned()
                })?;
                client
                    .rename_storage(casket, &name)
                    .await
                    .map_err(|error| error.to_string())
            })
            .await
            .map_err(|error| error.to_string())?
    }

    /// Submits a trade-up contract of exactly ten owned skins, all of the
    /// same GC rarity and StatTrak state (legacy `processTradeOrder`). The GC
    /// reports the result through inventory changes, so success here only
    /// means the request was accepted for sending.
    pub async fn craft_trade_up(&self, item_ids: Vec<String>) -> Result<(), String> {
        if item_ids.len() != 10 {
            return Err("Select ten items to trade up.".to_owned());
        }
        let ids: Vec<u64> = item_ids
            .iter()
            .map(|id| parse_storage_id(id).map_err(|_| "Invalid item ID.".to_owned()))
            .collect::<Result<_, _>>()?;
        let mut unique = ids.clone();
        unique.sort_unstable();
        unique.dedup();
        if unique.len() != ids.len() {
            return Err("Each item can only be used once.".to_owned());
        }
        let session = &*self.session;
        let inventory = &*self.inventory;
        let schema = self.schema_snapshot()?;
        session
            .with_connection(|_connection, generation| async move {
                let state = inventory.0.try_lock().map_err(|_| {
                    "An inventory or storage request is already in progress.".to_owned()
                })?;
                let slot = state
                    .as_ref()
                    .filter(|slot| slot.generation == generation)
                    .ok_or_else(|| "Connect to CS2 and load inventory first.".to_owned())?;
                let mut rarity = None;
                let mut stat_trak = None;
                for id in &ids {
                    let item = slot
                        .items
                        .iter()
                        .find(|item| item.id == *id)
                        .ok_or_else(|| "One of the selected items is not in the inventory.".to_owned())?;
                    let row = enrich_item(item.clone(), &schema);
                    if !valid_trade_up_input(&row) {
                        return Err(format!("{} cannot be used in a trade-up.", row.name));
                    }
                    if rarity.is_some_and(|rarity| rarity != item.rarity)
                        || stat_trak.is_some_and(|stat_trak| stat_trak != row.stat_trak)
                    {
                        return Err(
                            "All ten items must share the same rarity and StatTrak state."
                                .to_owned(),
                        );
                    }
                    rarity = Some(item.rarity);
                    stat_trak = Some(row.stat_trak);
                }
                let client = slot.client.as_ref().ok_or_else(|| {
                    "The CS2 attach failed. Sign out of Steam and sign back in before reconnecting."
                        .to_owned()
                })?;
                client
                    .craft(&ids, rarity.unwrap_or_default())
                    .await
                    .map_err(|error| match error {
                        InventoryError::InvalidRequest => "Invalid trade-up request.".to_owned(),
                        error => error.to_string(),
                    })
            })
            .await
            .map_err(|error| error.to_string())?
    }

    /// Opens a container with its key (legacy `openContainer`, sent from the
    /// trade-up inventory). Both must be owned, tradeable, and the key must
    /// match the container; the unlocked item arrives through inventory
    /// changes, so success only means the request was sent.
    pub async fn open_container(
        &self,
        container_id: String,
        key_id: String,
    ) -> Result<(), String> {
        if container_id == key_id {
            return Err("Pick a key for the container.".to_owned());
        }
        let container = parse_storage_id(&container_id)?;
        let key = parse_storage_id(&key_id).map_err(|_| "Invalid item ID.".to_owned())?;
        let session = &*self.session;
        let inventory = &*self.inventory;
        let schema = self.schema_snapshot()?;
        session
            .with_connection(|_connection, generation| async move {
                let state = inventory.0.try_lock().map_err(|_| {
                    "An inventory or storage request is already in progress.".to_owned()
                })?;
                let slot = state
                    .as_ref()
                    .filter(|slot| slot.generation == generation)
                    .ok_or_else(|| "Connect to CS2 and load inventory first.".to_owned())?;
                let find = |id: u64, what: &str| {
                    slot.items
                        .iter()
                        .find(|item| item.id == id)
                        .map(|item| enrich_item(item.clone(), &schema))
                        .ok_or_else(|| format!("That {what} is not in the current inventory."))
                };
                let container_row = find(container, "container")?;
                let key_row = find(key, "key")?;
                if container_row.is_storage_unit {
                    return Err("Storage units cannot be opened.".to_owned());
                }
                if !valid_container(&container_row) {
                    return Err(format!("{} is not a container.", container_row.name));
                }
                if !valid_key(&key_row) {
                    return Err(format!("{} is not a key.", key_row.name));
                }
                if !key_row.moveable || !container_row.moveable {
                    return Err("That item cannot be used right now.".to_owned());
                }
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as i64)
                    .unwrap_or(0);
                if container_row.trade_unlock.is_some_and(|unlock| unlock > now)
                    || key_row.trade_unlock.is_some_and(|unlock| unlock > now)
                {
                    return Err("That container or key is still on trade hold.".to_owned());
                }
                let client = slot.client.as_ref().ok_or_else(|| {
                    "The CS2 attach failed. Sign out of Steam and sign back in before reconnecting."
                        .to_owned()
                })?;
                client
                    .open_container(&[container, key])
                    .await
                    .map_err(|error| match error {
                        InventoryError::InvalidRequest => {
                            "Invalid container request.".to_owned()
                        }
                        error => error.to_string(),
                    })
            })
            .await
            .map_err(|error| error.to_string())?
    }

    async fn casket_move(
        &self,
        casket_id: String,
        item_id: String,
        add: bool,
    ) -> Result<(), String> {
        let casket = parse_storage_id(&casket_id)?;
        let item_id = parse_storage_id(&item_id).map_err(|_| "Invalid item ID.".to_owned())?;
        let session = &*self.session;
        let inventory = &*self.inventory;
        let schema = self.schema_snapshot()?;
        session
            .with_connection(|_connection, generation| async move {
                let state = inventory.0.try_lock().map_err(|_| {
                    "An inventory or storage request is already in progress.".to_owned()
                })?;
                let slot = state
                    .as_ref()
                    .filter(|slot| slot.generation == generation)
                    .ok_or_else(|| "Connect to CS2 and load inventory first.".to_owned())?;
                let count = storage_item_count(&slot.items, casket)?;
                if add && count as usize >= STORAGE_CAPACITY {
                    return Err("That storage unit is full (1000 items).".to_owned());
                }
                let item = slot
                    .items
                    .iter()
                    .find(|item| item.id == item_id)
                    .ok_or_else(|| {
                        if add {
                            "That item is not in the current inventory.".to_owned()
                        } else {
                            "That item is not in this storage unit.".to_owned()
                        }
                    })?;
                if add {
                    if item.definition_index == 1201 {
                        return Err(
                            "Storage units cannot be put inside another storage unit.".to_owned()
                        );
                    }
                    if !enrich_item(item.clone(), &schema).moveable {
                        return Err("That item cannot be moved.".to_owned());
                    }
                }
                let client = slot.client.as_ref().ok_or_else(|| {
                    "The CS2 attach failed. Sign out of Steam and sign back in before reconnecting."
                        .to_owned()
                })?;
                let result = if add {
                    client.add_to_storage(casket, item_id).await
                } else {
                    client.remove_from_storage(casket, item_id).await
                };
                result.map_err(|error| match error {
                    InventoryError::InvalidRequest => "Invalid storage-unit request.".to_owned(),
                    error => error.to_string(),
                })
            })
            .await
            .map_err(|error| error.to_string())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_cs2::ItemAttribute;

    fn item() -> EconItem {
        EconItem {
            id: u64::MAX - 1,
            definition_index: 7,
            inventory_position: Some(1),
            quantity: 1,
            quality: 0,
            rarity: 5,
            custom_name: Some("Nametag".into()),
            origin: None,
            flags: None,
            equipped_classes: Vec::new(),
            music_index: None,
            tradeable_after_secs: None,
            attributes: vec![ItemAttribute {
                definition_index: 8,
                value: 0.1f32.to_le_bytes().to_vec(),
            }],
        }
    }

    #[test]
    fn enriches_real_schema_names_and_preserves_ids_as_json_strings() {
        let converted = enrich_item(item(), &ItemSchema::bundled().unwrap());
        assert_eq!(converted.name, "AK-47");
        assert_eq!(converted.wear_name.as_deref(), Some("Minimal Wear"));
        assert_eq!(converted.custom_name.as_deref(), Some("Nametag"));
        let json = serde_json::to_value(converted).unwrap();
        assert_eq!(json["id"], (u64::MAX - 1).to_string());
    }

    #[test]
    fn incremental_changes_replace_add_remove_and_keep_position_order() {
        let mut items = vec![item()];
        let mut changed = item();
        changed.quantity = 5;
        assert_eq!(
            apply_inventory_change(&mut items, InventoryChange::Upsert(changed.clone())),
            Some((InventoryChangeReason::ItemChanged, changed))
        );
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].quantity, 5);

        let mut acquired = item();
        acquired.id = 42;
        acquired.inventory_position = Some(0);
        assert_eq!(
            apply_inventory_change(&mut items, InventoryChange::Upsert(acquired.clone())),
            Some((InventoryChangeReason::ItemAcquired, acquired))
        );
        assert_eq!(
            items.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![42, u64::MAX - 1]
        );

        let expected_removed = items[0].clone();
        assert_eq!(
            apply_inventory_change(&mut items, InventoryChange::Remove(42)),
            Some((InventoryChangeReason::ItemRemoved, expected_removed))
        );
        assert!(apply_inventory_change(&mut items, InventoryChange::Remove(42)).is_none());
    }

    #[test]
    fn enriches_applied_stickers_collection_and_trade_up_metadata() {
        let schema = ItemSchema::from_json_with_collections(
            r##"{
                "items":{"7":{"name":"weapon_ak47","item_name":"#AK"}},
                "paint_kits":{"44":{"name":"redline","description_tag":"#REDLINE"}},
                "sticker_kits":{"12":{"item_name":"#PATCH","patch_material":"test_patch"}}
            }"##,
            r##"{"ak":"\"AK-47\"","redline":"\"Redline\"","patch":"\"Test Patch\""}"##,
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
        let mut source = item();
        source.attributes.extend([
            ItemAttribute {
                definition_index: 6,
                value: 44.0f32.to_le_bytes().to_vec(),
            },
            ItemAttribute {
                definition_index: 113,
                value: 12u32.to_le_bytes().to_vec(),
            },
            ItemAttribute {
                definition_index: 114,
                value: 0.25f32.to_le_bytes().to_vec(),
            },
            ItemAttribute {
                definition_index: 115,
                value: 1.0f32.to_le_bytes().to_vec(),
            },
            ItemAttribute {
                definition_index: 116,
                value: 0.0f32.to_le_bytes().to_vec(),
            },
            ItemAttribute {
                definition_index: 278,
                value: 0.1f32.to_le_bytes().to_vec(),
            },
            ItemAttribute {
                definition_index: 279,
                value: (-0.1f32).to_le_bytes().to_vec(),
            },
        ]);
        let row = enrich_item(source, &schema);
        assert_eq!(row.name, "AK-47 | Redline");
        assert_eq!(row.collection.as_deref(), Some("The Test Collection"));
        assert!(row.trade_up_eligible);
        assert_eq!(
            row.image_url.as_deref(),
            Some("https://media.steampowered.com/apps/730/test.png")
        );
        assert_eq!(row.stickers.len(), 1);
        assert_eq!(row.stickers[0].slot, 0);
        assert_eq!(row.stickers[0].name, "Test Patch");
        assert_eq!(row.stickers[0].kind, "Patch");
        assert_eq!(row.stickers[0].wear, Some(0.25));
        assert!((row.stickers[0].offset_x.unwrap() - 0.1).abs() < 1e-6);
        assert!((row.stickers[0].offset_y.unwrap() + 0.1).abs() < 1e-6);
    }

    #[test]
    fn music_kits_graffiti_and_casket_icons_are_enriched() {
        let schema = ItemSchema::from_json_with_collections(
            r##"{
                "items":{"7":{"name":"weapon_ak47","item_name":"#AK"}},
                "music_kits":{"4":{"loc_name":"#MUSIC","image_inventory":"econ/music/test"}},
                "graffiti_tints":{"bright_blue":{"id":9}},
                "casket_icons":{"3":{"icon_path":"econ/tools/casket_test"}}
            }"##,
            r##"{"ak":"\"AK-47\"","music":"\"Test Music\""}"##,
            "{}",
        )
        .unwrap();
        let mut music = item();
        music.music_index = Some(4);
        music.attributes.push(ItemAttribute {
            definition_index: 233,
            value: 9u32.to_le_bytes().to_vec(),
        });
        let row = enrich_item(music, &schema);
        assert_eq!(row.name, "Music Kit | Test Music (Bright Blue)");
        assert_eq!(row.music_kit.as_deref(), Some("Music Kit | Test Music"));
        assert_eq!(row.graffiti.as_deref(), Some("Bright Blue"));
        assert!(!row.trade_up_eligible);

        let mut casket = item();
        casket.definition_index = 1201;
        casket.attributes.push(ItemAttribute {
            definition_index: 70,
            value: 3u32.to_le_bytes().to_vec(),
        });
        assert_eq!(
            enrich_item(casket, &schema).image_path.as_deref(),
            Some("econ/tools/casket_test")
        );
    }

    #[test]
    fn standalone_stickers_graffiti_and_cases_use_their_own_image_ids() {
        use item_schema::ImageCatalogKind;

        let mut schema = ItemSchema::from_json_with_collections(
            r##"{
                "items":{
                    "1209":{"item_name":"#STICKER"},
                    "1348":{"item_name":"#GRAFFITI"},
                    "1349":{"item_name":"#UNSEALED"},
                    "4001":{"item_name":"#CASE"}
                },
                "sticker_kits":{"4001":{"item_name":"#KIT","sticker_material":"test"}},
                "music_kits":{"4001":{"loc_name":"#MUSIC"}},
                "graffiti_tints":{"brick_red":{"id":1},"battle_green":{"id":2}}
            }"##,
            r#"{"sticker":"Sticker","graffiti":"Sealed Graffiti","unsealed":"Graffiti",
                "case":"Test Case","kit":"Test Kit","music":"Test Music"}"#,
            "{}",
        )
        .unwrap();
        for (kind, json) in [
            (
                ImageCatalogKind::Definitions,
                r#"[{"def_index":"4001","image":"https://cdn.steamstatic.com/case.png"}]"#,
            ),
            (
                ImageCatalogKind::Stickers,
                r#"[{"def_index":"4001","image":"https://cdn.steamstatic.com/sticker.png"}]"#,
            ),
            (
                ImageCatalogKind::Graffiti,
                r#"[{"def_index":"4001","image":"https://cdn.steamstatic.com/plain.png"},
                    {"def_index":"4001","color_index":1,"image":"https://cdn.steamstatic.com/red.png"},
                    {"def_index":"4001","color_index":2,"image":"https://cdn.steamstatic.com/green.png"}]"#,
            ),
            (
                ImageCatalogKind::MusicKits,
                r#"[{"def_index":"4001","name":"Test Music","image":"https://cdn.steamstatic.com/music.png"}]"#,
            ),
        ] {
            schema.add_image_catalog(kind, json).unwrap();
        }
        for (definition, tint, expected) in [
            (1209, None, "sticker"),
            (1348, None, "plain"),
            (1348, Some(1u32), "red"),
            (1349, Some(2u32), "green"),
            (4001, None, "case"),
        ] {
            let mut source = item();
            source.definition_index = definition;
            source.attributes = vec![
                ItemAttribute {
                    definition_index: 6,
                    value: 0.0f32.to_le_bytes().to_vec(),
                },
                ItemAttribute {
                    definition_index: 113,
                    value: 4001u32.to_le_bytes().to_vec(),
                },
            ];
            if let Some(tint) = tint {
                source.attributes.push(ItemAttribute {
                    definition_index: 233,
                    value: tint.to_le_bytes().to_vec(),
                });
            }
            let row = enrich_item(source, &schema);
            assert_eq!(
                row.image_url,
                Some(format!("https://cdn.steamstatic.com/{expected}.png")),
                "definition {definition}, tint {tint:?}"
            );
            if definition == 1209 {
                assert_eq!(row.name, "Sticker | Test Kit");
            } else if definition == 4001 {
                assert_eq!(row.name, "Test Case");
            }
        }
        assert_eq!(
            schema.music_kit(4001).unwrap().image_url.as_deref(),
            Some("https://cdn.steamstatic.com/music.png")
        );
    }

    #[test]
    fn ignores_invalid_attribute_values_and_maps_legacy_wear_boundaries() {
        for (wear, expected) in [
            (0.0, "Factory New"),
            (0.07, "Factory New"),
            (0.15, "Minimal Wear"),
            (0.38, "Field-Tested"),
            (0.45, "Well-Worn"),
            (1.0, "Battle-Scarred"),
        ] {
            assert_eq!(wear_name(wear), expected);
        }
        let mut source = item();
        source.attributes[0].value = f32::NAN.to_le_bytes().to_vec();
        assert_eq!(
            enrich_item(source, &ItemSchema::bundled().unwrap()).wear,
            None
        );
        let mut source = item();
        source.attributes[0].value = vec![1];
        assert_eq!(
            enrich_item(source, &ItemSchema::bundled().unwrap()).wear,
            None
        );
    }

    #[test]
    fn old_cleanup_and_requests_cannot_remove_a_newer_gc_slot() {
        let mut slot = Some(GcSlot {
            generation: 3,
            client: Some(()),
            items: Vec::new(),
        });
        clear_generation(&mut slot, 1);
        assert!(slot.as_ref().unwrap().client.is_some());
        assert!(prepare_slot(&mut slot, 1).is_err());
        assert_eq!(slot.as_ref().unwrap().generation, 3);
        clear_generation(&mut slot, 3);
        assert!(slot.is_none());
    }

    #[test]
    fn failed_attach_is_not_recreated_on_the_same_steam_connection() {
        let mut slot: Option<GcSlot<()>> = None;
        assert!(prepare_slot(&mut slot, 1).unwrap());
        for _attempt in 0..10 {
            assert!(!prepare_slot(&mut slot, 1).unwrap());
        }
        assert!(prepare_slot(&mut slot, 3).unwrap());
    }

    #[test]
    fn storage_request_validates_ids_and_reads_count_attribute_as_little_endian() {
        assert_eq!(parse_storage_id("18446744073709551614"), Ok(u64::MAX - 1));
        for invalid in ["", "0", "01", "+1", " 1", "18446744073709551616"] {
            assert!(parse_storage_id(invalid).is_err(), "accepted {invalid:?}");
        }

        let mut storage = item();
        storage.id = 42;
        storage.definition_index = 1201;
        storage.attributes = vec![ItemAttribute {
            definition_index: 270,
            value: 0x0102_0304u32.to_le_bytes().to_vec(),
        }];
        assert_eq!(storage_item_count(&[storage.clone()], 42), Ok(0x0102_0304));
        assert!(storage_item_count(&[storage.clone()], 43).is_err());

        storage.attributes[0].value = vec![4, 3, 2];
        assert!(storage_item_count(&[storage], 42).is_err());
    }

    #[test]
    fn enriched_rows_carry_the_storage_unit_contents_count() {
        let schema = ItemSchema::bundled().unwrap();

        let mut storage = item();
        storage.definition_index = 1201;
        storage.attributes = vec![ItemAttribute {
            definition_index: 270,
            value: 7u32.to_le_bytes().to_vec(),
        }];
        assert_eq!(enrich_item(storage, &schema).storage_item_count, Some(7));

        // Regular items never expose the count, even with a stray attribute.
        let mut plain = item();
        plain.attributes = vec![ItemAttribute {
            definition_index: 270,
            value: 7u32.to_le_bytes().to_vec(),
        }];
        assert_eq!(enrich_item(plain, &schema).storage_item_count, None);
    }

    #[test]
    fn storage_gc_errors_map_to_actionable_ipc_messages() {
        assert_eq!(
            storage_contents_error(InventoryError::InvalidRequest),
            "Invalid storage-unit request."
        );
        assert_eq!(
            storage_contents_error(InventoryError::ContentsMismatch),
            "CS2 returned incomplete or inconsistent storage-unit contents."
        );
    }

    #[test]
    fn reward_previews_are_not_owned_items_and_invalid_markers_are_rejected() {
        let mut source = item();
        assert_eq!(is_owned_item(&source), Ok(true));
        source.attributes.push(ItemAttribute {
            definition_index: 277,
            value: 1u32.to_le_bytes().to_vec(),
        });
        assert_eq!(is_owned_item(&source), Ok(false));
        source.attributes.last_mut().unwrap().value = 0u32.to_le_bytes().to_vec();
        assert_eq!(is_owned_item(&source), Ok(true));
        source.attributes.last_mut().unwrap().value = vec![1];
        assert!(is_owned_item(&source).is_err());
    }

    #[test]
    fn casket_items_and_legacy_placeholders_are_hidden_from_the_main_inventory_only() {
        let schema = ItemSchema::bundled().unwrap();
        let mut contained = item();
        contained.id = 100;
        contained.attributes.push(ItemAttribute {
            definition_index: 272,
            value: vec![1, 0, 0, 0],
        });
        contained.attributes.push(ItemAttribute {
            definition_index: 273,
            value: vec![0, 0, 0, 0],
        });
        assert!(is_casket_contained(&contained));

        // One half alone is not a casket reference (globaloffensive requires both).
        let mut half = item();
        half.id = 101;
        half.attributes.push(ItemAttribute {
            definition_index: 272,
            value: vec![1, 0, 0, 0],
        });
        assert!(!is_casket_contained(&half));

        let mut placeholder = item();
        placeholder.id = EXCLUDED_ITEM_IDS[0];

        let main = to_rows(
            vec![contained.clone(), half.clone(), placeholder.clone()],
            &schema,
            false,
        )
        .unwrap();
        assert_eq!(
            main.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
            vec![(101u64).to_string()]
        );

        // The storage-contents view keeps casket contents (legacy isCasket),
        // but the placeholder IDs are dropped in every path.
        let storage = to_rows(vec![contained, placeholder], &schema, true).unwrap();
        assert_eq!(storage.len(), 1);
        assert_eq!(storage[0].id, (100u64).to_string());
    }

    #[test]
    fn stattrak_star_and_souvenir_prefixes_follow_legacy_order() {
        let schema = ItemSchema::bundled().unwrap();

        let mut stattrak = item();
        stattrak.attributes.push(ItemAttribute {
            definition_index: 80,
            value: vec![0],
        });
        let row = enrich_item(stattrak, &schema);
        assert!(row.stat_trak);
        assert_eq!(row.name, "StatTrak™ AK-47");

        let mut star = item();
        star.quality = 3;
        let row = enrich_item(star, &schema);
        assert_eq!(row.name, "★ AK-47");

        // Both: star outside, exactly like the legacy converter's ordering.
        let mut both = item();
        both.quality = 3;
        both.attributes.push(ItemAttribute {
            definition_index: 80,
            value: vec![0],
        });
        let row = enrich_item(both, &schema);
        assert_eq!(row.name, "★ StatTrak™ AK-47");

        let mut souvenir = item();
        souvenir.attributes.push(ItemAttribute {
            definition_index: 140,
            value: vec![1],
        });
        let row = enrich_item(souvenir, &schema);
        assert!(
            row.name.starts_with("Souvenir "),
            "expected Souvenir prefix, got {}",
            row.name
        );
    }

    #[test]
    fn classified_categories_come_from_the_econ_path() {
        let schema = ItemSchema::bundled().unwrap();

        // Painted weapon -> econ/default_generated/... -> Skins & Knives.
        // Attribute values are float32-encoded on the wire (legacy reads them
        // with readFloatLE).
        let mut painted = item();
        painted.attributes.push(ItemAttribute {
            definition_index: 6,
            value: 2.0f32.to_le_bytes().to_vec(),
        });
        let row = enrich_item(painted, &schema);
        assert_eq!(row.category.as_deref(), Some("Skins & Knives"));
        assert_eq!(row.major, None);

        // Plain weapon -> econ/weapons/base_weapons/... -> legacy leftover bucket.
        let row = enrich_item(item(), &schema);
        assert_eq!(row.category, None);
    }

    #[test]
    fn enriched_rows_use_the_legacy_position_semantics() {
        let schema = ItemSchema::bundled().unwrap();

        let mut flagged = item();
        flagged.inventory_position = Some((1 << 30) | 0x1234);
        assert_eq!(enrich_item(flagged, &schema).position, 0);

        let mut plain = item();
        plain.inventory_position = Some(0x00AA_000C);
        assert_eq!(enrich_item(plain, &schema).position, 0x000C);

        let mut missing = item();
        missing.inventory_position = None;
        assert_eq!(enrich_item(missing, &schema).position, 9999);
    }

    #[test]
    fn trade_unlock_only_survives_for_future_non_key_items() {
        let now = 1_800_000_000_000i64;
        // Future unlock passes, in raw seconds → ms.
        assert_eq!(
            trade_unlock_ms("AK-47", Some(1_800_008_640), now),
            Some(1_800_008_640_000)
        );
        // Past unlocks are dropped at conversion time (legacy compares to now).
        assert_eq!(trade_unlock_ms("AK-47", Some(1_700_000_000), now), None);
        // Keys never show a tradehold.
        assert_eq!(
            trade_unlock_ms("Weapon Case Key", Some(1_800_008_640), now),
            None
        );
        assert_eq!(trade_unlock_ms("AK-47", None, now), None);
    }

    #[test]
    fn moveable_follows_legacy_rules_and_overrides() {
        let schema = ItemSchema::bundled().unwrap();
        let skin_url = "econ/default_generated/weapon_ak47_bloodsport_light_large";
        let plain = item();

        // Skins are moveable; plain base weapons are not.
        assert!(compute_moveable(&schema, &plain, skin_url, "AK-47"));
        assert!(!compute_moveable(
            &schema,
            &plain,
            "econ/weapons/base_weapons/weapon_ak47",
            "AK-47"
        ));

        // The bundled schema marks storage units "cannot trade".
        let mut casket = item();
        casket.definition_index = 1201;
        assert!(!compute_moveable(
            &schema,
            &casket,
            "econ/tools/casket",
            "Storage Unit"
        ));

        // Legacy hard-coded exclusions.
        let mut flagged = item();
        flagged.definition_index = 987;
        assert!(!compute_moveable(&schema, &flagged, skin_url, "X"));
        assert!(!compute_moveable(
            &schema,
            &plain,
            "econ/status_icons/service_medal_2018",
            "Medal"
        ));
        let mut origin_zero = item();
        origin_zero.origin = Some(0);
        assert!(!compute_moveable(
            &schema,
            &origin_zero,
            "econ/status_icons/coin",
            "Coin"
        ));
        let mut keyed = item();
        keyed.flags = Some(10);
        assert!(!compute_moveable(
            &schema,
            &keyed,
            "econ/tools/weapon_case_key_crate_key",
            "Key"
        ));

        // Overrides: star quality forces moveable; promotional pin (origin 5)
        // and music kit (origin 0) are forced unmoveable.
        let mut star = item();
        star.definition_index = 987;
        star.quality = 3;
        assert!(compute_moveable(&schema, &star, skin_url, "X"));
        let mut pin = item();
        pin.origin = Some(5);
        assert!(!compute_moveable(&schema, &pin, skin_url, "Pin - Katowice"));
        let mut music = item();
        music.origin = Some(0);
        music.music_index = Some(3);
        assert!(!compute_moveable(&schema, &music, skin_url, "Music Kit"));
    }

    #[test]
    fn prices_are_only_looked_up_for_moveable_items() {
        let schema = ItemSchema::bundled().unwrap();

        // Painted skins are moveable and priced from the bundled snapshot
        // (missing table entries resolve to Some(0.0), like legacy).
        let mut painted = item();
        painted.attributes.push(ItemAttribute {
            definition_index: 6,
            value: 2.0f32.to_le_bytes().to_vec(),
        });
        let row = enrich_item(painted, &schema);
        assert!(row.moveable);
        assert!(row.price.is_some(), "moveable rows must carry a price");

        // Plain base weapons are not moveable → legacy's handleItem skips
        // them entirely → no price at all.
        let row = enrich_item(item(), &schema);
        assert!(!row.moveable);
        assert_eq!(row.price, None);
    }

    #[test]
    fn equipped_classes_map_to_legacy_ct_and_t_flags() {
        let schema = ItemSchema::bundled().unwrap();
        let mut rifle = item();
        rifle.equipped_classes = vec![2];
        let row = enrich_item(rifle, &schema);
        assert!(row.equipped_t);
        assert!(!row.equipped_ct);

        let mut rifle = item();
        rifle.equipped_classes = vec![3, 1];
        let row = enrich_item(rifle, &schema);
        assert!(row.equipped_ct);
        assert!(!row.equipped_t);
    }
}
