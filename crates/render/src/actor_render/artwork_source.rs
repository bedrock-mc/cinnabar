//! Immutable source keys compare content only within one artwork slot.
use render_model::ActorTexturePage;
use std::{collections::hash_map::RandomState, hash::BuildHasher, sync::Arc};

/// Fingerprints speed up comparisons; exact source equality keeps collisions harmless.
#[derive(Clone)]
pub(super) struct TextureKey {
    page: ActorTexturePage,
    fingerprint: u64,
}

impl TextureKey {
    /// Hashes content only when a publication introduces a different source allocation.
    pub(super) fn new(page: &ActorTexturePage, hasher: &RandomState) -> Self {
        #[cfg(test)]
        super::work_tests::record_hash(page.rgba8.len());
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

    /// Reports whether uploaded source layers cover every layer requested by a fallback.
    pub(super) fn contains_layers(&self, layers: u32) -> bool {
        self.page.layers >= layers
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
