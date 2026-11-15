//! Live market quotes: the disk cache, the `market_hash_name` key derivation,
//! and response parsing for Steam Market's `priceoverview` and the bulk
//! Skinport/CSFloat catalogues. The HTTP transport lives in `app-core`
//! (`fetch_market_prices`).

use std::collections::HashMap;
use std::path::Path;

/// How long a fetched quote counts as fresh; older entries are refetched on
/// the next inventory load but keep displaying until they are replaced.
pub const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;

/// A live price source. Each one prices by `market_hash_name`, like Steam.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Market {
    #[default]
    Steam,
    Skinport,
    Csfloat,
}

impl Market {
    pub const ALL: [Market; 3] = [Self::Steam, Self::Skinport, Self::Csfloat];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Steam => "steam",
            Self::Skinport => "skinport",
            Self::Csfloat => "csfloat",
        }
    }

    /// Parses a stored selector value; unknown names fall back to Steam.
    pub fn from_stored(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|market| market.as_str() == value)
            .unwrap_or_default()
    }

    /// The label the source picker shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Steam => "Steam Market",
            Self::Skinport => "Skinport",
            Self::Csfloat => "CSFloat",
        }
    }

    /// What the quoted figure actually is, so two sources are not compared
    /// blindly: Steam and Skinport quote a median sale price, CSFloat's bulk
    /// list only exposes its lowest active listing.
    pub fn basis(self) -> &'static str {
        match self {
            Self::Steam => "median sale price",
            Self::Skinport => "median sale price",
            Self::Csfloat => "lowest listing",
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Entry {
    price: f64,
    fetched_at_ms: i64,
}

/// Overlay of live quotes keyed by [`live_key`], persisted as JSON beside
/// the settings store. Each market keeps its own map so quotes for the same
/// item from different sources coexist; missing entries fall back to the
/// bundled snapshot.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LiveCache {
    /// Steam Market quotes. Older caches stored a flat `{key: entry}` map
    /// here, which is exactly this shape, so they still load.
    #[serde(default)]
    steam: HashMap<String, Entry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    skinport: HashMap<String, Entry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    csfloat: HashMap<String, Entry>,
}

impl LiveCache {
    /// The persisted cache; a missing or unreadable file starts empty. A file
    /// written before multiple markets existed stores a flat `entries` map,
    /// which is folded into the Steam Market quotes (see [`Self::from_json`]).
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| Self::from_json(&text))
            .unwrap_or_default()
    }

    /// Deserializes either cache layout. Only callers that must preserve old
    /// files need this; anything else can let serde handle it.
    fn from_json(text: &str) -> Option<Self> {
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PreMarket {
            #[serde(default)]
            entries: HashMap<String, Entry>,
        }
        // The market maps are all optional, so an old file would otherwise
        // deserialize into an empty cache and lose its quotes.
        #[derive(serde::Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Current {
            #[serde(default)]
            steam: HashMap<String, Entry>,
            #[serde(default)]
            skinport: HashMap<String, Entry>,
            #[serde(default)]
            csfloat: HashMap<String, Entry>,
        }
        if let Ok(cache) = serde_json::from_str::<Current>(text) {
            return Some(Self {
                steam: cache.steam,
                skinport: cache.skinport,
                csfloat: cache.csfloat,
            });
        }
        let old = serde_json::from_str::<PreMarket>(text).ok()?;
        Some(Self {
            steam: old.entries,
            ..Self::default()
        })
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    }

    fn entries(&self, market: Market) -> &HashMap<String, Entry> {
        match market {
            Market::Steam => &self.steam,
            Market::Skinport => &self.skinport,
            Market::Csfloat => &self.csfloat,
        }
    }

    fn entries_mut(&mut self, market: Market) -> &mut HashMap<String, Entry> {
        match market {
            Market::Steam => &mut self.steam,
            Market::Skinport => &mut self.skinport,
            Market::Csfloat => &mut self.csfloat,
        }
    }

    /// The cached `market` quote for `key`, however old — a stale live price
    /// still beats the bundled snapshot.
    pub fn price(&self, key: &str, market: Market) -> Option<f64> {
        self.entries(market).get(key).map(|entry| entry.price)
    }

    /// Whether `key` has a `market` quote fetched within [`MAX_AGE_MS`] of
    /// `now_ms`.
    pub fn fresh(&self, key: &str, market: Market, now_ms: i64) -> bool {
        self.entries(market)
            .get(key)
            .is_some_and(|entry| now_ms - entry.fetched_at_ms < MAX_AGE_MS)
    }

    pub fn insert(&mut self, key: &str, market: Market, price: f64, now_ms: i64) {
        self.entries_mut(market).insert(
            key.to_owned(),
            Entry {
                price,
                fetched_at_ms: now_ms,
            },
        );
    }
}

