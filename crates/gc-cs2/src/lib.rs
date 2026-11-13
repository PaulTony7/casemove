//! CS2 Game Coordinator: session, SO cache, casket and item operations.

use protobuf::Message;
use std::collections::BTreeMap;
use std::time::Duration;
use steam_vent::{Connection, ConnectionTrait, GameCoordinator};
use steam_vent_proto_csgo::{
    GCHandshake,
    base_gcmessages::CSOEconItem,
    gcsdk_gcmessages::{CMsgClientHello, CMsgClientWelcome},
};

const ECON_ITEM_TYPE: i32 = 1;
const MAX_ITEMS: usize = 100_000;
const MAX_ITEM_BYTES: usize = 256 * 1024;

#[derive(Clone, Debug, PartialEq)]
pub struct ItemAttribute {
    pub definition_index: u32,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EconItem {
    pub id: u64,
    pub definition_index: u32,
    pub inventory_position: u32,
    pub quantity: u32,
    pub quality: u32,
    pub rarity: u32,
    pub custom_name: Option<String>,
    pub attributes: Vec<ItemAttribute>,
}

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum InventoryError {
    #[error("Could not connect to the CS2 Game Coordinator. Close CS2 and try again.")]
    ConnectionFailed,
    #[error("The CS2 Game Coordinator did not respond within 30 seconds. Try again.")]
    TimedOut,
    #[error("CS2 did not provide an inventory cache. Try connecting again.")]
    MissingCache,
    #[error("CS2 returned a malformed inventory item.")]
    InvalidItem,
    #[error("CS2 returned duplicate inventory item IDs.")]
    DuplicateItem,
    #[error("The CS2 inventory exceeded the supported payload limit.")]
    PayloadLimit,
}

pub struct InventoryClient {
    coordinator: GameCoordinator,
}

impl InventoryClient {
    pub async fn connect(connection: &Connection) -> Result<(Self, Vec<EconItem>), InventoryError> {
        let (coordinator, welcome) = tokio::time::timeout(
            Duration::from_secs(30),
            connection.game_coordinator(&GCHandshake::default()),
        )
        .await
        .map_err(|_| InventoryError::TimedOut)?
        .map_err(|_| InventoryError::ConnectionFailed)?;
        let inventory = parse_inventory(&welcome)?;
        Ok((Self { coordinator }, inventory))
    }

    pub async fn refresh(&self) -> Result<Vec<EconItem>, InventoryError> {
        let operation = async {
            let welcome = self.coordinator.one::<CMsgClientWelcome>();
            self.coordinator
                .send(CMsgClientHello::new())
                .await
                .map_err(|_| InventoryError::ConnectionFailed)?;
            let welcome = welcome
                .await
                .map_err(|_| InventoryError::ConnectionFailed)?;
            parse_inventory(&welcome)
        };
        tokio::time::timeout(Duration::from_secs(30), operation)
            .await
            .map_err(|_| InventoryError::TimedOut)?
    }
}

pub fn parse_inventory(welcome: &CMsgClientWelcome) -> Result<Vec<EconItem>, InventoryError> {
    if welcome.outofdate_subscribed_caches.is_empty() {
        return Err(InventoryError::MissingCache);
    }
    let mut items = BTreeMap::new();
    let mut has_inventory_cache = false;
    for cache in &welcome.outofdate_subscribed_caches {
        for object in &cache.objects {
            if object.type_id() != ECON_ITEM_TYPE {
                continue;
            }
            has_inventory_cache = true;
            for bytes in &object.object_data {
                if bytes.len() > MAX_ITEM_BYTES || items.len() >= MAX_ITEMS {
                    return Err(InventoryError::PayloadLimit);
                }
                let item = CSOEconItem::parse_from_bytes(bytes)
                    .map_err(|_| InventoryError::InvalidItem)?;
                if item.id() == 0 || item.def_index() == 0 {
                    return Err(InventoryError::InvalidItem);
                }
                let item = EconItem {
                    id: item.id(),
                    definition_index: item.def_index(),
                    inventory_position: item.inventory(),
                    quantity: item.quantity.unwrap_or(1),
                    quality: item.quality(),
                    rarity: item.rarity(),
                    custom_name: item.custom_name.filter(|name| !name.is_empty()),
                    attributes: item
                        .attribute
                        .iter()
                        .map(|attribute| ItemAttribute {
                            definition_index: attribute.def_index(),
                            value: attribute.value_bytes().to_vec(),
                        })
                        .collect(),
                };
                if items.insert(item.id, item).is_some() {
                    return Err(InventoryError::DuplicateItem);
                }
            }
        }
    }
    if !has_inventory_cache {
        return Err(InventoryError::MissingCache);
    }
    let mut items: Vec<_> = items.into_values().collect();
    items.sort_by_key(|item| (item.inventory_position, item.id));
    Ok(items)
}

#[cfg(test)]
mod tests {
    use super::*;
    use steam_vent_proto_csgo::gcsdk_gcmessages::{
        CMsgSOCacheSubscribed, cmsg_socache_subscribed::SubscribedType,
    };

