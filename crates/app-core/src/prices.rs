//! Live Steam Market price lookups (`priceoverview`) for inventory rows.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

const PRICEOVERVIEW_URL: &str = "https://steamcommunity.com/market/priceoverview/";
/// Two requests in flight, each followed by a pause — Steam throttles
/// bursts of priceoverview calls.
const CONCURRENCY: usize = 2;
const DELAY: Duration = Duration::from_millis(400);
const TIMEOUT: Duration = Duration::from_secs(10);

/// Fetches live Steam Market quotes for `keys` (each a `market_hash_name`).
/// Throttling stops the batch early; callers keep the bundled snapshot for
/// anything that comes back empty.
pub async fn fetch_live_prices(keys: Vec<String>) -> Vec<(String, f64)> {
    let client = reqwest::Client::new();
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
}
