use crate::{AssetError, TextureArray};

pub const MAX_TEXTURE_PAGES: usize = 2;
pub const MAX_MODEL_TEMPLATES: usize = 65_536;
pub const MAX_MODEL_TEMPLATE_QUADS: usize = u32::BITS as usize;
pub const MAX_MODEL_QUADS: usize = MAX_MODEL_TEMPLATES * MAX_MODEL_TEMPLATE_QUADS;
pub const MAX_ANIMATIONS: usize = 65_536;
pub const MAX_ANIMATION_FRAMES: usize = 1_048_576;
pub const NO_MODEL_TEMPLATE: u32 = u32::MAX;
pub const NO_ANIMATION: u32 = u32::MAX;
/// Native TopSnow has this many equal-height visual layers per full block.
pub const TOP_SNOW_LAYER_COUNT: u8 = 8;
/// Semantic TopSnow identity, including the full-height cube fast path. This
/// high bit is not part of the packed model transform and preserves the native
/// snow/covered-vegetation pairing without inferring identity from its texture.
pub const BLOCK_VISUAL_VARIANT_TOP_SNOW: u32 = 1 << 31;
/// Full snow and powder snow also select the snowy side of grass below them.
pub const BLOCK_VISUAL_VARIANT_SNOW_COVER: u32 = 1 << 30;
/// Cube-only neighbor material selector. The low material bits carry the
/// untinted side material used when native snow cover is directly above grass.
pub const BLOCK_VISUAL_VARIANT_COVERED_GRASS: u32 = 1 << 29;
/// Base of world-only leaf materials: covered/exposed faces, followed by their
/// opaque deep-leaf counterparts. Carried faces remain in the ordinary table.
pub const BLOCK_VISUAL_VARIANT_SEASONAL_LEAF: u32 = 1 << 28;
pub const SEASONAL_LEAF_EXPOSED_OFFSET: u32 = crate::BlockFace::ALL.len() as u32;
pub const SEASONAL_LEAF_DEEP_OFFSET: u32 = SEASONAL_LEAF_EXPOSED_OFFSET * 2;
pub const SEASONAL_LEAF_MATERIAL_COUNT: u32 = SEASONAL_LEAF_DEEP_OFFSET * 2;
/// Season-agnostic leaves use the same cutout/deep group layout, without a
/// seasonal colour selector. Its carried face table is unchanged.
pub const BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF: u32 = 1 << 27;
pub const BLOCK_VISUAL_VARIANT_MATERIAL_MASK: u32 = crate::MAX_MATERIALS as u32 - 1;
/// Native final grass-side texture variant, precolored for snow cover.
pub const SNOWED_GRASS_SIDE_TEXTURE: &str = "textures/blocks/grass_side_snowed";

pub(crate) fn covered_grass_variant_is_valid(
    kind: VisualKind,
    variant: u32,
    material_count: usize,
) -> bool {
    if variant & BLOCK_VISUAL_VARIANT_SEASONAL_LEAF != 0 {
        let base = variant & BLOCK_VISUAL_VARIANT_MATERIAL_MASK;
        return kind == VisualKind::Cube
            && variant
                & !(BLOCK_VISUAL_VARIANT_SEASONAL_LEAF | BLOCK_VISUAL_VARIANT_MATERIAL_MASK)
                == 0
            && base != crate::DIAGNOSTIC_MATERIAL
            && base
                .checked_add(SEASONAL_LEAF_MATERIAL_COUNT)
                .is_some_and(|end| end as usize <= material_count);
    }
    if variant & BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF != 0 {
        let base = variant & BLOCK_VISUAL_VARIANT_MATERIAL_MASK;
        return kind == VisualKind::Cube
            && variant
                & !(BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF | BLOCK_VISUAL_VARIANT_MATERIAL_MASK)
                == 0
            && base != crate::DIAGNOSTIC_MATERIAL
            && base
                .checked_add(SEASONAL_LEAF_MATERIAL_COUNT)
                .is_some_and(|end| end as usize <= material_count);
    }
    if variant & BLOCK_VISUAL_VARIANT_COVERED_GRASS == 0 {
        return true;
    }
    let material = variant & BLOCK_VISUAL_VARIANT_MATERIAL_MASK;
    kind == VisualKind::Cube
        && variant & !(BLOCK_VISUAL_VARIANT_COVERED_GRASS | BLOCK_VISUAL_VARIANT_MATERIAL_MASK) == 0
        && material != crate::DIAGNOSTIC_MATERIAL
        && (material as usize) < material_count
}

