//! A browser downloads a language's carrier only once a player picks it
//! (`assets::carriers::file::remote`), instead of holding every language in memory.

use std::{
    cell::RefCell,
    collections::HashSet,
    path::{Path, PathBuf},
};

use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

thread_local! {
    /// Languages downloading now, and those whose download failed, which then stay English.
    static DOWNLOADING: RefCell<HashSet<PathBuf>> = RefCell::default();
    static FAILED: RefCell<HashSet<PathBuf>> = RefCell::default();
}

/// Whether the carrier of language `code` beside `world_asset_path` is still on its way; the
/// first call for a language not in memory starts its download.
pub(super) fn language_arriving(world_asset_path: &Path, code: &str) -> bool {
    let Some(path) = assets::carriers::startup::language_carrier_path(world_asset_path, code)
    else {
        return false;
    };
    let Some(url) = assets::carriers::file::remote(&path) else {
        return false;
    };
    if FAILED.with_borrow(|failed| failed.contains(&path)) {
        return false;
    }
    if !DOWNLOADING.with_borrow_mut(|downloading| downloading.insert(path.clone())) {
        return true;
    }
    wasm_bindgen_futures::spawn_local(async move {
        match download(&url).await {
            Some(bytes) => assets::carriers::file::add_downloaded(&path, bytes),
            None => {
                bevy::log::warn!(%url, "language download failed; text stays in English");
                FAILED.with_borrow_mut(|failed| failed.insert(path.clone()));
            }
        }
        DOWNLOADING.with_borrow_mut(|downloading| downloading.remove(&path));
    });
    true
}

/// The body of `url`, or `None` when it cannot be fetched whole.
async fn download(url: &str) -> Option<Vec<u8>> {
    let response: web_sys::Response = JsFuture::from(web_sys::window()?.fetch_with_str(url))
        .await
        .ok()?
        .dyn_into()
        .ok()?;
    if !response.ok() {
        return None;
    }
    let body = JsFuture::from(response.array_buffer().ok()?).await.ok()?;
    Some(js_sys::Uint8Array::new(&body).to_vec())
}