    fn welcome(payloads: Vec<Vec<u8>>) -> CMsgClientWelcome {
        CMsgClientWelcome {
            outofdate_subscribed_caches: vec![CMsgSOCacheSubscribed {
                objects: vec![SubscribedType {
                    type_id: Some(ECON_ITEM_TYPE),
                    object_data: payloads,
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn item(id: u64, position: u32) -> Vec<u8> {
        CSOEconItem {
            id: Some(id),
            def_index: Some(7),
            inventory: Some(position),
            custom_name: Some("Test rifle".into()),
            ..Default::default()
        }
        .write_to_bytes()
        .unwrap()
    }

    #[test]
    fn parses_and_orders_real_protobuf_items_without_losing_large_ids() {
        let inventory =
            parse_inventory(&welcome(vec![item(u64::MAX - 1, 2), item(42, 1)])).unwrap();
        assert_eq!(inventory[0].id, 42);
        assert_eq!(inventory[1].id, u64::MAX - 1);
        assert_eq!(inventory[1].custom_name.as_deref(), Some("Test rifle"));
        assert_eq!(inventory[1].quantity, 1);
    }

    #[test]
    fn empty_inventory_is_distinct_from_a_missing_cache() {
        assert!(parse_inventory(&welcome(vec![])).unwrap().is_empty());
        assert_eq!(
            parse_inventory(&CMsgClientWelcome::new()),
            Err(InventoryError::MissingCache)
        );
    }

    #[test]
    fn rejects_corruption_missing_identity_and_duplicate_ids() {
        assert_eq!(
            parse_inventory(&welcome(vec![vec![0xff]])),
            Err(InventoryError::InvalidItem)
        );
        assert_eq!(
            parse_inventory(&welcome(vec![vec![]])),
            Err(InventoryError::InvalidItem)
        );
        assert_eq!(
            parse_inventory(&welcome(vec![item(1, 0), item(1, 1)])),
            Err(InventoryError::DuplicateItem)
        );
    }

    #[test]
    fn ignores_unrelated_cache_types_and_limits_item_payloads() {
        let mut message = welcome(vec![vec![0xff]]);
        message.outofdate_subscribed_caches[0].objects[0].type_id = Some(2);
        assert_eq!(parse_inventory(&message), Err(InventoryError::MissingCache));
        message.outofdate_subscribed_caches[0]
            .objects
            .push(SubscribedType {
                type_id: Some(ECON_ITEM_TYPE),
                ..Default::default()
            });
        assert!(parse_inventory(&message).unwrap().is_empty());
        assert_eq!(
            parse_inventory(&welcome(vec![vec![0; MAX_ITEM_BYTES + 1]])),
            Err(InventoryError::PayloadLimit)
        );
    }
}
