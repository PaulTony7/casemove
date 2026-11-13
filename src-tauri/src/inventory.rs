use app_commands::{InventoryItem, InventorySnapshot};
use gc_cs2::{EconItem, InventoryClient};
use item_schema::ItemSchema;
use tokio::sync::Mutex;

#[derive(Default)]
pub struct InventoryState(Mutex<Option<(u64, InventoryClient)>>);

impl InventoryState {
    pub async fn clear(&self) {
        self.0.lock().await.take();
    }
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
        image_url: description.image_path.map(|path| {
            format!("https://cdn.cloudflare.steamstatic.com/apps/730/icons/{path}.png")
        }),
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

#[tauri::command]
pub async fn refresh_inventory(
    session: tauri::State<'_, crate::session::DesktopSession>,
    inventory: tauri::State<'_, InventoryState>,
    schema: tauri::State<'_, ItemSchema>,
) -> Result<InventorySnapshot, String> {
    session
        .with_connection(|connection, generation| async move {
            let mut state = inventory
                .0
                .try_lock()
                .map_err(|_| "An inventory request is already in progress.".to_owned())?;
            if state
                .as_ref()
                .is_some_and(|(owner, _)| *owner != generation)
            {
                state.take();
            }
            let items = match state.as_ref() {
                Some((_, client)) => client.refresh().await.map_err(|error| error.to_string())?,
                None => {
                    let (client, items) = InventoryClient::connect(&connection)
                        .await
                        .map_err(|error| error.to_string())?;
                    *state = Some((generation, client));
                    items
                }
            };
            Ok(InventorySnapshot {
                steam_id: connection.steam_id().steam64().to_string(),
                items: items
                    .into_iter()
                    .map(|item| enrich_item(item, &schema))
                    .collect(),
            })
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
}
