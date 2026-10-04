//! Shared skin layout limits and conversion, used by admission and rendering.

use std::{
    hash::{BuildHasher, Hash, Hasher, RandomState},
    ops::Deref,
    sync::{Arc, OnceLock},
};

/// Local ceiling for the standard skin raster, including persona skins.
pub const MAX_STANDARD_SKIN_SIDE: u32 = 512;
/// Animated texture slots the vanilla player renderer adds to the base skin.
pub const MAX_SKIN_ANIMATION_LAYERS: usize = 3;

/// The authored square texture side for the classic skin layout.
pub const CLASSIC_SKIN_SIDE: usize = 64;
/// Largest square raster accepted by vanilla's classic skin validator.
pub const MAX_CLASSIC_SKIN_SIDE: usize = CLASSIC_SKIN_SIDE * 2;

/// Expands a legacy half-height skin to the square layout: the left limbs are the right limbs
/// with every face mirrored, as the legacy geometry draws them.
pub fn expand_legacy_skin_rgba8(rgba8: &[u8], side: usize) -> Vec<u8> {
    let scale = side / CLASSIC_SKIN_SIDE;
    let mut square = vec![0; side * side * 4];
    square[..rgba8.len()].copy_from_slice(rgba8);
    // (source x, source y, dest offset x, dest offset y, width, height) in 64-unit texels.
    const LIMB_FACES: [(usize, usize, isize, usize, usize, usize); 12] = [
        (4, 16, 16, 32, 4, 4),
        (8, 16, 16, 32, 4, 4),
        (0, 20, 24, 32, 4, 12),
        (4, 20, 16, 32, 4, 12),
        (8, 20, 8, 32, 4, 12),
        (12, 20, 16, 32, 4, 12),
        (44, 16, -8, 32, 4, 4),
        (48, 16, -8, 32, 4, 4),
        (40, 20, 0, 32, 4, 12),
        (44, 20, -8, 32, 4, 12),
        (48, 20, -16, 32, 4, 12),
        (52, 20, -8, 32, 4, 12),
    ];
    for (x, y, dx, dy, width, height) in LIMB_FACES {
        let (x, y, width, height) = (x * scale, y * scale, width * scale, height * scale);
        let target_x = (x as isize + dx * scale as isize) as usize;
        let target_y = y + dy * scale;
        for row in 0..height {
            for column in 0..width {
                let source = ((y + row) * side + x + column) * 4;
                let target = ((target_y + row) * side + target_x + width - 1 - column) * 4;
                let pixel: [u8; 4] = rgba8[source..source + 4].try_into().expect("four bytes");
                square[target..target + 4].copy_from_slice(&pixel);
            }
        }
    }
    square
}

/// Skin texels with a content hash taken once at ingest, so equal skins match without a byte scan.
///
/// Equality is identity: the same allocation, or the same length and keyed 64-bit content hash.
#[derive(Clone, Debug)]
pub struct SkinRgba8 {
    rgba8: Arc<[u8]>,
    content_hash: u64,
}

impl SkinRgba8 {
    #[must_use]
    pub fn new(rgba8: Arc<[u8]>) -> Self {
        // Process-random keys keep a server from crafting two skins that collide.
        static KEYS: OnceLock<RandomState> = OnceLock::new();
        let content_hash = KEYS.get_or_init(RandomState::new).hash_one(&*rgba8);
        Self {
            rgba8,
            content_hash,
        }
    }

    #[must_use]
    pub const fn pixels(&self) -> &Arc<[u8]> {
        &self.rgba8
    }

    #[must_use]
    pub const fn content_hash(&self) -> u64 {
        self.content_hash
    }
}

impl Deref for SkinRgba8 {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        &self.rgba8
    }
}

impl AsRef<[u8]> for SkinRgba8 {
    fn as_ref(&self) -> &[u8] {
        &self.rgba8
    }
}

impl From<Arc<[u8]>> for SkinRgba8 {
    fn from(rgba8: Arc<[u8]>) -> Self {
        Self::new(rgba8)
    }
}

impl From<Vec<u8>> for SkinRgba8 {
    fn from(rgba8: Vec<u8>) -> Self {
        Self::new(rgba8.into())
    }
}

impl PartialEq for SkinRgba8 {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.rgba8, &other.rgba8)
            || (self.content_hash == other.content_hash && self.rgba8.len() == other.rgba8.len())
    }
}

impl Eq for SkinRgba8 {}

impl Hash for SkinRgba8 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.content_hash);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_texels_in_distinct_allocations_are_one_skin() {
        let first = SkinRgba8::from(vec![7_u8; 4096]);
        let copy = SkinRgba8::from(vec![7_u8; 4096]);
        let mut changed = vec![7_u8; 4096];
        changed[4095] = 8;
        let changed = SkinRgba8::from(changed);
        assert!(!Arc::ptr_eq(first.pixels(), copy.pixels()));
        assert_eq!(first, copy);
        assert_eq!(first.content_hash(), copy.content_hash());
        assert_ne!(first, changed);
        assert_ne!(first, SkinRgba8::from(vec![7_u8; 4092]));
    }
}
