use app_commands::{InventoryItem, InventorySnapshot};
use gc_cs2::{EconItem, InventoryClient, InventoryError};
use item_schema::ItemSchema;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct InventoryState(Mutex<Option<GcSlot<InventoryClient>>>);

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

fn enrich_item(item: EconItem, schema: &ItemSchema) -> InventoryItem {
    let paint = attribute_integer(&item, 6);
    let wear = attribute_float(&item, 8)
        .map(f64::from)
        .filter(|wear| (0.0..=1.0).contains(wear));
    let paint_seed = attribute_integer(&item, 7);
    let description = schema.describe(item.definition_index, paint);
    InventoryItem {
        id: item.id.to_string(),
        definition_index: item.definition_index,
        name: description.name,
        custom_name: item.custom_name,
        image_url: description.image_url,
        quantity: item.quantity,
        rarity: item.rarity,
        position: item.inventory_position,
        wear,
        wear_name: wear.map(|wear| wear_name(wear).to_owned()),
        paint_seed,
        stat_trak: item
            .attributes
            .iter()
            .any(|attribute| attribute.definition_index == 80),
        is_storage_unit: item.definition_index == 1201,
    }
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

#[tauri::command]
pub async fn refresh_inventory(
    session: tauri::State<'_, crate::session::DesktopSession>,
    inventory: tauri::State<'_, InventoryState>,
    schema: tauri::State<'_, ItemSchema>,
) -> Result<InventorySnapshot, String> {
    let session = &*session;
    session
        .with_connection(|connection, generation| async move {
            let mut state = inventory
                .0
                .try_lock()
                .map_err(|_| "An inventory request is already in progress.".to_owned())?;
            let attach = session.in_generation(generation, || prepare_slot(&mut state, generation))
                .map_err(|error| error.to_string())??;
            let items = if attach {
                let connected = if let Some(browser_client) = connection.browser_client() {
                    InventoryClient::connect_browser(browser_client).await
                } else if let Some(steam_vent) = connection.steam_vent() {
                    InventoryClient::connect(steam_vent).await
                } else {
                    return Err("The Steam connection is unavailable.".into());
                };
                let (client, items) = connected
                    .map_err(|error| format!("{error} Sign out of Steam and sign back in before reconnecting."))?;
                session.in_generation(generation, || {
                    *state = Some(GcSlot { generation, client: Some(client), items: items.clone() });
                }).map_err(|error| error.to_string())?;
                items
            } else {
                let client = state.as_ref().and_then(|slot| slot.client.as_ref())
                    .ok_or_else(|| "The CS2 attach failed. Sign out of Steam and sign back in before reconnecting.".to_owned())?;
                let items = client.refresh().await.map_err(|error| error.to_string())?;
                session.in_generation(generation, || {
                    state.as_mut().filter(|slot| slot.generation == generation)
                        .map(|slot| slot.items = items.clone())
                }).map_err(|error| error.to_string())?
                    .ok_or_else(|| "The Steam session changed. Load inventory again.".to_owned())?;
                items
            };
            let mut rows = Vec::with_capacity(items.len());
            for item in items {
                if is_owned_item(&item)? {
                    rows.push(enrich_item(item, &schema));
                }
            }
            Ok(InventorySnapshot {
                steam_id: connection.steam_id(),
                items: rows,
            })
        })
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn storage_contents(
    casket_id: String,
    session: tauri::State<'_, crate::session::DesktopSession>,
    inventory: tauri::State<'_, InventoryState>,
    schema: tauri::State<'_, ItemSchema>,
) -> Result<Vec<InventoryItem>, String> {
    let casket_id = parse_storage_id(&casket_id)?;
    let schema = &*schema;
    let inventory = &*inventory;
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
            let mut rows = Vec::with_capacity(items.len());
            for item in items {
                if is_owned_item(&item)? {
                    rows.push(enrich_item(item, schema));
                }
            }
            Ok(rows)
        })
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use gc_cs2::ItemAttribute;

    fn item() -> EconItem {
        EconItem {
            id: u64::MAX - 1,
            definition_index: 7,
            inventory_position: 1,
            quantity: 1,
            quality: 0,
            rarity: 5,
            custom_name: Some("Nametag".into()),
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
}
