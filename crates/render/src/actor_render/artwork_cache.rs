//! Content keys retain immutable device textures across artwork publications.
use super::GpuArtworkPage;
use std::{
    hash::{BuildHasher, Hash, Hasher},
    sync::Arc,
};

/// Fingerprints select candidates; exact source equality keeps hash collisions harmless.
#[derive(Clone)]
pub(super) struct TextureKey {
    page: ActorTexturePage,
    fingerprint: u64,
}

impl TextureKey {
    /// Hashes content only when a publication introduces a different source allocation.
    pub(super) fn new(page: &ActorTexturePage, hasher: &RandomState) -> Self {
        Self {
            page: page.clone(),
            fingerprint: hasher.hash_one((page.width, page.height, page.layers, &page.rgba8)),
        }
    }

    /// Pointer identity is safe because every key owns its immutable source allocation.
    pub(super) fn shares_pixels_with(&self, page: &ActorTexturePage) -> bool {
        self.page.width == page.width
            && self.page.height == page.height
            && self.page.layers == page.layers
            && Arc::ptr_eq(&self.page.rgba8, &page.rgba8)
    }
}

impl PartialEq for TextureKey {
    /// Source dimensions and exact pixels define texture reuse independently of material flags.
    fn eq(&self, other: &Self) -> bool {
        self.fingerprint == other.fingerprint
            && self.page.width == other.page.width
            && self.page.height == other.page.height
            && self.page.layers == other.page.layers
            && (Arc::ptr_eq(&self.page.rgba8, &other.page.rgba8)
                || self.page.rgba8 == other.page.rgba8)
    }
}
impl Eq for TextureKey {}
impl Hash for TextureKey {
    /// Cache lookup reuses the fingerprint without traversing source pixels again.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fingerprint.hash(state);
    }
}
use crate::ActorTexturePage;
use std::collections::{HashMap, hash_map::RandomState};

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct AllocationKey {
    address: usize,
    bytes: usize,
    width: u16,
    height: u16,
    layers: u32,
}

impl AllocationKey {
    /// The retained content key keeps this allocation alive for the lookup's lifetime.
    fn new(page: &ActorTexturePage) -> Self {
        Self {
            address: page.rgba8.as_ptr() as usize,
            bytes: page.rgba8.len(),
            width: page.width,
            height: page.height,
            layers: page.layers,
        }
    }
}

#[derive(Default)]
pub(super) struct TextureCache {
    allocations: HashMap<AllocationKey, TextureKey>,
    textures: HashMap<TextureKey, GpuArtworkPage>,
}

impl TextureCache {
    /// Retains the previous publication until every new route has found its texture.
    pub(super) fn new(pages: Vec<GpuArtworkPage>) -> Self {
        let mut cache = Self::default();
        for page in pages {
            cache.insert(page);
        }
        cache
    }

    /// Shared allocations avoid hashing; equal independent allocations share their texture too.
    pub(super) fn key(&self, page: &ActorTexturePage, hasher: &RandomState) -> TextureKey {
        self.allocations
            .get(&AllocationKey::new(page))
            .cloned()
            .unwrap_or_else(|| TextureKey::new(page, hasher))
    }

    /// Returns another handle to the same immutable device texture and bindings.
    pub(super) fn get(&self, key: &TextureKey) -> Option<GpuArtworkPage> {
        self.textures.get(key).cloned()
    }

    /// A page may occur more than once in one publication without another upload.
    pub(super) fn insert(&mut self, page: GpuArtworkPage) {
        self.allocations
            .insert(AllocationKey::new(&page.source.page), page.source.clone());
        self.textures.insert(page.source.clone(), page);
    }
}
