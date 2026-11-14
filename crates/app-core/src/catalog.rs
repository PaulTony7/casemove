//! Econ-path image catalog (`econ/...` → Steam CDN URL), fetched once at
//! startup and disk-cached. Completes thumbnail coverage for items the
//! bundled collections map does not name.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

const CATALOG_URL: &str =
    "https://raw.githubusercontent.com/ByMykel/counter-strike-image-tracker/main/static/images.json";
/// The catalog ships new items as they arrive; re-pull weekly.
const CATALOG_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// The catalog from disk, refreshing it first when the cache is stale or
/// missing. Returns an empty map when neither source is available — items
/// then fall back to the bundled collections and the placeholder glyph.
pub async fn fetch_image_catalog() -> HashMap<String, String> {
    let path = crate::config_dir().join("images-catalog.json");
    let stale = std::fs::metadata(&path)
        .ok()
        .and_then(|meta| meta.modified().ok())
        .and_then(|modified| modified.elapsed().ok())
        .is_none_or(|age| age > CATALOG_TTL);
    if stale
        && let Some(text) = fetch_catalog_text().await
    {
        let destination = path.clone();
        return tokio::task::spawn_blocking(move || {
            let catalog = parse_catalog(&text);
            if !catalog.is_empty() {
                let _ = std::fs::write(&destination, &text);
            }
            catalog
        })
        .await
        .unwrap_or_default();
    }
    tokio::task::spawn_blocking(move || read_catalog(&path))
        .await
        .unwrap_or_default()
}

async fn fetch_catalog_text() -> Option<String> {
    let request = async {
        reqwest::get(CATALOG_URL)
            .await
            .ok()?
            .error_for_status()
            .ok()?
            .text()
            .await
            .ok()
    };
    tokio::time::timeout(Duration::from_secs(60), request)
        .await
        .ok()
        .flatten()
}

fn read_catalog(path: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| parse_catalog(&text))
        .unwrap_or_default()
}

/// The tracker maps paths to URLs (with occasional nulls for retired items).
fn parse_catalog(text: &str) -> HashMap<String, String> {
    serde_json::from_str::<HashMap<String, Option<String>>>(text)
        .map(|entries| {
            entries
                .into_iter()
                .filter_map(|(path, url)| url.map(|url| (path, url)))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_paths_and_drops_retired_null_entries() {
        let catalog = parse_catalog(
            r#"{"econ/tools/casket":"https://cdn/economy/image/abc","econ/gone":null}"#,
        );
        assert_eq!(
            catalog.get("econ/tools/casket").map(String::as_str),
            Some("https://cdn/economy/image/abc")
        );
        assert!(!catalog.contains_key("econ/gone"));
        assert!(parse_catalog("<html>rate limited</html>").is_empty());
    }

    #[tokio::test]
    async fn missing_cache_and_failed_fetch_yield_an_empty_catalog() {
        let path = std::env::temp_dir().join(format!(
            "casemove-images-catalog-{}.json",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        assert!(read_catalog(&path).is_empty());

        let text = r#"{"econ/tools/casket":"https://cdn/x"}"#;
        std::fs::write(&path, text).unwrap();
        assert_eq!(read_catalog(&path).len(), 1);
        let _ = std::fs::remove_file(&path);
    }
}
