//! Wallet balance updates pushed by the Steam CM
//! (`EMsg::k_EMsgClientWalletInfoUpdate`), mirroring node-steam-user's
//! `wallet` event (which divides the raw cents balance by 100).

use steam_vent::ConnectionTrait;
use steam_vent_proto_common::{
    RpcMessage, RpcMessageWithKind,
    protobuf::{self, Message},
};
use steam_vent_proto_steam::{
    enums_clientserver::EMsg, steammessages_clientserver::CMsgClientWalletInfoUpdate,
};
use tokio_stream::{Stream, StreamExt};

/// A wallet update in whole currency units, like node-steam-user's `wallet` event.
#[derive(Clone, Debug, PartialEq)]
pub struct WalletUpdate {
    pub has_wallet: bool,
    pub balance: f64,
    pub currency: String,
}

/// Local wrapper so the blanket `NetMessage` impl for `RpcMessageWithKind`
/// types applies (orphan rule prevents implementing the traits on the proto
/// type directly).
#[derive(Debug)]
pub struct WalletInfoUpdate(pub CMsgClientWalletInfoUpdate);

impl RpcMessage for WalletInfoUpdate {
    fn parse(reader: &mut dyn std::io::Read) -> protobuf::Result<Self> {
        Ok(Self(CMsgClientWalletInfoUpdate::parse_from_reader(reader)?))
    }

    fn write(&self, writer: &mut dyn std::io::Write) -> protobuf::Result<()> {
        self.0.write_to_writer(writer)
    }

    fn encode_size(&self) -> usize {
        self.0.compute_size() as usize
    }
}

impl RpcMessageWithKind for WalletInfoUpdate {
    type KindEnum = EMsg;
    const KIND: EMsg = EMsg::k_EMsgClientWalletInfoUpdate;
}

/// Steam's numeric currency codes (identical to legacy `pricing.tsx`).
pub fn currency_code(code: i32) -> &'static str {
    match code {
        1 => "USD",
        2 => "GBP",
        3 => "EUR",
        4 => "CHF",
        5 => "RUB",
        6 => "PLN",
        7 => "BRL",
        8 => "JPY",
        9 => "NOK",
        10 => "IDR",
        11 => "MYR",
        12 => "PHP",
        13 => "SGD",
        14 => "THB",
        15 => "VND",
        16 => "KRW",
        17 => "TRY",
        18 => "UAH",
        19 => "MXN",
        20 => "CAD",
        21 => "AUD",
        22 => "NZD",
        23 => "CNY",
        24 => "INR",
        25 => "CLP",
        26 => "PEN",
        27 => "COP",
        28 => "ZAR",
        29 => "HKD",
        30 => "TWD",
        31 => "SAR",
        32 => "AED",
        33 => "SEK",
        34 => "ARS",
        35 => "ILS",
        36 => "BYN",
        37 => "KZT",
        38 => "KWD",
        39 => "QAR",
        40 => "CRC",
        41 => "UYU",
        42 => "BGN",
        43 => "HRK",
        44 => "CZK",
        45 => "DKK",
        46 => "HUF",
        47 => "RON",
        _ => "USD",
    }
}

impl From<&CMsgClientWalletInfoUpdate> for WalletUpdate {
    fn from(message: &CMsgClientWalletInfoUpdate) -> Self {
        let cents = message.balance.or(message.balance64.map(|value| value as i32));
        Self {
            has_wallet: message.has_wallet.unwrap_or(false),
            balance: f64::from(cents.unwrap_or(0)) / 100.0,
            currency: currency_code(message.currency.unwrap_or(1)).to_owned(),
        }
    }
}

impl WalletInfoUpdate {
    pub fn into_update(self) -> WalletUpdate {
        WalletUpdate::from(&self.0)
    }
}

/// Subscribes to wallet updates, recovering any message that arrived before
/// the subscription: the CM pushes `ClientWalletInfoUpdate` right after
/// logon, and steam-vent buffers unmatched messages (32-slot ring) that late
/// subscriptions never see.
pub fn wallet_stream(
    connection: &steam_vent::Connection,
) -> std::pin::Pin<Box<dyn Stream<Item = WalletUpdate> + Send>> {
    let subscribed = connection
        .on::<WalletInfoUpdate>()
        .filter_map(|result| result.ok().map(WalletInfoUpdate::into_update));
    let buffered = buffered_wallet_updates(connection);
    Box::pin(tokio_stream::iter(buffered).chain(subscribed))
}

/// Drains the unprocessed buffer for wallet messages. The drain drops any
/// other buffered message; at enrichment time those are only post-logon
/// pushes nothing in this app listens to (GC traffic starts later, when the
/// inventory attaches).
fn buffered_wallet_updates(connection: &steam_vent::Connection) -> Vec<WalletUpdate> {
    connection
        .take_unprocessed()
        .into_iter()
        .filter(|raw| raw.kind == EMsg::k_EMsgClientWalletInfoUpdate)
        .filter_map(|raw| raw.into_message::<WalletInfoUpdate>().ok())
        .map(WalletInfoUpdate::into_update)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn converts_cents_and_currency_like_node_steam_user() {
        let mut message = CMsgClientWalletInfoUpdate::new();
        message.has_wallet = Some(true);
        message.balance = Some(1234);
        message.currency = Some(3);
        assert_eq!(
            WalletUpdate::from(&message),
            WalletUpdate {
                has_wallet: true,
                balance: 12.34,
                currency: "EUR".into(),
            }
        );
    }

    #[test]
    fn missing_wallet_info_is_neutral_and_unknown_currencies_fall_back() {
        let message = CMsgClientWalletInfoUpdate::new();
        let update = WalletUpdate::from(&message);
        assert!(!update.has_wallet);
        assert_eq!(update.balance, 0.0);
        assert_eq!(update.currency, "USD");

        let mut message = CMsgClientWalletInfoUpdate::new();
        message.currency = Some(9999);
        assert_eq!(WalletUpdate::from(&message).currency, "USD");
    }

    #[test]
    fn wallet_message_round_trips_through_the_wrapper() {
        let mut message = CMsgClientWalletInfoUpdate::new();
        message.has_wallet = Some(true);
        message.balance = Some(250);
        message.currency = Some(1);
        let wrapper = WalletInfoUpdate(message);

        let mut bytes = Vec::new();
        wrapper.write(&mut Cursor::new(&mut bytes)).unwrap();
        let decoded = WalletInfoUpdate::parse(&mut Cursor::new(&bytes)).unwrap();
        assert_eq!(decoded.into_update().balance, 2.50);
        assert_eq!(<WalletInfoUpdate as RpcMessageWithKind>::KIND, EMsg::k_EMsgClientWalletInfoUpdate);
    }

    #[test]
    fn every_legacy_currency_code_is_mapped() {
        for code in 1..=47 {
            let mapped = currency_code(code);
            assert_eq!(mapped.len(), 3, "code {code} unmapped");
            assert!(
                mapped.chars().all(|c| c.is_ascii_uppercase()),
                "code {code} produced {mapped}"
            );
        }
        assert_eq!(currency_code(2), "GBP");
        assert_eq!(currency_code(47), "RON");
        assert_eq!(currency_code(0), "USD");
    }
}