/// Steam's `market_hash_name` for a row: `Name (Wear)` when the item has a
/// wear name, otherwise the bare name. Unlike the snapshot's lookup key this
/// keeps `(Holo/Foil)` — that spelling is what Steam itself uses.
pub fn live_key(name: &str, wear: Option<&str>) -> String {
    match wear {
        Some(wear) => format!("{name} ({wear})"),
        None => name.to_owned(),
    }
}

/// What one `priceoverview` body says.
#[derive(Debug, PartialEq)]
pub enum PriceOverview {
    /// The median sale price, or the lowest listing when there is no median.
    Price(f64),
    /// `success: false` (or no usable figures) — nothing under this name.
    NotListed,
    /// Not a priceoverview document at all (throttle or block page).
    Unparseable,
}

/// Parses a Steam `priceoverview` response. `currency=1&country=US` makes
/// prices come back as USD strings like `"$1,234.56"`.
pub fn parse_priceoverview(body: &str) -> PriceOverview {
    #[derive(serde::Deserialize)]
    struct Overview {
        success: bool,
        median_price: Option<String>,
        lowest_price: Option<String>,
    }
    let Ok(overview) = serde_json::from_str::<Overview>(body) else {
        return PriceOverview::Unparseable;
    };
    if !overview.success {
        return PriceOverview::NotListed;
    }
    match overview
        .median_price
        .as_deref()
        .or(overview.lowest_price.as_deref())
        .and_then(parse_money)
    {
        Some(price) => PriceOverview::Price(price),
        None => PriceOverview::NotListed,
    }
}

/// `"$1,234.56"` → `1234.56`; anything without digits (localized
/// separators, `N/A`) fails to parse.
fn parse_money(text: &str) -> Option<f64> {
    let digits: String = text
        .chars()
        .filter(|character| character.is_ascii_digit() || *character == '.')
        .collect();
    digits.parse().ok()
}

/// Parses Skinport's `/v1/items` catalogue into `(market_hash_name, USD)`
/// pairs. Skinport quotes the median sale price in the requested currency;
/// entries without a usable figure are skipped rather than priced as zero.
pub fn parse_skinport_prices(body: &str) -> Vec<(String, f64)> {
    #[derive(serde::Deserialize)]
    struct Item {
        market_hash_name: String,
        median_price: Option<f64>,
        min_price: Option<f64>,
    }
    serde_json::from_str::<Vec<Item>>(body)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let price = item.median_price.or(item.min_price)?;
            (price > 0.0).then_some((item.market_hash_name, price))
        })
        .collect()
}

