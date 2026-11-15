//! Live market price lookups: Steam Market `priceoverview` per item, and the
//! bulk Skinport/CSFloat catalogues fetched once per refresh.

use pricing::Market;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PRICEOVERVIEW_URL: &str = "https://steamcommunity.com/market/priceoverview/";
/// Skinport returns the whole CS2 catalogue (~37k rows) in one response and
/// only speaks Brotli or gzip; the brotli feature is what makes it readable.
const SKINPORT_URL: &str = "https://api.skinport.com/v1/items";
/// CSFloat's public price list: lowest listing per market name, in cents.
const CSFLOAT_URL: &str = "https://csfloat.com/api/v1/listings/price-list";
/// Two requests in flight, each followed by a pause — Steam throttles
/// bursts of priceoverview calls.
const CONCURRENCY: usize = 2;
const DELAY: Duration = Duration::from_millis(400);
/// Bulk catalogues are large; give them longer than a single quote.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Fetches live quotes for `keys` (each a `market_hash_name`) from `market`.
///
/// Steam is queried per item and stops early when it throttles; Skinport and
/// CSFloat are queried once and then filtered down to `keys`, so an empty
/// `keys` list means nothing to do.
pub async fn fetch_market_prices(market: Market, keys: Vec<String>) -> Vec<(String, f64)> {
    if keys.is_empty() {
        return Vec::new();
    }
    let client = reqwest::Client::new();
    match market {
        Market::Steam => fetch_steam(&client, keys).await,
        Market::Skinport => fetch_bulk(&client, SKINPORT_URL, keys, pricing::parse_skinport_prices)
            .await,
        Market::Csfloat => fetch_bulk(&client, CSFLOAT_URL, keys, pricing::parse_csfloat_prices)
            .await,
    }
}

/// Fetches live Steam Market quotes for `keys`. Throttling stops the batch
/// early; callers keep the bundled snapshot for anything that comes back
/// empty.
pub async fn fetch_live_prices(keys: Vec<String>) -> Vec<(String, f64)> {
    fetch_market_prices(Market::Steam, keys).await
}

async fn fetch_steam(client: &reqwest::Client, keys: Vec<String>) -> Vec<(String, f64)> {
    let semaphore = Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let throttled = Arc::new(AtomicBool::new(false));
    let mut requests = Vec::with_capacity(keys.len());
    for key in keys {
        let client = client.clone();
        let semaphore = Arc::clone(&semaphore);
        let throttled = Arc::clone(&throttled);
        requests.push(tokio::spawn(async move {
            let Ok(_permit) = semaphore.acquire_owned().await else {
                return None;
            };
            if throttled.load(Ordering::Relaxed) {
                return None;
            }
            let outcome = fetch_one(&client, &key).await;
            tokio::time::sleep(DELAY).await;
            match outcome {
                Ok(Some(price)) => Some((key, price)),
                Ok(None) => None,
                Err(()) => {
                    throttled.store(true, Ordering::Relaxed);
                    None
                }
            }
        }));
    }
    let mut prices = Vec::new();
    for request in requests {
        if let Some(entry) = request.await.ok().flatten() {
            prices.push(entry);
        }
    }
    prices
}

/// Downloads a whole catalogue, keeps the rows we asked for, and drops
/// anything the caller did not have an item for.
async fn fetch_bulk(
    client: &reqwest::Client,
    url: &str,
    keys: Vec<String>,
    parse: fn(&str) -> Vec<(String, f64)>,
) -> Vec<(String, f64)> {
    let request = match url {
        SKINPORT_URL => client.get(url).query(&[
            ("app_id", "730"),
            ("currency", "USD"),
            ("tradable", "0"),
        ]),
        _ => client.get(url).query(&[("limit", "100000")]),
    };
    let Ok(response) = tokio::time::timeout(TIMEOUT, request.send()).await else {
        return Vec::new();
    };
    let Ok(response) = response else {
        return Vec::new();
    };
    if !response.status().is_success() {
        return Vec::new();
    }
    let Ok(body) = response.text().await else {
        return Vec::new();
    };
    let wanted: std::collections::HashSet<&str> = keys.iter().map(String::as_str).collect();
    parse(&body)
        .into_iter()
        .filter(|(key, _)| wanted.contains(key.as_str()))
        .collect()
}

/// `Ok(Some(price))` fetched, `Ok(None)` not on the market, `Err` throttle
/// (or anything else that should stop the batch).
async fn fetch_one(client: &reqwest::Client, key: &str) -> Result<Option<f64>, ()> {
    let request = client.get(PRICEOVERVIEW_URL).query(&[
        ("appid", "730"),
        ("currency", "1"),
        ("country", "US"),
        ("language", "english"),
        ("market_hash_name", key),
    ]);
    let response = tokio::time::timeout(TIMEOUT, request.send())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if !response.status().is_success() {
        return Err(());
    }
    let body = response.text().await.map_err(|_| ())?;
    match pricing::parse_priceoverview(&body) {
        pricing::PriceOverview::Price(price) => Ok(Some(price)),
        pricing::PriceOverview::NotListed => Ok(None),
        pricing::PriceOverview::Unparseable => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "hits the live Steam Market"]
    async fn fetches_a_live_price_for_a_known_skin() {
        let prices = fetch_live_prices(vec!["AK-47 | Redline (Field-Tested)".to_owned()]).await;
        assert_eq!(prices.len(), 1);
        assert!(prices[0].1 > 0.0);
    }

    /// The bulk catalogues only accept clients that advertise Brotli, which
    /// is why `reqwest` carries that feature — check both end to end.
    #[tokio::test]
    #[ignore = "hits the live Skinport and CSFloat APIs"]
    async fn fetches_a_live_price_from_the_bulk_catalogues() {
        let keys = vec!["AK-47 | Redline (Field-Tested)".to_owned()];
        for market in [Market::Skinport, Market::Csfloat] {
            let prices = fetch_market_prices(market, keys.clone()).await;
            assert_eq!(prices.len(), 1, "{market:?} returned {prices:?}");
            assert!(prices[0].1 > 5.0, "{market:?} returned {:?}", prices[0]);
        }
    }

    #[tokio::test]
    async fn an_empty_key_list_skips_the_network() {
        assert!(fetch_market_prices(Market::Skinport, Vec::new()).await.is_empty());
    }
}
