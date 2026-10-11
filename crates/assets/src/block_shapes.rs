//! Compiled block interaction geometry and state facts.

use bitflags::bitflags;

/// Fixed-point units in one block for compiled movement coordinates and scalars.
pub const COLLISION_UNITS_PER_BLOCK: u32 = 100_000_000;
/// Converts the carrier's fixed-point quantities to simulation coordinates.
pub const COLLISION_COORDINATE_SCALE: f64 = 1.0 / COLLISION_UNITS_PER_BLOCK as f64;

/// Piston extension directions in the native piston-state order.
pub const PISTON_DIRECTIONS: [[i32; 3]; 6] = [
    [0, -1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [0, 0, -1],
    [1, 0, 0],
    [-1, 0, 0],
];

/// A block-local box. Coordinates retain the precision of their compiler inputs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl ShapeBox {
    /// Builds a box from its lower and upper corners.
    pub const fn new(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }

    /// Whether all coordinates are finite and every axis has positive extent.
    pub fn is_valid(self) -> bool {
        (0..3).all(|axis| {
            self.min[axis].is_finite()
                && self.max[axis].is_finite()
                && self.min[axis] < self.max[axis]
        })
    }
}

/// The context a ray must supply before a block can be picked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum PickRule {
    Never = 0,
    #[default]
    Always = 1,
    LiquidSource = 2,
    Liquid = 3,
    Creative = 4,
    HeldLight = 5,
}

/// Context supplied by the interaction owner rather than inferred from collision.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PickContext {
    pub creative: bool,
    pub liquids: bool,
    pub holding_light: bool,
}

impl PickRule {
    /// Applies the compiled eligibility rule to this ray's context and liquid state.
    pub fn allows(self, context: PickContext, liquid_depth: Option<u8>) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::LiquidSource => context.liquids && liquid_depth == Some(0),
            Self::Liquid => context.liquids,
            Self::Creative => context.creative,
            Self::HeldLight => context.holding_light,
        }
    }
}

bitflags! {
    /// State facts consumed without inspecting block names or state JSON.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub struct ShapeFlags: u16 {
        const FULL_CUBE = 1 << 0;
        const BUILD_INTENTION = 1 << 1;
        const BAMBOO_OFFSET = 1 << 2;
        const AIR = 1 << 3;
        const CUSTOM_SURFACE = 1 << 4;
        const HIDE_OVERLAY = 1 << 5;
        const CREATIVE_OVERLAY = 1 << 6;
        const LIGHT_BLOCK = 1 << 7;
        const DRIPSTONE_OFFSET = 1 << 8;
        const PORTAL = 1 << 9;
        const CHORUS_CONNECT = 1 << 10;
        const END_STONE = 1 << 11;
        const VINE_SUPPORT = 1 << 12;
        const FULL_TOP_SUPPORT = 1 << 13;
    }
}

/// Door properties; the other half supplies the remaining runtime context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledDoor {
    /// State-invariant class identity used to match the other half without names.
    pub family: [u8; 32],
    /// South, west, north, east, in that order.
    pub facing: u8,
    pub upper: bool,
    pub open: bool,
    pub hinge_right: bool,
}

/// Material facts for liquid flow, independent of collision geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompiledFlow {
    pub blocks_motion: bool,
    pub is_solid: bool,
    pub blocked_faces: u8,
    pub allowed_faces: u8,
    pub liquid_depth: Option<u8>,
}

/// Final state-determined selection and interaction facts for one block state.
#[derive(Debug, Clone, PartialEq)]
pub struct CompiledBlockShapes {
    pub state_identity: [u8; 32],
    /// Native six-face orientation, or absent for a class without actor geometry.
    pub actor_facing: Option<u8>,
    /// State-only visual shape used when this state is carried by a moving-block actor.
    pub actor_visual: Option<ShapeBox>,
    pub selection: Box<[ShapeBox]>,
    pub selection_context: SelectionContext,
    /// Alternate geometry selected by the runtime context rule.
    pub contextual_selection: Box<[ShapeBox]>,
    /// An outline override; absent means the union of the selection boxes.
    pub outline: Option<ShapeBox>,
    /// Outline used when the matching neighbouring block continues this shape.
    pub connected_outline: Option<ShapeBox>,
    pub outline_context: OutlineContext,
    pub pick_rule: PickRule,
    pub flags: ShapeFlags,
    pub door: Option<CompiledDoor>,
    pub flow: Option<CompiledFlow>,
    pub tags: Box<[Box<str>]>,
}

/// Neighbour-dependent outline rules; all box constants live in compiled data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum OutlineContext {
    #[default]
    Static = 0,
    TwistingVines = 1,
    WeepingVines = 2,
    Chorus = 3,
}

/// Runtime selection rules whose constants are supplied by the compiler.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum SelectionContext {
    #[default]
    Static = 0,
    PortalAxis = 1,
    VineSupport = 2,
    ScaffoldSupport = 3,
    PistonArm = 4,
    MovingBlock = 5,
}
