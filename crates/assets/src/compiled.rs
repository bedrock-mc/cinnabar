use crate::{
    Animation, BlockFlags, CompiledBiomeAssets, ContributorRole, LightProperties, ModelQuad,
    ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE, TexturePage, TextureRef, VisualKind,
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
pub const MATERIAL_FLAGS_MASK: u32 = MATERIAL_FLAG_UV_MASK
    | MATERIAL_FLAG_TINT_MASK
    | MATERIAL_FLAG_OVERLAY_MASK
    | MATERIAL_FLAG_ALPHA_BLEND
    | MATERIAL_FLAG_ALPHA_CUTOUT
    | MATERIAL_FLAG_FOLIAGE_CLASS_MASK
    | MATERIAL_FLAG_LIQUID_DEPTH_WRITE;

pub(crate) const fn material_flags_are_valid(flags: u32) -> bool {
    flags & !MATERIAL_FLAGS_MASK == 0
        && flags & (MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT)
            != MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT
        && (flags & MATERIAL_FLAG_FOLIAGE_CLASS_MASK == 0
            || flags & MATERIAL_FLAG_TINT_MASK == MATERIAL_FLAG_FOLIAGE_TINT)
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
}

const _: () = assert!(std::mem::size_of::<Material>() == 12);

/// Per-face material IDs and registry facts for one sequential block ID.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BlockVisual {
    pub faces: [u32; 6],
    pub flags: BlockFlags,
    pub kind: VisualKind,
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
            contributor_role,
            model_template: NO_MODEL_TEMPLATE,
            animation: NO_ANIMATION,
            variant: 0,
        }
    }
}

pub(crate) fn visual_semantics_are_valid(
    kind: VisualKind,
    flags: BlockFlags,
    role: ContributorRole,
) -> bool {
    if flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        && !flags.contains(BlockFlags::CUBE_GEOMETRY)
        && !matches!(kind, VisualKind::Model)
    {
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
}
