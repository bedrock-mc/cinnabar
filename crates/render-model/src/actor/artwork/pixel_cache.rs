//! Live artwork generations share equal packed pixels without retaining retired pages.
use super::*;
use std::sync::{Mutex, Weak};

struct Page {
    owner: Weak<[ActorTexturePage]>,
    index: usize,
}

impl Page {
    /// Borrows pixels only while their page collection still has a live owner.
    fn pixels(&self) -> Option<Arc<[u8]>> {
        Some(Arc::clone(&self.owner.upgrade()?.get(self.index)?.rgba8))
    }
}

type Pixels = BTreeMap<[u8; 32], Page>;

static LIVE: Mutex<Pixels> = Mutex::new(BTreeMap::new());

/// Hashes the ordered layers into the page identity, reusing a live packed allocation.
/// Cache entries own no pixels and cannot prevent a retired generation from being freed.
pub(super) fn pack<'a>(
    layers: impl Iterator<Item = &'a [u8]> + Clone,
    identity: &mut Sha256,
) -> ([u8; 32], Arc<[u8]>) {
    let mut digest = Sha256::new();
    for layer in layers.clone() {
        digest.update(layer);
        raster_pages::hash_pixels(identity, layer);
    }
    let key = digest.finalize().into();
    if let Some(pixels) = LIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&key)
        .and_then(Page::pixels)
    {
        return (key, pixels);
    }
    let pixels: Arc<[u8]> = concatenate_layers(layers).into();
    (key, pixels)
}

/// Registers page owners, whose destructors release pixel allocations even with weak entries.
/// A weak reference directly to an `Arc<[u8]>` would keep its entire pixel allocation reserved.
pub(super) fn remember(pages: &Arc<[ActorTexturePage]>, keys: &[([u8; 32], usize)]) {
    let mut live = LIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if live.len() >= MAX_ACTOR_TEXTURE_PAGES {
        live.retain(|_, page| page.owner.strong_count() != 0);
    }
    for &(key, index) in keys {
        if live.len() < MAX_ACTOR_TEXTURE_PAGES {
            live.insert(
                key,
                Page {
                    owner: Arc::downgrade(pages),
                    index,
                },
            );
        }
    }
}