/// Parses CSFloat's `listings/price-list` into `(market_hash_name, USD)`
/// pairs. The endpoint reports `min_price` in **cents** and only lists items
/// that are currently on sale; anything already priced as Steam Market is
/// comparable once divided by 100.
pub fn parse_csfloat_prices(body: &str) -> Vec<(String, f64)> {
    #[derive(serde::Deserialize)]
    struct Item {
        market_hash_name: String,
        min_price: Option<u64>,
    }
    serde_json::from_str::<Vec<Item>>(body)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            let cents = item.min_price?;
            (cents > 0).then(|| (item.market_hash_name, cents as f64 / 100.0))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_median_price_with_a_lowest_listing_fallback() {
        let body = r#"{"success":true,"lowest_price":"$0.05","volume":"1,234","median_price":"$1,234.56"}"#;
        assert_eq!(parse_priceoverview(body), PriceOverview::Price(1234.56));

        let body = r#"{"success":true,"lowest_price":"$0.05"}"#;
        assert_eq!(parse_priceoverview(body), PriceOverview::Price(0.05));
    }

    #[test]
    fn success_false_means_the_market_has_no_listing() {
        assert_eq!(
            parse_priceoverview(r#"{"success":false}"#),
            PriceOverview::NotListed
        );
        // Listed but with no usable figure: also just "not listed".
        assert_eq!(
            parse_priceoverview(r#"{"success":true,"median_price":"N/A"}"#),
            PriceOverview::NotListed
        );
    }

    #[test]
    fn anything_that_is_not_a_priceoverview_document_is_unparseable() {
        assert_eq!(
            parse_priceoverview("<html>slow down</html>"),
            PriceOverview::Unparseable
        );
        assert_eq!(parse_priceoverview(""), PriceOverview::Unparseable);
        assert_eq!(
            parse_priceoverview(r#"{"error":"rate limited"}"#),
            PriceOverview::Unparseable
        );
    }

    #[test]
    fn live_key_is_the_market_hash_name() {
        assert_eq!(
            live_key("AK-47 | Redline", Some("Field-Tested")),
            "AK-47 | Redline (Field-Tested)"
        );
        assert_eq!(
            live_key("Operation Breakout Weapon Case", None),
            "Operation Breakout Weapon Case"
        );
        // Steam spells the combined sticker variant with a slash.
        assert_eq!(
            live_key("Sticker | Titan (Holo/Foil) | Katowice 2014", None),
            "Sticker | Titan (Holo/Foil) | Katowice 2014"
        );
    }

    #[test]
    fn cache_keeps_one_quote_per_market_and_ages_out() {
        let path = std::env::temp_dir().join(format!(
            "casemove-live-cache-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        // A missing file loads as an empty cache.
        assert!(LiveCache::load(&path).price("anything", Market::Steam).is_none());

        let mut cache = LiveCache::default();
        cache.insert("AK-47 | Redline (Field-Tested)", Market::Steam, 12.5, 1_000);
        cache.save(&path).unwrap();

        let reopened = LiveCache::load(&path);
        assert_eq!(
            reopened.price("AK-47 | Redline (Field-Tested)", Market::Steam),
            Some(12.5)
        );
        // The same key on another market is a separate quote, not a hit.
        assert_eq!(
            reopened.price("AK-47 | Redline (Field-Tested)", Market::Csfloat),
            None
        );
        assert!(reopened.fresh(
            "AK-47 | Redline (Field-Tested)",
            Market::Steam,
            1_000 + MAX_AGE_MS - 1
        ));
        assert!(!reopened.fresh(
            "AK-47 | Redline (Field-Tested)",
            Market::Steam,
            1_000 + MAX_AGE_MS
        ));
        assert!(!reopened.fresh("missing", Market::Steam, 1_000));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn market_quotes_do_not_override_each_other() {
        let mut cache = LiveCache::default();
        cache.insert("Item (Field-Tested)", Market::Steam, 10.0, 0);
        cache.insert("Item (Field-Tested)", Market::Skinport, 9.0, 0);
        assert_eq!(cache.price("Item (Field-Tested)", Market::Steam), Some(10.0));
        assert_eq!(
            cache.price("Item (Field-Tested)", Market::Skinport),
            Some(9.0)
        );
        assert_eq!(cache.price("Item (Field-Tested)", Market::Csfloat), None);
    }

    #[test]
    fn caches_from_before_multiple_markets_are_upgraded() {
        // The pre-market file shape: a flat map under `entries`.
        let path = std::env::temp_dir().join(format!(
            "casemove-legacy-live-cache-{}.json",
            std::process::id()
        ));
        std::fs::write(
            &path,
            r#"{"entries":{"AK-47 | Redline (Field-Tested)":{"price":12.5,"fetched_at_ms":1000}}}"#,
        )
        .unwrap();

        let cache = LiveCache::load(&path);
        assert_eq!(
            cache.price("AK-47 | Redline (Field-Tested)", Market::Steam),
            Some(12.5)
        );
        assert_eq!(
            cache.price("AK-47 | Redline (Field-Tested)", Market::Csfloat),
            None
        );

        // Saving rewrites it in the new shape without losing the quote.
        cache.save(&path).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("\"steam\""));
        assert_eq!(
            LiveCache::load(&path).price("AK-47 | Redline (Field-Tested)", Market::Steam),
            Some(12.5)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn skinport_quotes_the_median_and_skips_unpriced_entries() {
        let body = r#"[
            {"market_hash_name":"AK-47 | Redline (Field-Tested)","median_price":23.47,"min_price":21.0},
            {"market_hash_name":"Priced By Min Only","median_price":null,"min_price":4.2},
            {"market_hash_name":"No Price At All","median_price":null,"min_price":null},
            {"market_hash_name":"Zero","median_price":0.0,"min_price":0.0}
        ]"#;
        assert_eq!(
            parse_skinport_prices(body),
            vec![
                ("AK-47 | Redline (Field-Tested)".to_owned(), 23.47),
                ("Priced By Min Only".to_owned(), 4.2),
            ]
        );
        // Anything that is not the catalogue is simply no data.
        assert!(parse_skinport_prices(r#"{"errors":[]}"#).is_empty());
    }

    #[test]
    fn csfloat_min_price_is_reported_in_cents() {
        // The live endpoint reports 2347 for a Redline, i.e. $23.47.
        let body = r#"[
            {"market_hash_name":"AK-47 | Redline (Field-Tested)","quantity":3768,"min_price":2347},
            {"market_hash_name":"Karambit | Doppler (Factory New)","quantity":1,"min_price":6800000},
            {"market_hash_name":"Unlisted","quantity":0,"min_price":null}
        ]"#;
        assert_eq!(
            parse_csfloat_prices(body),
            vec![
                ("AK-47 | Redline (Field-Tested)".to_owned(), 23.47),
                ("Karambit | Doppler (Factory New)".to_owned(), 68000.0),
            ]
        );
        assert!(parse_csfloat_prices("[]").is_empty());
    }

    #[test]
    fn stored_market_selectors_round_trip() {
        for market in Market::ALL {
            assert_eq!(Market::from_stored(market.as_str()), market);
        }
        // An unknown or missing selector falls back to Steam.
        assert_eq!(Market::from_stored("buff163"), Market::Steam);
        assert_eq!(Market::from_stored(""), Market::Steam);
    }
}
