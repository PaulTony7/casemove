//! CS2 Game Coordinator: session, SO cache, casket and item operations.

use protobuf::Message;
use protobuf::UnknownValueRef;
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::pin::pin;
use std::sync::Arc;
use std::time::Duration;
use steam_client::{AppsEvent, GCProtoHeader, SteamClient, SteamEvent, services::GCMessage};
use steam_vent::{Connection, ConnectionTrait, GameCoordinator};
use steam_vent_proto_common::{RpcMessage, RpcMessageWithKind};
use steam_vent_proto_csgo::{
    GCHandshake,
    base_gcmessages::CSOEconItem,
    gcsdk_gcmessages::{CMsgClientHello, CMsgClientWelcome},
};
use steam_vent_proto_csgo::{
    econ_gcmessages::{CMsgCasketItem, CMsgGCItemCustomizationNotification, EGCItemMsg},
    gcsdk_gcmessages::{
        CMsgSOMultipleObjects, CMsgSOSingleObject, cmsg_somultiple_objects::SingleObject,
    },
    gcsystemmsgs::ESOMsg,
};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::{Stream, StreamExt};

const ECON_ITEM_TYPE: i32 = 1;
const MAX_ITEMS: usize = 100_000;
const MAX_ITEM_BYTES: usize = 256 * 1024;

fn client_hello() -> CMsgClientHello {
    CMsgClientHello {
        version: Some(2_000_244),
        client_session_need: Some(0),
        client_launcher: Some(0),
        steam_launcher: Some(0),
        ..Default::default()
    }
}

#[cfg(test)]
mod handshake_tests {
    use super::*;