/// Template selects its body or head quads from the primary block above it.
pub const MODEL_TEMPLATE_FLAG_KELP: u32 = 1 << 0;
/// Template belongs to a contiguous five-shape stair topology group.
pub const MODEL_TEMPLATE_FLAG_STAIR: u32 = 1 << 1;
/// Template continues into the immediately following part. A plain part ends
/// the chain, and each part retains one bounded visibility mask.
pub const MODEL_TEMPLATE_FLAG_COMPOUND_NEXT: u32 = 1 << 2;
/// Template belongs to a contiguous sixteen-mask thin-pane topology group.
pub const MODEL_TEMPLATE_FLAG_PANE: u32 = 1 << 3;
/// Template belongs to a wood-fence post plus sixteen-mask arm group.
pub const MODEL_TEMPLATE_FLAG_FENCE_WOOD: u32 = 1 << 4;
/// Template belongs to a nether-fence post plus sixteen-mask arm group.
pub const MODEL_TEMPLATE_FLAG_FENCE_NETHER: u32 = 1 << 5;
/// Template belongs to the connection-aware wall family.
pub const MODEL_TEMPLATE_FLAG_WALL: u32 = 1 << 6;
/// Compound fence gate whose facing direction lies on the X axis.
pub const MODEL_TEMPLATE_FLAG_GATE_AXIS_X: u32 = 1 << 7;
/// Compound fence gate whose facing direction lies on the Z axis.
pub const MODEL_TEMPLATE_FLAG_GATE_AXIS_Z: u32 = 1 << 8;
/// Standalone six-quad unit cube whose materials use alpha blending.
pub const MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE: u32 = 1 << 9;
/// Floor-anchored snow cuboid whose touching sides use height-aware occlusion.
pub const MODEL_TEMPLATE_FLAG_SNOW_LAYER: u32 = 1 << 10;
/// Native lily-pad planes use positional quarter turns and own-cell flat light.
pub const MODEL_TEMPLATE_FLAG_LILY_PAD: u32 = 1 << 11;
/// Template belongs to the supported/attached native fire topology group.
pub const MODEL_TEMPLATE_FLAG_FIRE: u32 = 1 << 12;
/// Nether portal cuboids; legacy unknown-axis visuals select between a Z/X pair.
pub const MODEL_TEMPLATE_FLAG_NETHER_PORTAL: u32 = 1 << 13;
/// Bamboo stalks and radial leaves select position-dependent stem UVs and offsets.
pub const MODEL_TEMPLATE_FLAG_BAMBOO: u32 = 1 << 14;
/// Neighbor-selected portal axis. The low two model-transform bits stay zero.
pub const BLOCK_VISUAL_VARIANT_PORTAL_UNKNOWN: u32 = 1 << 2;
pub const NETHER_PORTAL_IDENTIFIER: &str = "minecraft:portal";

/// The End portal surface is drawn by the block-entity renderer.
pub const END_PORTAL_IDENTIFIER: &str = "minecraft:end_portal";

/// The End gateway uses the same animated surface family.
pub const END_GATEWAY_IDENTIFIER: &str = "minecraft:end_gateway";

/// End portal frame state identity shared by compilation and presentation.
pub const END_PORTAL_FRAME_IDENTIFIER: &str = "minecraft:end_portal_frame";

pub(crate) fn transparent_cube_quad_geometry_is_valid(
    index: usize,
    positions: [[i16; 3]; 4],
    flags: u32,
) -> bool {
    const POSITIONS: [[[i16; 3]; 4]; 6] = [
        [[0, 0, 0], [0, 0, 256], [0, 256, 256], [0, 256, 0]],
        [[256, 0, 0], [256, 256, 0], [256, 256, 256], [256, 0, 256]],
        [[0, 0, 0], [256, 0, 0], [256, 0, 256], [0, 0, 256]],
        [[0, 256, 0], [0, 256, 256], [256, 256, 256], [256, 256, 0]],
        [[0, 0, 0], [0, 256, 0], [256, 256, 0], [256, 0, 0]],
        [[0, 0, 256], [256, 0, 256], [256, 256, 256], [0, 256, 256]],
    ];
    const FLAGS: [u32; 6] = [3, 4, 1, 2, 5, 6];
    POSITIONS.get(index) == Some(&positions) && FLAGS.get(index) == Some(&flags)
}

pub(crate) const fn model_template_flags_are_valid(flags: u32) -> bool {
    matches!(
        flags,
        0 | MODEL_TEMPLATE_FLAG_KELP
            | MODEL_TEMPLATE_FLAG_STAIR
            | MODEL_TEMPLATE_FLAG_PANE
            | MODEL_TEMPLATE_FLAG_FENCE_WOOD
            | MODEL_TEMPLATE_FLAG_FENCE_NETHER
            | MODEL_TEMPLATE_FLAG_WALL
            | MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
            | MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE
            | MODEL_TEMPLATE_FLAG_SNOW_LAYER
            | MODEL_TEMPLATE_FLAG_LILY_PAD
            | MODEL_TEMPLATE_FLAG_FIRE
            | MODEL_TEMPLATE_FLAG_NETHER_PORTAL
            | MODEL_TEMPLATE_FLAG_BAMBOO
    ) || flags == MODEL_TEMPLATE_FLAG_COMPOUND_NEXT | MODEL_TEMPLATE_FLAG_GATE_AXIS_X
        || flags == MODEL_TEMPLATE_FLAG_COMPOUND_NEXT | MODEL_TEMPLATE_FLAG_GATE_AXIS_Z
}

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

/// Evidence status of a compiled canonical block visual.
#[repr(u8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VisualSupport {
    Exact = 0,
    VanillaFallback = 1,
    Diagnostic = 2,
}

impl VisualSupport {
    pub(crate) fn from_raw(raw: u8) -> Result<Self, AssetError> {
        match raw {
            0 => Ok(Self::Exact),
            1 => Ok(Self::VanillaFallback),
            2 => Ok(Self::Diagnostic),
            _ => Err(invalid(format!("unknown visual support {raw}"))),
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

/// Returns the contiguous parts of one admitted compound model without allocating.
#[must_use]
pub fn model_template_parts(templates: &[ModelTemplate], first: u32) -> Option<&[ModelTemplate]> {
    let first = first as usize;
    let mut end = first;
    loop {
        let part = templates.get(end)?;
        end += 1;
        if part.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            return templates.get(first..end);
        }
    }
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
