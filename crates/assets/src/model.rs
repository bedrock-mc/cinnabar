use crate::{AssetError, TextureArray};

pub const MAX_TEXTURE_PAGES: usize = 2;
pub const MAX_MODEL_TEMPLATES: usize = 65_536;
pub const MAX_MODEL_QUADS: usize = MAX_MODEL_TEMPLATES * 32;
pub const MAX_ANIMATIONS: usize = 65_536;
pub const MAX_ANIMATION_FRAMES: usize = 1_048_576;
pub const NO_MODEL_TEMPLATE: u32 = u32::MAX;
pub const NO_ANIMATION: u32 = u32::MAX;

/// Template selects its body or head quads from the primary block above it.
pub const MODEL_TEMPLATE_FLAG_KELP: u32 = 1 << 0;
pub(crate) const MODEL_TEMPLATE_FLAGS_MASK: u32 = MODEL_TEMPLATE_FLAG_KELP;

const TEXTURE_PAGE_BIT: u32 = 1 << 31;
const TEXTURE_LAYER_MASK: u32 = 0x7ff;
const TEXTURE_RESERVED_MASK: u32 = !(TEXTURE_PAGE_BIT | TEXTURE_LAYER_MASK);

/// Canonical reference to a layer in one of at most two texture-array pages.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct TextureRef(u32);

impl TextureRef {
    pub const DIAGNOSTIC: Self = Self(0);

    pub fn new(page: u32, layer: u32) -> Result<Self, AssetError> {
        if page >= MAX_TEXTURE_PAGES as u32 || layer > TEXTURE_LAYER_MASK {
            return Err(invalid(format!(
                "texture reference page {page}, layer {layer} is out of range"
            )));
        }
        Ok(Self((page << 31) | layer))
    }

    pub fn from_raw(raw: u32) -> Result<Self, AssetError> {
        if raw & TEXTURE_RESERVED_MASK != 0 {
            return Err(invalid(format!(
                "texture reference {raw:#010x} has non-zero reserved bits"
            )));
        }
        Ok(Self(raw))
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn page(self) -> u32 {
        self.0 >> 31
    }

    #[must_use]
    pub const fn layer(self) -> u32 {
        self.0 & TEXTURE_LAYER_MASK
    }
}

/// One physical texture-array page. Pages are serialized with independent hashes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TexturePage {
    pub texture: TextureArray,
}

impl TexturePage {
    #[must_use]
    pub const fn new(texture: TextureArray) -> Self {
        Self { texture }
    }
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualKind {
    Diagnostic = 0,
    Cube = 1,
    Cross = 2,
    Model = 3,
    Liquid = 4,
    Invisible = 5,
}

impl VisualKind {
    pub(crate) fn from_raw(raw: u8) -> Result<Self, AssetError> {
        match raw {
            0 => Ok(Self::Diagnostic),
            1 => Ok(Self::Cube),
            2 => Ok(Self::Cross),
            3 => Ok(Self::Model),
            4 => Ok(Self::Liquid),
            5 => Ok(Self::Invisible),
            _ => Err(invalid(format!("unknown visual kind {raw}"))),
        }
    }
}

/// A bounded span of immutable model quads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelTemplate {
    pub quad_start: u32,
    pub quad_count: u32,
    pub flags: u32,
}

/// Fixed-point template quad. Position coordinates use 1/256 block units.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ModelQuad {
    pub positions: [[i16; 3]; 4],
    /// Per-vertex UV coordinates in 1/4096 texture-tile units. Values above
    /// 4096 intentionally support wrapped UVs on greedy-compatible templates.
    pub uvs: [[u16; 2]; 4],
    pub material: u32,
    pub flags: u32,
}

const _: () = assert!(std::mem::size_of::<ModelQuad>() == 48);

/// Optional face and cull-face use `0 = none`, `1..=6 =
/// down/up/west/east/north/south` in their respective three-bit fields.
pub const MODEL_QUAD_FLAG_FACE_MASK: u32 = 0x07;
pub const MODEL_QUAD_FLAG_TWO_SIDED: u32 = 1 << 3;
pub const MODEL_QUAD_FLAG_CULL_FACE_MASK: u32 = 0x70;
pub(crate) const MODEL_QUAD_FLAGS_MASK: u32 =
    MODEL_QUAD_FLAG_FACE_MASK | MODEL_QUAD_FLAG_TWO_SIDED | MODEL_QUAD_FLAG_CULL_FACE_MASK;

pub(crate) const fn model_quad_flags_are_valid(flags: u32) -> bool {
    flags & !MODEL_QUAD_FLAGS_MASK == 0
        && flags & MODEL_QUAD_FLAG_FACE_MASK <= 6
        && (flags & MODEL_QUAD_FLAG_CULL_FACE_MASK) >> 4 <= 6
}

/// Immutable animation timeline descriptor.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Animation {
    pub frame_start: u32,
    pub frame_count: u32,
    pub ticks_per_frame: u32,
    pub atlas_index: u32,
    pub atlas_tile_variant: u32,
    pub replicate: u32,
    pub flags: u32,
}

pub const ANIMATION_FLAG_BLEND: u32 = 1;
pub(crate) const ANIMATION_FLAGS_MASK: u32 = ANIMATION_FLAG_BLEND;

fn invalid(detail: impl Into<Box<str>>) -> AssetError {
    AssetError::InvalidCompiledAssets {
        detail: detail.into(),
    }
}
