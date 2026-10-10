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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandardSkin {
    pub width: u32,
    pub height: u32,
    pub rgba8: SkinRgba8,
    /// The skin's cape image when it carries a valid one; counts toward the skin byte budget.
    pub cape: Option<CapeImage>,
    /// The skin's own model inputs when it may name a non-default geometry.
    pub geometry: Option<Arc<SkinGeometrySource>>,
}

/// The resource patch and geometry JSON a skin carries; parsed by the actor runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinGeometrySource {
    pub resource_patch: Arc<str>,
    pub geometry_data: Arc<str>,
    pub animations: Arc<[SkinAnimation]>,
}

impl SkinGeometrySource {
    /// Returns the retained geometry and animation image bytes.
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.resource_patch.len()
            + self.geometry_data.len()
            + self
                .animations
                .iter()
                .map(|image| image.rgba8.len())
                .sum::<usize>()
    }
}

/// Model input bytes one skin may retain; larger models fall back to the default geometry.
pub const MAX_SKIN_GEOMETRY_SOURCE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapeImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
}

impl CapeImage {
    /// Checks that the raster matches one of the supported cape dimensions.
    pub fn is_valid(&self) -> bool {
        CAPE_DIMENSIONS.contains(&(self.width, self.height))
            && self.rgba8.len() == self.width as usize * self.height as usize * 4
    }
}

/// Cape image sizes Bedrock skins use, as `(width, height)`.
pub const CAPE_DIMENSIONS: [(u32, u32); 4] = [(64, 32), (128, 64), (256, 128), (1024, 512)];

/// Named geometry and texture slots used by the persona render controllers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinAnimationKind {
    Face,
    Body32,
    Body128,
}

impl SkinAnimationKind {
    /// Returns the stable atlas slot for this animation kind.
    pub const fn slot(self) -> usize {
        match self {
            Self::Face => 0,
            Self::Body32 => 1,
            Self::Body128 => 2,
        }
    }
    /// Returns the resource-patch geometry slot for this animation image.
    pub const fn geometry_key(self) -> &'static str {
        match self {
            Self::Face => "animated_face",
            Self::Body32 => "animated_32x32",
            Self::Body128 => "animated_128x128",
        }
    }
}

/// A transmitted persona animation atlas, without resampling or alpha modification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinAnimation {
    pub kind: SkinAnimationKind,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
    pub frames: u32,
    pub blinking: bool,
}

/// Minimum engine version for model inputs without a manifest requirement.
pub const DEFAULT_SKIN_GEOMETRY_ENGINE_VERSION: &str = "0.0.0";

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
