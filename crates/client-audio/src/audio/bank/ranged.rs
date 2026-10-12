//! A browser's sound bank: only its index is loaded, and each sound's bytes are downloaded by
//! byte range the first time it plays, then kept in the browser's Cache Storage for later visits.

use std::sync::Arc;

use assets::SoundBankEntry;
use js_sys::{ArrayBuffer, Uint8Array};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{Cache, Request, RequestInit, Response};

/// The Cache Storage cache sound ranges are kept in.
const CACHE_NAME: &str = "cinnabar-sounds";

/// Where a browser's bank entries are downloaded from.
#[derive(Clone)]
pub(super) struct RangedBank {
    pub(super) url: Arc<str>,
    /// Names this bank's contents in cache keys, so a recompiled bank never reuses stale ranges.
    pub(super) identity: Arc<str>,
}

impl RangedBank {
    /// The bytes of `entry`: from the cache, else downloaded and cached; `None` on any failure.
    pub(super) async fn read(&self, entry: SoundBankEntry) -> Option<Vec<u8>> {
        let last = entry
            .offset
            .checked_add(u64::from(entry.len))?
            .checked_sub(1)?;
        let key = format!(
            "{}?bank={}&bytes={}-{last}",
            self.url, self.identity, entry.offset
        );
        let cache = open_cache().await;
        if let Some(cache) = &cache
            && let Some(bytes) = cached(cache, &key).await
            && bytes.len() == entry.len as usize
        {
            return Some(bytes);
        }
        let response = fetch_range(&self.url, entry.offset, last).await?;
        let bytes = response_bytes(&response).await?;
        if bytes.len() != entry.len as usize {
            bevy::log::debug!(url = %self.url, "sound range download has the wrong length");
            return None;
        }
        if let Some(cache) = &cache {
            store(cache, &key, &bytes).await;
        }
        Some(bytes)
    }
}

async fn open_cache() -> Option<Cache> {
    let caches = web_sys::window()?.caches().ok()?;
    JsFuture::from(caches.open(CACHE_NAME))
        .await
        .ok()?
        .dyn_into()
        .ok()
}

async fn cached(cache: &Cache, key: &str) -> Option<Vec<u8>> {
    let found = JsFuture::from(cache.match_with_str(key)).await.ok()?;
    let response: Response = found.dyn_into().ok()?;
    response_bytes(&response).await
}

/// Stores `bytes` under `key`; a full or unavailable cache only costs a later download.
async fn store(cache: &Cache, key: &str, bytes: &[u8]) {
    let body = Uint8Array::from(bytes);
    let Ok(response) = Response::new_with_opt_buffer_source(Some(&body)) else {
        return;
    };
    let _ = JsFuture::from(cache.put_with_str(key, &response)).await;
}

/// Requests bytes `first..=last` of `url`; only a partial-content answer is accepted.
async fn fetch_range(url: &str, first: u64, last: u64) -> Option<Response> {
    let init = RequestInit::new();
    let headers = web_sys::Headers::new().ok()?;
    headers
        .set("Range", &format!("bytes={first}-{last}"))
        .ok()?;
    init.set_headers(&headers);
    let request = Request::new_with_str_and_init(url, &init).ok()?;
    let response: Response = JsFuture::from(web_sys::window()?.fetch_with_request(&request))
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    (response.status() == 206).then_some(response)
}

async fn response_bytes(response: &Response) -> Option<Vec<u8>> {
    let buffer: JsValue = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
    let buffer: ArrayBuffer = buffer.dyn_into().ok()?;
    Some(Uint8Array::new(&buffer).to_vec())
}
