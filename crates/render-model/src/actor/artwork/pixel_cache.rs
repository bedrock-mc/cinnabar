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

type Pixels = BTreeMap<[u8; 32], Vec<Page>>;

static LIVE: Mutex<Pixels> = Mutex::new(BTreeMap::new());

/// Hashes the ordered layers into the page identity, reusing a live packed allocation.
/// Single-layer pages share their source allocation. Cache entries own no pixels and cannot
/// prevent a retired generation from being freed.
pub(super) fn pack<'a>(
    layers: impl Iterator<Item = &'a Arc<[u8]>> + Clone,
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
        .and_then(|owners| owners.iter().find_map(Page::pixels))
    {
        return (key, pixels);
    }
    let mut ordered = layers.clone();
    let pixels = match (ordered.next(), ordered.next()) {
        (Some(layer), None) => Arc::clone(layer),
        _ => concatenate_layers(layers.map(AsRef::as_ref)).into(),
    };
    (key, pixels)
}

/// Registers page owners, whose destructors release pixel allocations even with weak entries.
/// A weak reference directly to an `Arc<[u8]>` would keep its entire pixel allocation reserved.
pub(super) fn remember(
    previous: &Arc<[ActorTexturePage]>,
    pages: &Arc<[ActorTexturePage]>,
    keys: &[([u8; 32], usize)],
) {
    let mut live = LIVE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    live.retain(|_, owners| {
        owners.retain(|page| page.owner.strong_count() != 0);
        !owners.is_empty()
    });
    let previous_owner = Arc::downgrade(previous);
    for owners in live.values_mut() {
        let inherited = owners.iter().find_map(|page| {
            if !Weak::ptr_eq(&page.owner, &previous_owner) {
                return None;
            }
            let (before, after) = (previous.get(page.index)?, pages.get(page.index)?);
            Arc::ptr_eq(&before.rgba8, &after.rgba8).then_some(page.index)
        });
        if let Some(index) = inherited {
            add_owner(owners, pages, index);
        }
    }
    for &(key, index) in keys {
        if live.contains_key(&key) || live.len() < MAX_ACTOR_TEXTURE_PAGES {
            add_owner(live.entry(key).or_default(), pages, index);
        }
    }
}

/// Keeps every live collection that owns these pixels, since generations can retire in any order.
fn add_owner(owners: &mut Vec<Page>, pages: &Arc<[ActorTexturePage]>, index: usize) {
    let owner = Arc::downgrade(pages);
    if !owners
        .iter()
        .any(|page| page.index == index && Weak::ptr_eq(&page.owner, &owner))
    {
        owners.push(Page { owner, index });
    }
}
