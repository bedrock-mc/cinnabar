use crate::{
    Animation, BlockFlags, CompiledBiomeAssets, ContributorRole, LightProperties, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, TexturePage, TextureRef, VisualKind,
    VisualSupport, provenance::BlobProvenance,
};

/// Bedrock block-face order, matching the packed renderer's face discriminants.
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockFace {
    West = 0,
    East = 1,
    Down = 2,
    Up = 3,
    North = 4,
    South = 5,
}

impl BlockFace {
    pub const ALL: [Self; 6] = [
        Self::West,
        Self::East,
        Self::Down,
        Self::Up,
        Self::North,
        Self::South,
    ];

    /// Encodes this face for the model carrier's face flag field.
    #[must_use]
    pub const fn model_quad_face_id(self) -> u32 {
        match self {
            Self::Down => 1,
            Self::Up => 2,
            Self::West => 3,
            Self::East => 4,
            Self::North => 5,
            Self::South => 6,
        }
    }

    #[doc(hidden)]
    #[must_use]
    pub const fn is_horizontal(self) -> bool {
        matches!(self, Self::West | Self::East | Self::North | Self::South)
    }
}

pub const DIAGNOSTIC_MATERIAL: u32 = 0;
pub const MAX_TEXTURE_LAYERS: usize = 2_048;
pub const MAX_MATERIALS: usize = 65_536;
pub const MATERIAL_FLAG_ROTATE_UV: u32 = 1 << 0;
pub const MATERIAL_FLAG_UV_MASK: u32 = 0x0000_000f;
pub const MATERIAL_FLAG_TINT_MASK: u32 = 0x0000_0030;
pub const MATERIAL_FLAG_GRASS_TINT: u32 = 1 << 4;
pub const MATERIAL_FLAG_FOLIAGE_TINT: u32 = 1 << 5;
pub const MATERIAL_FLAG_WATER_TINT: u32 = MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_FOLIAGE_TINT;
pub const MATERIAL_FLAG_OVERLAY_MASK: u32 = 1 << 6;
pub const MATERIAL_FLAG_ALPHA_BLEND: u32 = 1 << 7;
pub const MATERIAL_FLAG_ALPHA_CUTOUT: u32 = 1 << 8;
pub const MATERIAL_FLAG_FOLIAGE_CLASS_MASK: u32 = 0x0000_0600;
pub const MATERIAL_FLAG_BIRCH_FOLIAGE: u32 = 1 << 9;
pub const MATERIAL_FLAG_EVERGREEN_FOLIAGE: u32 = 1 << 10;
pub const MATERIAL_FLAG_DRY_FOLIAGE: u32 = MATERIAL_FLAG_FOLIAGE_CLASS_MASK;
/// Selects the opaque, depth-writing liquid pipeline used by lava.
pub const MATERIAL_FLAG_LIQUID_DEPTH_WRITE: u32 = 1 << 11;
/// World-only seasonal leaf palette selector; carried leaves keep their ordinary tint.
pub const MATERIAL_FLAG_SEASONAL_FOLIAGE: u32 = 1 << 12;
/// Selects the exposed half of the native seasonal palette.
pub const MATERIAL_FLAG_EXPOSED_FOLIAGE: u32 = 1 << 13;
/// World cutout leaf faces are visible from either side of their shared plane.
pub const MATERIAL_FLAG_TWO_SIDED: u32 = 1 << 14;
/// World leaf RGB follows native UNORM material, lighting and fog composition.
pub const MATERIAL_FLAG_NATIVE_LEAF_COLOUR: u32 = 1 << 15;
/// Applies the pack-authored positional quarter-turn to this world cube face.
pub const MATERIAL_FLAG_ISOTROPIC: u32 = 1 << 16;
/// Omits ambient occlusion while preserving solved block and sky light.
pub const MATERIAL_FLAG_DISABLE_AO: u32 = 1 << 25;
/// Omits the directional face coefficient while preserving the lightmap.
pub const MATERIAL_FLAG_DISABLE_FACE_DIMMING: u32 = 1 << 26;
/// Compact world-leaf AO exponent. Zero selects the native omitted default (1).
pub const MATERIAL_LEAF_AO_EXPONENT_SHIFT: u32 = 17;
pub const MATERIAL_LEAF_AO_EXPONENT_MAX: u32 = u8::MAX as u32;
pub const MATERIAL_LEAF_AO_EXPONENT_MASK: u32 =
    MATERIAL_LEAF_AO_EXPONENT_MAX << MATERIAL_LEAF_AO_EXPONENT_SHIFT;
pub const MATERIAL_LEAF_AO_EXPONENT_SCALE: u32 = 100;
pub const MATERIAL_LEAF_METADATA_MASK: u32 = MATERIAL_LEAF_AO_EXPONENT_MASK;

/// Decodes admitted pack-authored hundredths, without changing the native default.
#[must_use]
pub fn material_leaf_ao_exponent(flags: u32) -> f32 {
    let value = (flags & MATERIAL_LEAF_AO_EXPONENT_MASK) >> MATERIAL_LEAF_AO_EXPONENT_SHIFT;
    if value == 0 {
        1.0
    } else {
        value as f32 / MATERIAL_LEAF_AO_EXPONENT_SCALE as f32
    }
}

