//! Live Steam Market quotes: the disk cache, Steam's `market_hash_name`
//! key derivation, and `priceoverview` response parsing. The HTTP transport
//! lives in `app-core` (`fetch_live_prices`).

use std::collections::HashMap;
use std::path::Path;

/// How long a fetched quote counts as fresh; older entries are refetched on
/// the next inventory load but keep displaying until they are replaced.
pub const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct Entry {
    price: f64,
    fetched_at_ms: i64,
}

/// Overlay of live quotes keyed by [`live_key`], persisted as JSON beside
/// the settings store. Missing entries fall back to the bundled snapshot.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LiveCache {
    #[serde(default)]
    entries: HashMap<String, Entry>,
}

impl LiveCache {
    /// The persisted cache; a missing or unreadable file starts empty.
    pub fn load(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = serde_json::to_string(self).map_err(std::io::Error::other)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, text)
    }

    /// The cached quote for `key`, however old — a stale live price still
    /// beats the bundled snapshot.
    pub fn price(&self, key: &str) -> Option<f64> {
        self.entries.get(key).map(|entry| entry.price)
    }

    /// Whether `key` was fetched within [`MAX_AGE_MS`] of `now_ms`.
    pub fn fresh(&self, key: &str, now_ms: i64) -> bool {
        self.entries
            .get(key)
            .is_some_and(|entry| now_ms - entry.fetched_at_ms < MAX_AGE_MS)
    }

    pub fn insert(&mut self, key: &str, price: f64, now_ms: i64) {
        self.entries.insert(
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
    fn cache_persists_and_ages_out() {
        let path = std::env::temp_dir().join(format!(
            "casemove-live-cache-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);

        // A missing file loads as an empty cache.
        assert!(LiveCache::load(&path).price("anything").is_none());

        let mut cache = LiveCache::default();
        cache.insert("AK-47 | Redline (Field-Tested)", 12.5, 1_000);
        cache.save(&path).unwrap();

        let reopened = LiveCache::load(&path);
        assert_eq!(
            reopened.price("AK-47 | Redline (Field-Tested)"),
            Some(12.5)
        );
        assert!(reopened.fresh("AK-47 | Redline (Field-Tested)", 1_000 + MAX_AGE_MS - 1));
        assert!(!reopened.fresh("AK-47 | Redline (Field-Tested)", 1_000 + MAX_AGE_MS));
        assert!(!reopened.fresh("missing", 1_000));
        let _ = std::fs::remove_file(&path);
    }
}
