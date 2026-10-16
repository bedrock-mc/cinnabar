//! The Cinnabar API's read-only news feed, cached so the launcher still shows it offline.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

/// The feed route under the shared API origin.
const ROUTE: &str = "/v1/news";
const MAX_ITEMS: usize = 20;
/// A cache older than this reads as stale even when the last fetch succeeded.
const MAX_AGE: time::Duration = time::Duration::days(1);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct NewsItem {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub image_url: Option<String>,
    pub published_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tag: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "camelCase")]
pub struct News {
    pub items: Vec<NewsItem>,
    pub stale: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub fetched_at: Option<String>,
}

#[derive(Deserialize)]
struct Feed {
    items: Vec<NewsItem>,
}

#[derive(Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Cached {
    fetched_at: String,
    items: Vec<NewsItem>,
}

/// Keeps well-formed items with HTTPS links, skipping odd ones rather than the whole feed.
pub fn parse(body: &[u8]) -> Result<Vec<NewsItem>> {
    let feed: Feed = serde_json::from_slice(body)?;
    let https = |url: &str| url.starts_with("https://");
    Ok(feed
        .items
        .into_iter()
        .filter(|item| {
            https(&item.url)
                && item.image_url.as_deref().is_none_or(https)
                && OffsetDateTime::parse(&item.published_at, &Rfc3339).is_ok()
        })
        .take(MAX_ITEMS)
        .collect())
}

/// What the launcher shows from `cached`; `online` says whether the latest fetch succeeded.
pub fn present(cached: Option<Cached>, online: bool, now: OffsetDateTime) -> News {
    let Some(cached) = cached else {
        return News {
            items: Vec::new(),
            stale: !online,
            fetched_at: None,
        };
    };
    let fresh = OffsetDateTime::parse(&cached.fetched_at, &Rfc3339)
        .is_ok_and(|fetched| now - fetched <= MAX_AGE && fetched <= now);
    News {
        stale: !online || !fresh,
        items: cached.items,
        fetched_at: Some(cached.fetched_at),
    }
}

pub fn load(path: &Path) -> Option<Cached> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Replaces the cache with `items` fetched at `now`, returning what was saved.
pub fn save(path: &Path, items: Vec<NewsItem>, now: OffsetDateTime) -> Result<Cached> {
    let cached = Cached {
        fetched_at: now.format(&Rfc3339)?,
        items,
    };
    let directory = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("news cache has no directory"))?;
    std::fs::create_dir_all(directory)?;
    let mut file = tempfile::NamedTempFile::new_in(directory)?;
    serde_json::to_writer(&mut file, &cached)?;
    file.persist(path)?;
    Ok(cached)
}

/// Fetches the feed from the API.
pub async fn fetch() -> Result<Vec<NewsItem>> {
    parse(&crate::api::get(ROUTE).await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, url: &str) -> serde_json::Value {
        serde_json::json!({
            "id": id,
            "title": "Title",
            "summary": "Summary",
            "url": url,
            "publishedAt": "2026-10-16T00:00:00Z",
        })
    }

    #[test]
    fn odd_items_are_skipped_without_dropping_the_feed() {
        let body = serde_json::json!({ "items": [
            item("good", "https://example.test/a"),
            item("plain", "http://example.test/b"),
            { "id": "dated", "title": "", "summary": "", "url": "https://example.test/c", "publishedAt": "yesterday" },
        ]});
        let items = parse(&serde_json::to_vec(&body).unwrap()).unwrap();
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["good"]
        );
        assert!(parse(b"{").is_err());
    }

    #[test]
    fn the_cache_is_stale_offline_or_once_it_ages_past_a_day() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("news.json");
        let fetched = OffsetDateTime::UNIX_EPOCH + time::Duration::days(100);
        let items = parse(
            &serde_json::to_vec(
                &serde_json::json!({ "items": [item("a", "https://example.test")] }),
            )
            .unwrap(),
        )
        .unwrap();
        save(&path, items.clone(), fetched).unwrap();
        let shown = present(load(&path), true, fetched + time::Duration::hours(1));
        assert!(!shown.stale);
        assert_eq!(shown.items, items);
        assert!(present(load(&path), false, fetched).stale);
        assert!(present(load(&path), true, fetched + time::Duration::days(2)).stale);
        // A clock that moved backwards cannot make an old cache fresh.
        assert!(present(load(&path), true, fetched - time::Duration::days(1)).stale);
        let empty = present(None, false, fetched);
        assert!(empty.stale && empty.items.is_empty() && empty.fetched_at.is_none());
    }
}