pub const MATERIAL_FLAGS_MASK: u32 = MATERIAL_FLAG_UV_MASK
    | MATERIAL_FLAG_TINT_MASK
    | MATERIAL_FLAG_OVERLAY_MASK
    | MATERIAL_FLAG_ALPHA_BLEND
    | MATERIAL_FLAG_ALPHA_CUTOUT
    | MATERIAL_FLAG_FOLIAGE_CLASS_MASK
    | MATERIAL_FLAG_LIQUID_DEPTH_WRITE
    | MATERIAL_FLAG_SEASONAL_FOLIAGE
    | MATERIAL_FLAG_EXPOSED_FOLIAGE
    | MATERIAL_FLAG_TWO_SIDED
    | MATERIAL_FLAG_NATIVE_LEAF_COLOUR
    | MATERIAL_FLAG_ISOTROPIC
    | MATERIAL_LEAF_METADATA_MASK
    | MATERIAL_FLAG_DISABLE_AO
    | MATERIAL_FLAG_DISABLE_FACE_DIMMING;

pub(crate) const fn material_flags_are_valid(flags: u32) -> bool {
    flags & !MATERIAL_FLAGS_MASK == 0
        && (flags & MATERIAL_LEAF_METADATA_MASK == 0
            || flags & MATERIAL_FLAG_NATIVE_LEAF_COLOUR != 0)
        && (flags & MATERIAL_FLAG_TWO_SIDED == 0 || flags & MATERIAL_FLAG_ALPHA_CUTOUT != 0)
        && (flags & MATERIAL_FLAG_NATIVE_LEAF_COLOUR == 0 || flags & MATERIAL_FLAG_ALPHA_BLEND == 0)
        && flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT)
            != MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT
        && (flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK == 0
            || flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_FOLIAGE_TINT)
        && (flags & MATERIAL_FLAG_SEASONAL_FOLIAGE == 0
            || (flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_FOLIAGE_TINT
                && flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK != MATERIAL_FLAG_DRY_FOLIAGE))
        && (flags & MATERIAL_FLAG_EXPOSED_FOLIAGE == 0
            || flags & MATERIAL_FLAG_SEASONAL_FOLIAGE != 0)
        && (flags & MATERIAL_FLAG_LIQUID_DEPTH_WRITE == 0
            || flags
                & (MATERIAL_FLAG_ALPHA_BLEND
                    | MATERIAL_FLAG_ALPHA_CUTOUT
                    | MATERIAL_FLAG_TINT_MASK)
                == 0)
}

/// One immutable GPU material-table entry.
#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Material {
    pub texture: TextureRef,
    pub flags: u32,
    pub animation: u32,
    /// Contiguous weighted materials selected after the block state and face.
    pub variation_start: u32,
    pub variation_count: u32,
    /// Normalized f32 weight bits; zero for ordinary materials.
    pub variation_weight: u32,
}

impl Material {
    /// Returns an ordinary diagnostic material with no positional alternatives.
    pub const fn unvaried() -> Self {
        Self {
            texture: TextureRef::DIAGNOSTIC,
            flags: 0,
            animation: NO_ANIMATION,
            variation_start: 0,
            variation_count: 0,
            variation_weight: 0,
        }
    }
}

const _: () = assert!(std::mem::size_of::<Material>() == 24);

/// Per-face material IDs and registry facts for one sequential block ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockVisual {
    pub faces: [u32; 6],
    pub flags: BlockFlags,
    pub kind: VisualKind,
    pub support: VisualSupport,
    pub contributor_role: ContributorRole,
    pub model_template: u32,
    pub animation: u32,
    pub variant: u32,
}

impl BlockVisual {
    #[must_use]
    pub fn diagnostic(flags: BlockFlags, contributor_role: ContributorRole) -> Self {
        Self {
            faces: [DIAGNOSTIC_MATERIAL; 6],
            flags,
            kind: VisualKind::Diagnostic,
            support: VisualSupport::Diagnostic,
            contributor_role,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }
    }
}

pub(crate) fn visual_semantics_are_valid(
    kind: VisualKind,
    support: VisualSupport,
    flags: BlockFlags,
    role: ContributorRole,
) -> bool {
    if flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        && !flags.contains(BlockFlags::CUBE_GEOMETRY)
        && !matches!(kind, VisualKind::Model)
    {
        return false;
    }
    if matches!(kind, VisualKind::Diagnostic) != matches!(support, VisualSupport::Diagnostic) {
        return false;
    }
    match kind {
        VisualKind::Diagnostic => true,
        VisualKind::Cube => {
            matches!(role, ContributorRole::Primary) && flags.contains(BlockFlags::CUBE_GEOMETRY)
        }
        VisualKind::Cross | VisualKind::Model => {
            matches!(role, ContributorRole::Primary)
                && !flags.intersects(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY)
        }
        VisualKind::Liquid => {
            matches!(role, ContributorRole::LiquidAdditional)
                && !flags.intersects(BlockFlags::AIR | BlockFlags::CUBE_GEOMETRY)
        }
        VisualKind::Invisible => {
            !matches!(role, ContributorRole::LiquidAdditional)
                && !flags.contains(BlockFlags::CUBE_GEOMETRY)
                && (matches!(role, ContributorRole::Air) == flags.contains(BlockFlags::AIR))
        }
    }
}

/// Deterministic compiler output ready for checked blob serialization.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompiledAssets {
    pub visuals: Box<[BlockVisual]>,
    pub light_properties: Box<[LightProperties]>,
    pub hashed: Box<[(u32, u32)]>,
    pub materials: Box<[Material]>,
    pub model_templates: Box<[ModelTemplate]>,
    pub model_quads: Box<[ModelQuad]>,
    pub animations: Box<[Animation]>,
    pub animation_frames: Box<[TextureRef]>,
    pub texture_pages: Box<[TexturePage]>,
    pub biomes: CompiledBiomeAssets,
    /// Exact source identities bound into the blob header. Encode rejects
    /// incomplete identity, so serialized output always names its inputs.
    pub provenance: BlobProvenance,
}
