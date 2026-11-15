//! Price table (bundled legacy snapshot), source resolution, live market
//! quotes, and USD formatting.

mod live;
pub use live::{
    live_key, parse_csfloat_prices, parse_priceoverview, parse_skinport_prices, LiveCache,
    Market, PriceOverview, MAX_AGE_MS,
};

use std::collections::HashMap;
use std::sync::OnceLock;

/// Bundled price snapshot (`data/prices.json`, the legacy `backup` copy): ~23k entries keyed
/// by `"Name (Wear)"` or the bare name, all values in USD.
const PRICES_JSON: &str =
    include_str!("../../../data/prices.json");

/// Price sources, named like legacy `settings.source.title` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceSource {
    Buff163,
    SteamListing,
    Skinport,
    Bitskins,
}

impl PriceSource {
    pub const ALL: [PriceSource; 4] = [
        Self::Buff163,
        Self::SteamListing,
        Self::Skinport,
        Self::Bitskins,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Buff163 => "buff163",
            Self::SteamListing => "steam_listing",
            Self::Skinport => "skinport",
            Self::Bitskins => "bitskins",
        }
    }
}

/// Resolved per-source prices for one item. `None` means the table has no
/// value for that source (legacy: `undefined` → the row renders unpriced);
/// a present entry with no market data resolves to `0.0` (legacy's
/// all-zero fallback dict).
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub buff163: Option<f64>,
    pub steam_listing: Option<f64>,
    pub skinport: Option<f64>,
    pub bitskins: Option<f64>,
}

impl Quote {
    fn zeros() -> Self {
        Self {
            buff163: Some(0.0),
            steam_listing: Some(0.0),
            skinport: Some(0.0),
            bitskins: Some(0.0),
        }
    }