    #[test]
    fn hello_matches_the_legacy_cs2_protocol_payload() {
        let encoded = client_hello().write_to_bytes().unwrap();
        let hello = CMsgClientHello::parse_from_bytes(&encoded).unwrap();
        assert_eq!(hello.version, Some(2_000_244));
        assert_eq!(hello.client_session_need, Some(0));
        assert_eq!(hello.client_launcher, Some(0));
        assert_eq!(hello.steam_launcher, Some(0));
        assert!(!encoded.is_empty());
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ItemAttribute {
    pub definition_index: u32,
    pub value: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EconItem {
    pub id: u64,
    pub definition_index: u32,
    /// Raw `CSOEconItem.inventory` value; `None` when the field is absent.
    pub inventory_position: Option<u32>,
    pub quantity: u32,
    pub quality: u32,
    pub rarity: u32,
    pub custom_name: Option<String>,
    pub attributes: Vec<ItemAttribute>,
}

/// Legacy display/sort position, matching globaloffensive's
/// `_processSOEconItem` (`(inventory >>> 30 & 1) ? 0 : inventory & 0xFFFF`)
/// and the converter's default of 9999 when no position exists.
pub fn position_from_inventory(raw: Option<u32>) -> u32 {
    match raw {
        None => 9999,
        Some(raw) if (raw >> 30) & 1 == 1 => 0,
        Some(raw) => raw & 0xFFFF,
    }
}

impl EconItem {
    pub fn legacy_position(&self) -> u32 {
        position_from_inventory(self.inventory_position)
    }
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
    #[error("The storage unit request was invalid.")]
    InvalidRequest,
    #[error("CS2 returned storage unit contents that do not match the expected item count.")]
    ContentsMismatch,
}

pub struct InventoryClient {
    coordinator: Option<GameCoordinator>,
    browser: Option<BrowserInventoryClient>,
}

struct BrowserInventoryClient {
    client: Arc<Mutex<SteamClient>>,
    messages: Mutex<mpsc::UnboundedReceiver<GCMessage>>,
    dispatcher: tokio::task::JoinHandle<()>,
}

impl Drop for BrowserInventoryClient {
    fn drop(&mut self) {
        self.dispatcher.abort();
    }
}

impl InventoryClient {
    pub async fn connect(connection: &Connection) -> Result<(Self, Vec<EconItem>), InventoryError> {
        let handshake = GCHandshake {
            hello: client_hello(),
        };
        let (coordinator, welcome) = tokio::time::timeout(
            Duration::from_secs(30),
            connection.game_coordinator(&handshake),
        )
        .await
        .map_err(|_| InventoryError::TimedOut)?
        .map_err(|_| InventoryError::ConnectionFailed)?;
        let inventory = parse_inventory(&welcome)?;
        Ok((
            Self {
                coordinator: Some(coordinator),
                browser: None,
            },
            inventory,
        ))
    }

    pub async fn connect_browser(
        client: Arc<Mutex<SteamClient>>,
    ) -> Result<(Self, Vec<EconItem>), InventoryError> {
        client
            .lock()
            .await
            .games_played(vec![730])
            .await
            .map_err(|_| InventoryError::ConnectionFailed)?;
        let (sender, receiver) = mpsc::unbounded_channel();
        let dispatcher_client = client.clone();
        let dispatcher = tokio::spawn(async move {
            loop {
                let result = dispatcher_client
                    .lock()
                    .await
                    .poll_event_timeout(Duration::from_millis(100))
                    .await;
                match result {
                    Ok(Some(SteamEvent::Apps(AppsEvent::GCReceived(message))))
                        if message.appid == 730 =>
                    {
                        if sender.send(message).is_err() {
                            break;
                        }
                    }
                    Ok(Some(_)) | Ok(None) => {}
                    Err(_) => break,
                }
            }
        });
        let browser = BrowserInventoryClient {
            client,
            messages: Mutex::new(receiver),
            dispatcher,
        };
        let inventory = match browser.refresh().await {
            Ok(items) => items,
            Err(error) => {
                drop(browser);
                return Err(error);
            }
        };
        Ok((
            Self {
                coordinator: None,
                browser: Some(browser),
            },
            inventory,
        ))
    }

    pub async fn refresh(&self) -> Result<Vec<EconItem>, InventoryError> {
        if let Some(browser) = &self.browser {
            return browser.refresh().await;
        }
        let coordinator = self
            .coordinator
            .as_ref()
            .ok_or(InventoryError::ConnectionFailed)?;
        let operation = async {
            let welcome = coordinator.one::<CMsgClientWelcome>();
            coordinator
                .send(client_hello())
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

    /// Loads the contents of a storage unit (casket) from the GC.
    ///
    /// The GC has no reply carrying the items: it sends them as SO objects and
    /// then an item-customization notification for the casket. The result is
    /// returned only when that notification arrived and the collected item count
    /// equals `expected_count` (attribute 270 of the storage unit). An
    /// `expected_count` of 0 is an empty unit and sends nothing.
    pub async fn storage_contents(
        &self,
        casket_id: u64,
        expected_count: u32,
    ) -> Result<Vec<EconItem>, InventoryError> {
        if let Some(browser) = &self.browser {
            return browser.storage_contents(casket_id, expected_count).await;
        }
        self.storage_contents_steam_vent(casket_id, expected_count)
            .await
    }

    async fn storage_contents_steam_vent(
        &self,
        casket_id: u64,
        expected_count: u32,
    ) -> Result<Vec<EconItem>, InventoryError> {
        if !validate_request(casket_id, expected_count)? {
            return Ok(Vec::new());
        }
        let coordinator = self
            .coordinator
            .as_ref()
            .ok_or(InventoryError::ConnectionFailed)?;
        let failed = |_| InventoryError::ConnectionFailed;
        // Subscribe before sending so no response can be missed.
        let create = coordinator
            .on::<SoCreate>()
            .map(move |m| m.map(|m| single_object_event(&m.0, false)).map_err(failed));
        let update = coordinator
            .on::<SoUpdate>()
            .map(move |m| m.map(|m| single_object_event(&m.0, false)).map_err(failed));
        let destroy = coordinator
            .on::<SoDestroy>()
            .map(move |m| m.map(|m| single_object_event(&m.0, true)).map_err(failed));
        let multiple = coordinator
            .on::<SoUpdateMultiple>()
            .map(move |m| m.map_err(failed).and_then(|m| multi_event(&m.0)));
        let notification = coordinator.on::<CasketNotification>().map(move |m| {
            m.map(|m| CasketEvent::Notification {
                item_ids: m.0.item_id.clone(),
                request: m.0.request(),
            })
            .map_err(failed)
        });
        let events = create
            .merge(update)
            .merge(destroy)
            .merge(multiple)
            .merge(notification);
        let request = CasketLoadRequest(CMsgCasketItem {
            casket_item_id: Some(casket_id),
            item_item_id: Some(casket_id),
            ..Default::default()
        });
        coordinator
            .send(request)
            .await
            .map_err(|_| InventoryError::ConnectionFailed)?;
        collect_contents(
            events,
            casket_id,
            expected_count as usize,
            CASKET_LOAD_TIMEOUT,
        )
        .await
    }
}

impl BrowserInventoryClient {
    async fn refresh(&self) -> Result<Vec<EconItem>, InventoryError> {
        let hello = client_hello();
        let payload = hello
            .write_to_bytes()
            .map_err(|_| InventoryError::ConnectionFailed)?;
        let mut messages = self.messages.lock().await;
        tokio::time::timeout(Duration::from_secs(30), async {
            self.client
                .lock()
                .await
                .send_to_gc_proto(730, 4006, &payload, GCProtoHeader::default())
                .await
                .map_err(|_| InventoryError::ConnectionFailed)?;
            while let Some(message) = messages.recv().await {
                if message.msg_type == 4004 {
                    let welcome = CMsgClientWelcome::parse_from_bytes(&message.payload)
                        .map_err(|_| InventoryError::ConnectionFailed)?;
                    return parse_inventory(&welcome);
                }
            }
            Err(InventoryError::ConnectionFailed)
        })
        .await
        .map_err(|_| InventoryError::TimedOut)?
    }

    async fn storage_contents(
        &self,
        casket_id: u64,
        expected_count: u32,
    ) -> Result<Vec<EconItem>, InventoryError> {
        if !validate_request(casket_id, expected_count)? {
            return Ok(Vec::new());
        }
        let request = CMsgCasketItem {
            casket_item_id: Some(casket_id),
            item_item_id: Some(casket_id),
            ..Default::default()
        };
        let payload = request
            .write_to_bytes()
            .map_err(|_| InventoryError::ConnectionFailed)?;
        let mut messages = self.messages.lock().await;
        self.client
            .lock()
            .await
            .send_to_gc_proto(
                730,
                EGCItemMsg::k_EMsgGCCasketItemLoadContents as i32 as u32,
                &payload,
                GCProtoHeader::default(),
            )
            .await
            .map_err(|_| InventoryError::ConnectionFailed)?;

        let mut collector = Collector {
            casket_id,
            expected: expected_count as usize,
            items: BTreeMap::new(),
            notified: false,
        };
        let result = tokio::time::timeout(CASKET_LOAD_TIMEOUT, async {
            while let Some(message) = messages.recv().await {
                if let Some(event) = browser_casket_event(message)? {
                    if collector.apply(event)? {
                        return Ok(());
                    }
                }
            }
            Err(InventoryError::ConnectionFailed)
        })
        .await;
        match result {
            Ok(result) => result?,
            Err(_) if collector.notified => return Err(InventoryError::ContentsMismatch),
            Err(_) => return Err(InventoryError::TimedOut),
        }
        let mut items: Vec<_> = collector.items.into_values().collect();
        items.sort_by_key(|item| (item.inventory_position.unwrap_or(u32::MAX), item.id));
        Ok(items)
    }
}

fn browser_casket_event(message: GCMessage) -> Result<Option<CasketEvent>, InventoryError> {
    let event = match message.msg_type {
        kind if kind == ESOMsg::k_ESOMsg_Create as i32 as u32 => {
            let object = CMsgSOSingleObject::parse_from_bytes(&message.payload)
                .map_err(|_| InventoryError::ConnectionFailed)?;
            Some(single_object_event(&object, false))
        }
        kind if kind == ESOMsg::k_ESOMsg_Update as i32 as u32 => {
            let object = CMsgSOSingleObject::parse_from_bytes(&message.payload)
                .map_err(|_| InventoryError::ConnectionFailed)?;
            Some(single_object_event(&object, false))
        }
        kind if kind == ESOMsg::k_ESOMsg_Destroy as i32 as u32 => {
            let object = CMsgSOSingleObject::parse_from_bytes(&message.payload)
                .map_err(|_| InventoryError::ConnectionFailed)?;
            Some(single_object_event(&object, true))
        }
        kind if kind == ESOMsg::k_ESOMsg_UpdateMultiple as i32 as u32 => {
            let objects = CMsgSOMultipleObjects::parse_from_bytes(&message.payload)
                .map_err(|_| InventoryError::ConnectionFailed)?;
            Some(multi_event(&objects)?)
        }
        kind if kind == EGCItemMsg::k_EMsgGCItemCustomizationNotification as i32 as u32 => {
            let notification =
                CMsgGCItemCustomizationNotification::parse_from_bytes(&message.payload)
                    .map_err(|_| InventoryError::ConnectionFailed)?;
            Some(CasketEvent::Notification {
                item_ids: notification.item_id.clone(),
                request: notification.request(),
            })
        }
        _ => None,
    };
    Ok(event)
}

fn parse_item(bytes: &[u8]) -> Result<EconItem, InventoryError> {
    if bytes.len() > MAX_ITEM_BYTES {
        return Err(InventoryError::PayloadLimit);
    }
    let item = CSOEconItem::parse_from_bytes(bytes).map_err(|_| InventoryError::InvalidItem)?;
    if item.id() == 0 || item.def_index() == 0 {
        return Err(InventoryError::InvalidItem);
    }
    Ok(EconItem {
        id: item.id(),
        definition_index: item.def_index(),
        inventory_position: item.inventory,
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
    })
}

fn removed_item_id(bytes: &[u8]) -> Result<u64, InventoryError> {
    if bytes.len() > MAX_ITEM_BYTES {
        return Err(InventoryError::PayloadLimit);
    }
    let item = CSOEconItem::parse_from_bytes(bytes).map_err(|_| InventoryError::InvalidItem)?;
    match item.id() {
        0 => Err(InventoryError::InvalidItem),
        id => Ok(id),
    }
}

fn attribute_u32(item: &EconItem, definition_index: u32) -> Result<Option<u32>, InventoryError> {
    let Some(attribute) = item
        .attributes
        .iter()
        .find(|attribute| attribute.definition_index == definition_index)
    else {
        return Ok(None);
    };
    <[u8; 4]>::try_from(attribute.value.as_slice())
        .map(|bytes| Some(u32::from_le_bytes(bytes)))
        .map_err(|_| InventoryError::InvalidItem)
}

fn containing_casket(item: &EconItem) -> Result<Option<u64>, InventoryError> {
    let low = attribute_u32(item, CASKET_ID_LOW_ATTRIBUTE)?;
    let high = attribute_u32(item, CASKET_ID_HIGH_ATTRIBUTE)?;
    Ok(match (low, high) {
        (Some(low), Some(high)) => Some(u64::from(high) << 32 | u64::from(low)),
        _ => None,
    })
}

/// Returns whether a GC request is needed (`false` for an empty unit).
fn validate_request(casket_id: u64, expected_count: u32) -> Result<bool, InventoryError> {
    if casket_id == 0 {
        return Err(InventoryError::InvalidRequest);
    }
    if expected_count as usize > MAX_ITEMS {
        return Err(InventoryError::PayloadLimit);
    }
    Ok(expected_count > 0)
}

struct Collector {
    casket_id: u64,
    expected: usize,
    items: BTreeMap<u64, EconItem>,
    notified: bool,
}

impl Collector {
    /// Returns `true` once the matching notification arrived and the collected
    /// items equal the expected count.
    fn apply(&mut self, event: CasketEvent) -> Result<bool, InventoryError> {
        match event {
            CasketEvent::Objects { upserts, removals } => {
                for bytes in &upserts {
                    let item = parse_item(bytes)?;
                    if containing_casket(&item)? == Some(self.casket_id) {
                        self.items.insert(item.id, item);
                    } else {
                        self.items.remove(&item.id);
                    }
                }
                for bytes in &removals {
                    self.items.remove(&removed_item_id(bytes)?);
                }
                if self.items.len() > MAX_ITEMS {
                    return Err(InventoryError::PayloadLimit);
                }
            }
            CasketEvent::Notification { item_ids, request } => {
                if item_ids.first() == Some(&self.casket_id)
                    && request == CASKET_CONTENTS_NOTIFICATION
                {
                    self.notified = true;
                }
            }
        }
        if !self.notified {
            return Ok(false);
        }
        match self.items.len().cmp(&self.expected) {
            std::cmp::Ordering::Equal => Ok(true),
            std::cmp::Ordering::Greater => Err(InventoryError::ContentsMismatch),
            std::cmp::Ordering::Less => Ok(false),
        }
    }
}

async fn collect_contents<S>(
    events: S,
    casket_id: u64,
    expected: usize,
    limit: Duration,
) -> Result<Vec<EconItem>, InventoryError>
where
    S: Stream<Item = Result<CasketEvent, InventoryError>>,
{
    let mut events = pin!(events);
    let mut collector = Collector {
        casket_id,
        expected,
        items: BTreeMap::new(),
        notified: false,
    };
    let run = async {
        while let Some(event) = events.next().await {
            if collector.apply(event?)? {
                return Ok(());
            }
        }
        Err(InventoryError::ConnectionFailed)
    };
    match tokio::time::timeout(limit, run).await {
        Ok(result) => result?,
        Err(_) if collector.notified => return Err(InventoryError::ContentsMismatch),
        Err(_) => return Err(InventoryError::TimedOut),
    }
    let mut items: Vec<_> = collector.items.into_values().collect();
    items.sort_by_key(|item| (item.inventory_position.unwrap_or(u32::MAX), item.id));
    Ok(items)
}

const CASKET_LOAD_TIMEOUT: Duration = Duration::from_secs(30);
/// `k_EGCItemCustomizationNotification_CasketContents`.
const CASKET_CONTENTS_NOTIFICATION: u32 = 1012;
/// Low and high 32 bits of the containing casket ID on an item.
const CASKET_ID_LOW_ATTRIBUTE: u32 = 272;
const CASKET_ID_HIGH_ATTRIBUTE: u32 = 273;

/// Local GC message wrappers with kinds; the proto crate provides
/// `RpcMessageWithKind` for none of these messages.
macro_rules! gc_message {
    ($name:ident, $inner:ty, $kind_enum:ty, $kind:expr) => {
        #[derive(Debug)]
        struct $name($inner);

        impl RpcMessage for $name {
            fn parse(reader: &mut dyn Read) -> protobuf::Result<Self> {
                <$inner as Message>::parse_from_reader(reader).map($name)
            }

            fn write(&self, writer: &mut dyn Write) -> protobuf::Result<()> {
                self.0.write_to_writer(writer)
            }

            fn encode_size(&self) -> usize {
                self.0.compute_size() as usize
            }
        }

        impl RpcMessageWithKind for $name {
            type KindEnum = $kind_enum;
            const KIND: Self::KindEnum = $kind;
        }
    };
}

gc_message!(
    CasketLoadRequest,
    CMsgCasketItem,
    EGCItemMsg,
    EGCItemMsg::k_EMsgGCCasketItemLoadContents
);
gc_message!(
    CasketNotification,
    CMsgGCItemCustomizationNotification,
    EGCItemMsg,
    EGCItemMsg::k_EMsgGCItemCustomizationNotification
);
gc_message!(
    SoCreate,
    CMsgSOSingleObject,
    ESOMsg,
    ESOMsg::k_ESOMsg_Create
);
gc_message!(
    SoUpdate,
    CMsgSOSingleObject,
    ESOMsg,
    ESOMsg::k_ESOMsg_Update
);
gc_message!(
    SoDestroy,
    CMsgSOSingleObject,
    ESOMsg,
    ESOMsg::k_ESOMsg_Destroy
);
gc_message!(
    SoUpdateMultiple,
    CMsgSOMultipleObjects,
    ESOMsg,
    ESOMsg::k_ESOMsg_UpdateMultiple
);

#[derive(Debug, PartialEq)]
enum CasketEvent {
    Objects {
        upserts: Vec<Vec<u8>>,
        removals: Vec<Vec<u8>>,
    },
    Notification {
        item_ids: Vec<u64>,
        request: u32,
    },
}

fn single_object_event(object: &CMsgSOSingleObject, removal: bool) -> CasketEvent {
    let mut upserts = Vec::new();
    let mut removals = Vec::new();
    if object.type_id() == ECON_ITEM_TYPE {
        let data = object.object_data().to_vec();
        if removal {
            removals.push(data);
        } else {
            upserts.push(data);
        }
    }
    CasketEvent::Objects { upserts, removals }
}

/// `objects_added` (4) and `objects_removed` (5) are missing from the bundled
/// proto, so they are read from the preserved unknown fields.
fn multi_event(message: &CMsgSOMultipleObjects) -> Result<CasketEvent, InventoryError> {
    let mut upserts = Vec::new();
    let mut removals = Vec::new();
    for (field, value) in message.special_fields.unknown_fields().iter() {
        if field != 4 && field != 5 {
            continue;
        }
        let UnknownValueRef::LengthDelimited(bytes) = value else {
            return Err(InventoryError::InvalidItem);
        };
        let object =
            SingleObject::parse_from_bytes(bytes).map_err(|_| InventoryError::InvalidItem)?;
        if object.type_id() != ECON_ITEM_TYPE {
            continue;
        }
        if field == 4 {
            upserts.push(object.object_data().to_vec());
        } else {
            removals.push(object.object_data().to_vec());
        }
    }
    upserts.extend(
        message
            .objects_modified
            .iter()
            .filter(|object| object.type_id() == ECON_ITEM_TYPE)
            .map(|object| object.object_data().to_vec()),
    );
    Ok(CasketEvent::Objects { upserts, removals })
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
                    inventory_position: item.inventory,
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
    items.sort_by_key(|item| (item.inventory_position.unwrap_or(u32::MAX), item.id));
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

    use steam_vent_proto_common::MsgKindEnum;
    use steam_vent_proto_csgo::base_gcmessages::CSOEconItemAttribute;

    const CASKET: u64 = u64::MAX - 5;

    fn attribute(definition_index: u32, value: Vec<u8>) -> CSOEconItemAttribute {
        CSOEconItemAttribute {
            def_index: Some(definition_index),
            value_bytes: Some(value),
            ..Default::default()
        }
    }

    fn casket_item(id: u64, casket: u64, position: u32) -> Vec<u8> {
        CSOEconItem {
            id: Some(id),
            def_index: Some(7),
            inventory: Some(position),
            attribute: vec![
                attribute(272, (casket as u32).to_le_bytes().to_vec()),
                attribute(273, ((casket >> 32) as u32).to_le_bytes().to_vec()),
            ],
            ..Default::default()
        }
        .write_to_bytes()
        .unwrap()
    }

    fn objects(upserts: Vec<Vec<u8>>) -> Result<CasketEvent, InventoryError> {
        Ok(CasketEvent::Objects {
            upserts,
            removals: vec![],
        })
    }

    fn notification(casket: u64, request: u32) -> Result<CasketEvent, InventoryError> {
        Ok(CasketEvent::Notification {
            item_ids: vec![casket, 1],
            request,
        })
    }

    async fn run(
        events: Vec<Result<CasketEvent, InventoryError>>,
        expected: usize,
    ) -> Result<Vec<EconItem>, InventoryError> {
        let events = tokio_stream::iter(events).chain(tokio_stream::pending());
        collect_contents(events, CASKET, expected, Duration::from_millis(50)).await
    }

    #[tokio::test]
    async fn returns_complete_contents_with_large_ids_in_stable_order() {
        let items = run(
            vec![
                objects(vec![casket_item(u64::MAX - 1, CASKET, 2)]),
                objects(vec![casket_item(42, CASKET, 1)]),
                notification(CASKET, CASKET_CONTENTS_NOTIFICATION),
            ],
            2,
        )
        .await
        .unwrap();
        assert_eq!(
            items.iter().map(|i| i.id).collect::<Vec<_>>(),
            [42, u64::MAX - 1]
        );
    }

    #[test]
    fn empty_unit_needs_no_request_and_bad_requests_are_rejected() {
        assert_eq!(validate_request(CASKET, 0), Ok(false));
        assert_eq!(validate_request(CASKET, 3), Ok(true));
        assert_eq!(validate_request(0, 3), Err(InventoryError::InvalidRequest));
        assert_eq!(
            validate_request(CASKET, MAX_ITEMS as u32 + 1),
            Err(InventoryError::PayloadLimit)
        );
    }

    #[tokio::test]
    async fn ignores_unrelated_notifications_and_items_but_waits_for_late_items() {
        let items = run(
            vec![
                notification(CASKET + 1, CASKET_CONTENTS_NOTIFICATION),
                notification(CASKET, CASKET_CONTENTS_NOTIFICATION + 1),
                objects(vec![
                    casket_item(5, CASKET + 1, 0),
                    casket_item(6, CASKET, 0),
                ]),
                notification(CASKET, CASKET_CONTENTS_NOTIFICATION),
                objects(vec![casket_item(7, CASKET, 1)]),
            ],
            2,
        )
        .await
        .unwrap();
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [6, 7]);
    }

    #[tokio::test]
    async fn never_completes_without_the_matching_notification() {
        let events = vec![
            objects(vec![casket_item(6, CASKET, 0)]),
            notification(CASKET + 1, CASKET_CONTENTS_NOTIFICATION),
        ];
        assert_eq!(run(events, 1).await, Err(InventoryError::TimedOut));
    }

    #[tokio::test]
    async fn rejects_partial_and_excess_contents() {
        let partial = vec![
            objects(vec![casket_item(6, CASKET, 0)]),
            notification(CASKET, CASKET_CONTENTS_NOTIFICATION),
        ];
        assert_eq!(run(partial, 2).await, Err(InventoryError::ContentsMismatch));
        let excess = vec![
            objects(vec![casket_item(6, CASKET, 0), casket_item(7, CASKET, 1)]),
            notification(CASKET, CASKET_CONTENTS_NOTIFICATION),
        ];
        assert_eq!(run(excess, 1).await, Err(InventoryError::ContentsMismatch));
    }

    #[tokio::test]
    async fn applies_removals_and_moves_out_of_the_unit() {
        let removal = CasketEvent::Objects {
            upserts: vec![casket_item(7, CASKET + 1, 0)],
            removals: vec![
                CSOEconItem {
                    id: Some(6),
                    ..Default::default()
                }
                .write_to_bytes()
                .unwrap(),
            ],
        };
        let items = run(
            vec![
                objects(vec![
                    casket_item(6, CASKET, 0),
                    casket_item(7, CASKET, 1),
                    casket_item(8, CASKET, 2),
                ]),
                Ok(removal),
                notification(CASKET, CASKET_CONTENTS_NOTIFICATION),
            ],
            1,
        )
        .await
        .unwrap();
        assert_eq!(items.iter().map(|i| i.id).collect::<Vec<_>>(), [8]);
    }

    #[tokio::test]
    async fn malformed_data_fails_instead_of_returning_partial_results() {
        let bad_attribute = CSOEconItem {
            id: Some(9),
            def_index: Some(7),
            attribute: vec![attribute(272, vec![1, 2]), attribute(273, vec![0; 4])],
            ..Default::default()
        }
        .write_to_bytes()
        .unwrap();
        for bad in [vec![0xff], vec![], bad_attribute] {
            let events = vec![
                objects(vec![casket_item(6, CASKET, 0)]),
                objects(vec![bad]),
                notification(CASKET, CASKET_CONTENTS_NOTIFICATION),
            ];
            assert_eq!(run(events, 1).await, Err(InventoryError::InvalidItem));
        }
        let removal = CasketEvent::Objects {
            upserts: vec![],
            removals: vec![vec![0xff]],
        };
        assert_eq!(
            run(vec![Ok(removal)], 1).await,
            Err(InventoryError::InvalidItem)
        );
    }

    #[tokio::test]
    async fn closed_or_failed_streams_are_failures() {
        let closed = tokio_stream::iter(vec![objects(vec![casket_item(6, CASKET, 0)])]);
        assert_eq!(
            collect_contents(closed, CASKET, 1, Duration::from_secs(1)).await,
            Err(InventoryError::ConnectionFailed)
        );
        let failed = vec![Err(InventoryError::ConnectionFailed)];
        assert_eq!(run(failed, 1).await, Err(InventoryError::ConnectionFailed));
    }

    #[test]
    fn reads_added_modified_and_removed_objects_from_multi_updates() {
        let object = |type_id, data: Vec<u8>| SingleObject {
            type_id: Some(type_id),
            object_data: Some(data),
            ..Default::default()
        };
        let encode = |object: SingleObject| object.write_to_bytes().unwrap();
        let mut message = CMsgSOMultipleObjects {
            objects_modified: vec![object(1, vec![2]), object(2, vec![9])],
            ..Default::default()
        };
        let fields = message.special_fields.mut_unknown_fields();
        fields.add_length_delimited(4, encode(object(1, vec![1])));
        fields.add_length_delimited(4, encode(object(3, vec![8])));
        fields.add_length_delimited(5, encode(object(1, vec![3])));
        assert_eq!(
            multi_event(&message),
            Ok(CasketEvent::Objects {
                upserts: vec![vec![1], vec![2]],
                removals: vec![vec![3]],
            })
        );
        message.special_fields.mut_unknown_fields().add_varint(5, 1);
        assert_eq!(multi_event(&message), Err(InventoryError::InvalidItem));
    }

    #[test]
    fn wire_messages_use_the_gc_kinds_from_the_legacy_protocol() {
        assert_eq!(<SoCreate as RpcMessageWithKind>::KIND.enum_value(), 21);
        assert_eq!(<SoUpdate as RpcMessageWithKind>::KIND.enum_value(), 22);
        assert_eq!(<SoDestroy as RpcMessageWithKind>::KIND.enum_value(), 23);
        assert_eq!(
            <SoUpdateMultiple as RpcMessageWithKind>::KIND.enum_value(),
            26
        );
        assert_eq!(
            <CasketNotification as RpcMessageWithKind>::KIND.enum_value(),
            1090
        );
        assert_eq!(
            <CasketLoadRequest as RpcMessageWithKind>::KIND.enum_value(),
            1094
        );
        let request = CasketLoadRequest(CMsgCasketItem {
            casket_item_id: Some(CASKET),
            item_item_id: Some(CASKET),
            ..Default::default()
        });
        let mut bytes = Vec::new();
        request.write(&mut bytes).unwrap();
        assert_eq!(request.encode_size(), bytes.len());
        let parsed = CMsgCasketItem::parse_from_bytes(&bytes).unwrap();
        assert_eq!(
            (parsed.casket_item_id(), parsed.item_item_id()),
            (CASKET, CASKET)
        );
    }

    #[test]
    fn legacy_position_masks_raw_inventory_like_globaloffensive() {
        // Golden vectors from globaloffensive's _processSOEconItem plus the
        // converter's 9999 default for a missing position.
        assert_eq!(position_from_inventory(None), 9999);
        // "new" flag (bit 30) forces position 0
        assert_eq!(position_from_inventory(Some((1 << 30) | 0x1234)), 0);
        assert_eq!(position_from_inventory(Some(1 << 30)), 0);
        // plain positions pass through
        assert_eq!(position_from_inventory(Some(0)), 0);
        assert_eq!(position_from_inventory(Some(42)), 42);
        // everything above the low 16 bits is masked off
        assert_eq!(position_from_inventory(Some(0x00FF_0041)), 0x41);
        assert_eq!(position_from_inventory(Some(0x3FFF_FFFF)), 0xFFFF);
        // a raw value without the new flag but with high bits set
        assert_eq!(position_from_inventory(Some(0x2000_0007)), 7);
    }

    #[test]
    fn parsed_items_keep_raw_inventory_and_compute_legacy_position() {
        let inventory = parse_inventory(&welcome(vec![
            item(42, (1 << 30) | 9), // new-flagged
            item(43, 0x10005),
        ]))
        .unwrap();
        let flagged = inventory.iter().find(|item| item.id == 42).unwrap();
        assert_eq!(flagged.inventory_position, Some((1 << 30) | 9));
        assert_eq!(flagged.legacy_position(), 0);
        let plain = inventory.iter().find(|item| item.id == 43).unwrap();
        assert_eq!(plain.legacy_position(), 5);

        // An item without the inventory field keeps None -> 9999.
        let bytes = CSOEconItem {
            id: Some(44),
            def_index: Some(7),
            ..Default::default()
        }
        .write_to_bytes()
        .unwrap();
        let inventory = parse_inventory(&welcome(vec![bytes])).unwrap();
        assert_eq!(inventory[0].inventory_position, None);
        assert_eq!(inventory[0].legacy_position(), 9999);
    }
}