    pub fn get(&self, source: PriceSource) -> Option<f64> {
        match source {
            PriceSource::Buff163 => self.buff163,
            PriceSource::SteamListing => self.steam_listing,
            PriceSource::Skinport => self.skinport,
            PriceSource::Bitskins => self.bitskins,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct SteamPrices {
    last_24h: Option<f64>,
    last_7d: Option<f64>,
    last_30d: Option<f64>,
    last_90d: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
struct BuffPrice {
    price: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
struct BuffPrices {
    starting_at: Option<BuffPrice>,
}

#[derive(Debug, serde::Deserialize)]
struct SkinportPrices {
    starting_at: Option<f64>,
}

#[derive(Debug, serde::Deserialize)]
struct Entry {
    steam: Option<SteamPrices>,
    buff163: Option<BuffPrices>,
    skinport: Option<SkinportPrices>,
}

impl Entry {
    /// Legacy `makeSinglerequest` source resolution: steam_listing starts at
    /// last_90d and each *truthy* (defined and non-zero) window overrides it
    /// in order 30d → 7d → 24h; afterwards a zeroed 7d combined with an
    /// expensive buff163 order substitutes buff×0.8.
    fn resolve(&self) -> Quote {
        let buff163 = self
            .buff163
            .as_ref()
            .and_then(|buff| buff.starting_at.as_ref())
            .and_then(|starting_at| starting_at.price);
        let mut steam_listing = self.steam.as_ref().and_then(|steam| steam.last_90d);
        if let Some(steam) = &self.steam {
            if steam.last_30d.is_some_and(|value| value != 0.0) {
                steam_listing = steam.last_30d;
            }
            if steam.last_7d.is_some_and(|value| value != 0.0) {
                steam_listing = steam.last_7d;
            }
            if steam.last_24h.is_some_and(|value| value != 0.0) {
                steam_listing = steam.last_24h;
            }
            if steam.last_7d == Some(0.0) && buff163.is_some_and(|value| value > 2000.0) {
                steam_listing = buff163.map(|value| value * 0.8);
            }
        }
        Quote {
            buff163,
            steam_listing,
            skinport: self
                .skinport
                .as_ref()
                .and_then(|skinport| skinport.starting_at),
            bitskins: Some(0.0),
        }
    }
}

pub struct PriceTable {
    entries: HashMap<String, Entry>,
}

impl PriceTable {
    /// The bundled snapshot, parsed once on first use (~5.7 MB).
    pub fn bundled() -> &'static PriceTable {
        static TABLE: OnceLock<PriceTable> = OnceLock::new();
        TABLE.get_or_init(|| PriceTable {
            entries: serde_json::from_str(PRICES_JSON)
                .expect("the bundled price snapshot parses"),
        })
    }

    /// Legacy key handling: wear-less items replace `(Holo/Foil)` with
    /// `(Holo-Foil)`; worn items try `"Name (Wear)"` first and fall back to
    /// the bare name. Missing entries price as all-zero.
    pub fn quote(&self, name: &str, wear: Option<&str>) -> Quote {
        let entry = match wear {
            Some(wear) => {
                let wear_key = format!("{name} ({wear})");
                self.entries
                    .get(&wear_key)
                    .or_else(|| self.entries.get(name))
            }
            None => self
                .entries
                .get(&name.replace("(Holo/Foil)", "(Holo-Foil)")),
        };
        entry.map(Entry::resolve).unwrap_or_else(Quote::zeros)
    }

    pub fn price(&self, name: &str, wear: Option<&str>, source: PriceSource) -> Option<f64> {
        self.quote(name, wear).get(source)
    }
}

/// The display price for a row: the bundled snapshot quote for `source`,
/// overridden by a live quote from the matching market.
pub fn resolve(
    name: &str,
    wear: Option<&str>,
    source: PriceSource,
    live: Option<f64>,
) -> Option<f64> {
    let mut quote = PriceTable::bundled().quote(name, wear);
    if source == PriceSource::SteamListing
        && let Some(price) = live
    {
        quote.steam_listing = Some(price);
    }
    quote.get(source)
}

/// The same resolution, but a `market` override applies to whichever column
/// it belongs to: Steam Market quotes the listing price, Skinport and CSFloat
/// have no bundled column of their own and are live-only.
pub fn resolve_market(
    name: &str,
    wear: Option<&str>,
    source: PriceSource,
    market: Market,
    live: Option<f64>,
) -> Option<f64> {
    if market != Market::Steam {
        return live;
    }
    resolve(name, wear, source, live)
}

/// Formats USD the way legacy's `Intl.NumberFormat` did for the en-US
/// default: grouping separators, two decimals, `$` prefix.
pub fn format_usd(amount: f64) -> String {
    format_usd_with(amount, 2)
}

/// Whole-dollar formatting — the legacy overview cards pass
/// `maximumFractionDigits: 0`.
pub fn format_usd_whole(amount: f64) -> String {
    format_usd_with(amount, 0)
}

fn format_usd_with(amount: f64, decimals: usize) -> String {
    format_currency(&Currency::usd(), amount, decimals)
}

/// A selectable display currency: its ISO code, symbol, and locale used for
/// grouping (legacy `Intl.NumberFormat(settings.locale, {currency})`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Currency {
    pub code: String,
    pub symbol: String,
    pub locale: String,
}

impl Currency {
    pub fn usd() -> Self {
        Self {
            code: "USD".into(),
            symbol: "$".into(),
            locale: "en-US".into(),
        }
    }

    /// Builds a currency from its ISO code, falling back to `code` as the
    /// symbol for codes without a known sign.
    pub fn new(code: &str) -> Self {
        let code = code.trim().to_uppercase();
        // Legacy `Intl.NumberFormat(settings.locale, {style: 'currency'})`
        // grouped with the user's locale; without a stored locale it used
        // en-US, so the code itself stands in for the symbol.
        let locale = "en-US";
        let symbol = if code == "USD" {
            "$".to_owned()
        } else {
            code.clone()
        };
        Self {
            symbol,
            code,
            locale: locale.to_owned(),
        }
    }

    fn symbol(&self) -> &str {
        if self.symbol.is_empty() {
            "$"
        } else {
            &self.symbol
        }
    }

    /// Thousands separator for the currency's locale (legacy en-GB/en-US
    /// group with `,`; European locales with `.`).
    fn thousands(&self) -> char {
        if self.locale.ends_with("-FR")
            || self.locale.ends_with("-DE")
            || self.locale.ends_with("-ES")
            || self.locale.ends_with("-IT")
            || self.locale.ends_with("-NL")
            || self.locale.ends_with("-PT")
            || self.locale.ends_with("-BE")
            || self.locale.ends_with("-AT")
        {
            '.'
        } else {
            ','
        }
    }

    fn decimal(&self) -> char {
        if self.thousands() == '.' { ',' } else { '.' }
    }
}

/// Codes offered by the legacy settings page with the symbol and locale the
/// formatter used for each. `USD` is the identity (no conversion).
const CURRENCIES: &[(&str, &str, &str)] = &[
    ("USD", "$", "en-US"),
    ("EUR", "€", "de-DE"),
    ("GBP", "£", "en-GB"),
    ("JPY", "¥", "ja-JP"),
    ("CNY", "¥", "zh-CN"),
    ("KRW", "₩", "ko-KR"),
    ("RUB", "₽", "ru-RU"),
    ("BRL", "R$", "pt-BR"),
    ("INR", "₹", "en-IN"),
    ("AUD", "A$", "en-AU"),
    ("CAD", "CA$", "en-CA"),
    ("CHF", "CHF ", "de-CH"),
    ("SEK", "kr ", "sv-SE"),
    ("NOK", "kr ", "nb-NO"),
    ("DKK", "kr ", "da-DK"),
    ("PLN", "zł ", "pl-PL"),
    ("TRY", "₺", "tr-TR"),
    ("UAH", "₴", "uk-UA"),
    ("MXN", "MX$", "es-MX"),
    ("NZD", "NZ$", "en-NZ"),
];

/// The currencies the settings page shows first, with their symbols, in the
/// order they are listed there.
pub fn currency_common() -> Vec<(&'static str, &'static str)> {
    CURRENCIES
        .iter()
        .map(|(code, symbol, ..)| (*code, symbol.trim()))
        .collect()
}

/// Currency codes the settings page lists (legacy `currencyCode`, pruned to
/// those the bundled rate table covers).
pub fn currency_codes() -> Vec<&'static str> {
    CURRENCIES
        .iter()
        .map(|(code, ..)| *code)
        .chain(backup_rates().keys().map(String::as_str))
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Legacy USD→currency rates in [USD, rate] pairs.
fn backup_rates() -> &'static std::collections::BTreeMap<String, f64> {
    static RATES: OnceLock<std::collections::BTreeMap<String, f64>> = OnceLock::new();
    RATES.get_or_init(|| {
        #[derive(serde::Deserialize)]
        struct Backup {
            rates: std::collections::BTreeMap<String, f64>,
        }
        serde_json::from_str::<Backup>(BACKUP_CURRENCY_JSON)
            .map(|backup| backup.rates)
            .unwrap_or_default()
    })
}

/// Bundled offline rates (`data/currency.json`), used whenever a
/// live rate has not been fetched yet.
const BACKUP_CURRENCY_JSON: &str =
    include_str!("../../../data/currency.json");

/// USD→`code` conversion rate: the live rate when available, otherwise the
/// bundled backup, otherwise `None` (no conversion; legacy resolved
/// `undefined` to `NaN` and Chromium formatted it as `$0.00`).
pub fn rate(code: &str) -> Option<f64> {
    let code = code.trim().to_uppercase();
    if code == "USD" {
        return Some(1.0);
    }
    backup_rates().get(&code).copied()
}

/// Converts a USD amount into `currency` and formats it with the currency's
/// symbol, grouping, and two decimals.
pub fn format_currency(currency: &Currency, amount: f64, decimals: usize) -> String {
    let converted = rate(&currency.code)
        .filter(|rate| rate.is_finite())
        .map(|rate| amount * rate)
        .unwrap_or(amount);
    let negative = converted < 0.0;
    let rounded = format!("{:.*}", decimals, converted.abs());
    let (whole, fraction) = rounded
        .split_once('.')
        .unwrap_or((&rounded, ""));
    let mut grouped = String::with_capacity(whole.len() + whole.len() / 3);
    let thousands = currency.thousands();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            grouped.push(thousands);
        }
        grouped.push(digit);
    }
    let sign = if negative { "-" } else { "" };
    if decimals == 0 {
        format!("{sign}{}{grouped}", currency.symbol())
    } else {
        format!(
            "{sign}{}{grouped}{}{fraction}",
            currency.symbol(),
            currency.decimal()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> &'static PriceTable {
        PriceTable::bundled()
    }

    #[test]
    fn bundled_snapshot_parses_and_prices_a_known_skin() {
        let quote = table().quote("AK-47 | Redline", Some("Field-Tested"));
        // The snapshot carries real data for this key; assert the resolved
        // steam priority chain produced a sane positive value.
        let steam = quote.get(PriceSource::SteamListing).unwrap();
        assert!(steam > 0.0, "expected a positive steam price, got {steam}");
        assert!(quote.get(PriceSource::Skinport).is_some());
        assert_eq!(quote.get(PriceSource::Bitskins), Some(0.0));
    }

    #[test]
    fn missing_entries_price_as_zero_not_missing() {
        let quote = table().quote("No Such Item Ever", None);
        assert_eq!(quote, Quote::zeros());
    }

    #[test]
    fn wear_key_falls_back_to_the_bare_name() {
        // "Sticker | v4lde | London 2018" has no wear variant in the table.
        let with_wear = table().quote("Sticker | v4lde | London 2018", Some("Factory New"));
        let bare = table().quote("Sticker | v4lde | London 2018", None);
        assert_eq!(with_wear, bare);
        assert!(bare.get(PriceSource::SteamListing).unwrap() > 0.0);
    }

    #[test]
    fn holo_foil_quirk_applies_to_wearless_items_only() {
        // Build synthetic tables so the quirk is tested without depending on
        // which holographic stickers the snapshot happens to contain.
        let entries = [(
            "Sticker | Example (Holo-Foil)".to_owned(),
            parse_entry(r#"{"steam":{"last_24h":1.5},"skinport":null,"buff163":null}"#),
        )]
        .into_iter()
        .collect();
        let table = PriceTable { entries };

        assert_eq!(
            table.quote("Sticker | Example (Holo/Foil)", None)
                .get(PriceSource::SteamListing),
            Some(1.5)
        );
        // With a wear name the legacy code uses the raw name, no replacement.
        assert_eq!(
            table
                .quote("Sticker | Example (Holo/Foil)", Some("Used"))
                .get(PriceSource::SteamListing),
            Some(0.0)
        );
    }

    fn parse_entry(json: &str) -> Entry {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn steam_source_priority_prefers_newest_truthy_window() {
        let entry = parse_entry(
            r#"{"steam":{"last_24h":1.0,"last_7d":2.0,"last_30d":3.0,"last_90d":4.0}}"#,
        );
        assert_eq!(entry.resolve().steam_listing, Some(1.0));

        // Zero windows are falsy in legacy and do not override.
        let entry = parse_entry(
            r#"{"steam":{"last_24h":0.0,"last_7d":0.0,"last_30d":3.0,"last_90d":4.0}}"#,
        );
        assert_eq!(entry.resolve().steam_listing, Some(3.0));

        let entry = parse_entry(
            r#"{"steam":{"last_30d":0.0,"last_90d":4.0}}"#,
        );
        assert_eq!(entry.resolve().steam_listing, Some(4.0));

        // Only nulls → the base window itself is missing.
        let entry = parse_entry(r#"{"steam":{"last_90d":null}}"#);
        assert_eq!(entry.resolve().steam_listing, None);
    }

    #[test]
    fn expensive_buff_substitutes_a_zeroed_steam_window() {
        let entry = parse_entry(
            r#"{"steam":{"last_24h":0.0,"last_7d":0.0,"last_30d":0.0,"last_90d":0.0},"buff163":{"starting_at":{"price":2500.0}}}"#,
        );
        assert_eq!(entry.resolve().steam_listing, Some(2000.0));

        // …but only when the buff price is actually above 2000.
        let entry = parse_entry(
            r#"{"steam":{"last_7d":0.0,"last_90d":0.0},"buff163":{"starting_at":{"price":100.0}}}"#,
        );
        assert_eq!(entry.resolve().steam_listing, Some(0.0));
    }

    #[test]
    fn format_currency_converts_and_uses_locale_grouping() {
        let eur = Currency::new("EUR");
        let text = format_currency(&eur, 1234.5, 2);
        // en-US grouping, ISO code in place of the symbol.
        assert!(text.starts_with("EUR"), "{text}");
        assert!(text.contains("1,173"), "{text}");
        assert_eq!(
            format_currency(&Currency::new("USD"), 1234.5, 0),
            "$1,234"
        );
        // The conversion itself tracks the bundled backup rate.
        let rate = rate("EUR").unwrap();
        let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
        let expected: String = format!("{:.2}", 1234.5 * rate)
            .chars()
            .filter(|c| c.is_ascii_digit())
            .collect();
        assert_eq!(digits, expected, "{text}");
    }

    #[test]
    fn unknown_currencies_fall_back_to_code_and_no_conversion() {
        let unknown = Currency::new("ZZZ");
        assert_eq!(rate("ZZZ"), None);
        assert_eq!(format_currency(&unknown, 1.5, 2), "ZZZ1.50");
        assert_eq!(rate("USD"), Some(1.0));
    }

    #[test]
    fn currency_codes_cover_the_settings_list_and_backup_rates() {
        let codes = currency_codes();
        for code in ["AUD", "EUR", "JPY", "USD"] {
            assert!(codes.contains(&code), "{code}");
        }
        assert!(codes.len() > 50);
    }

    #[test]
    fn common_currencies_lead_the_full_list() {
        let common = currency_common();
        let all = currency_codes();
        assert_eq!(common.first(), Some(&("USD", "$")));
        assert_eq!(common.len(), CURRENCIES.len());
        assert!(common.iter().all(|(code, _)| all.contains(code)));
    }

    #[test]
    fn source_selection_returns_each_market() {
        let entry = parse_entry(
            r#"{"steam":{"last_24h":12.5},"buff163":{"starting_at":{"price":11.0}},"skinport":{"starting_at":13.0}}"#,
        );
        let quote = entry.resolve();
        assert_eq!(quote.get(PriceSource::SteamListing), Some(12.5));
        assert_eq!(quote.get(PriceSource::Buff163), Some(11.0));
        assert_eq!(quote.get(PriceSource::Skinport), Some(13.0));
        assert_eq!(quote.get(PriceSource::Bitskins), Some(0.0));
        assert_eq!(PriceSource::ALL.len(), 4);
        assert_eq!(PriceSource::SteamListing.as_str(), "steam_listing");
    }

    #[test]
    fn format_usd_groups_thousands_and_keeps_cents() {
        assert_eq!(format_usd(0.0), "$0.00");
        assert_eq!(format_usd(12.345), "$12.35");
        assert_eq!(format_usd(1234.5), "$1,234.50");
        assert_eq!(format_usd(1234567.891), "$1,234,567.89");
        assert_eq!(format_usd(-42.5), "-$42.50");
    }

    #[test]
    fn format_usd_whole_rounds_like_the_overview_cards() {
        assert_eq!(format_usd_whole(123456.78), "$123,457");
        assert_eq!(format_usd_whole(0.0), "$0");
        assert_eq!(format_usd_whole(-42.6), "-$43");
        assert_eq!(format_usd_whole(999.4), "$999");
    }

    #[test]
    fn switching_markets_shows_that_markets_quote_or_nothing() {
        // The snapshot only has Steam values; the other markets are live-only,
        // so a row must not fall back to the Steam snapshot's number under a
        // different provider.
        let name = "AK-47 | Redline";
        let wear = Some("Field-Tested");
        let snapshot = table()
            .price(name, wear, PriceSource::SteamListing)
            .unwrap();
        assert!(snapshot > 0.0);

        // Steam without a live quote: the bundled snapshot.
        assert_eq!(
            resolve_market(name, wear, PriceSource::SteamListing, Market::Steam, None),
            Some(snapshot)
        );
        // Steam with one: the live quote.
        assert_eq!(
            resolve_market(name, wear, PriceSource::SteamListing, Market::Steam, Some(11.0)),
            Some(11.0)
        );
        // Skinport with one: that quote, never the Steam snapshot.
        assert_eq!(
            resolve_market(name, wear, PriceSource::SteamListing, Market::Skinport, Some(9.5)),
            Some(9.5)
        );
        // Skinport before its prices arrive: nothing, not a Steam figure.
        assert_eq!(
            resolve_market(name, wear, PriceSource::SteamListing, Market::Skinport, None),
            None
        );
        assert_eq!(
            resolve_market(name, wear, PriceSource::SteamListing, Market::Csfloat, None),
            None
        );
    }

    #[test]
    fn live_quotes_override_only_the_steam_source() {
        let snapshot = table()
            .price(
                "AK-47 | Redline",
                Some("Field-Tested"),
                PriceSource::SteamListing,
            )
            .unwrap();
        assert!(snapshot > 0.0);

        // A live quote wins over the (older) snapshot value.
        assert_eq!(
            resolve(
                "AK-47 | Redline",
                Some("Field-Tested"),
                PriceSource::SteamListing,
                Some(snapshot + 5.0)
            ),
            Some(snapshot + 5.0)
        );

        // …but other sources still come from the snapshot.
        assert_eq!(
            resolve(
                "AK-47 | Redline",
                Some("Field-Tested"),
                PriceSource::Skinport,
                Some(99.0)
            ),
            table().price(
                "AK-47 | Redline",
                Some("Field-Tested"),
                PriceSource::Skinport
            )
        );

        // A live quote also covers items the snapshot is missing entirely…
        assert_eq!(
            resolve("No Such Item Ever", None, PriceSource::SteamListing, Some(3.5)),
            Some(3.5)
        );
        // …and without one they price as zero, like the snapshot does.
        assert_eq!(
            resolve("No Such Item Ever", None, PriceSource::SteamListing, None),
            Some(0.0)
        );
    }
}
