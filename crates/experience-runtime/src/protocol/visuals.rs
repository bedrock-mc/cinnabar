//! Protocol 5's block states and visuals and the Experience's items, as `loaded` carries them:
//! server WIT 0.5's `block-type` and `item-def`, with asset paths made absolute.

use serde::{Deserialize, Serialize};

/// A point in a block, in pixels: 0 to 16 on each axis, x east, y up, z south.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pixel {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

/// An axis-aligned box in a block, in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PixelBox {
    pub min: Pixel,
    pub max: Pixel,
}

/// How a material draws (`minecraft:material_instances` `render_method`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMethod {
    Opaque,
    AlphaTest,
    Blend,
    DoubleSided,
}

/// A texture animated as a vertical strip of square frames (`flipbook_textures.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Flipbook {
    pub ticks_per_frame: u32,
    pub frames: Vec<u32>,
    pub blend_frames: bool,
}

/// One material instance; `path` is absolute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Material {
    pub instance: String,
    pub path: String,
    pub render_method: RenderMethod,
    pub flipbook: Option<Flipbook>,
}

/// The values one state takes: a bool, or 1 to 16 strings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StateValues {
    Bool,
    Choices(Vec<String>),
}

/// A state a block declares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateDef {
    pub name: String,
    pub values: StateValues,
}

/// One state's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum StateValue {
    Bool(bool),
    Choice(String),
}

/// A state and its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockState {
    pub name: String,
    pub value: StateValue,
}

/// A Bedrock placement trait state a block takes, set when it is placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementState {
    CardinalDirection,
    FacingDirection,
    BlockFace,
    VerticalHalf,
}

impl PlacementState {
    /// Every placement state, in the order a block's states list them.
    pub const ALL: [Self; 4] = [
        Self::CardinalDirection,
        Self::FacingDirection,
        Self::BlockFace,
        Self::VerticalHalf,
    ];

    /// The block state the trait adds, and its values in the order the client enumerates them:
    /// the direction, facing and vertical half values that vanilla's placement traits set on
    /// the placed block. The Dragonfly fork's traits list the same values.
    pub fn state(self) -> (&'static str, &'static [&'static str]) {
        const FACES: &[&str] = &["down", "up", "north", "south", "west", "east"];
        match self {
            Self::CardinalDirection => (
                "minecraft:cardinal_direction",
                &["south", "west", "north", "east"],
            ),
            Self::FacingDirection => ("minecraft:facing_direction", FACES),
            Self::BlockFace => ("minecraft:block_face", FACES),
            Self::VerticalHalf => ("minecraft:vertical_half", &["bottom", "top"]),
        }
    }
}

/// `state == value` when `equal`, else `state != value`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateTest {
    pub state: BlockState,
    pub equal: bool,
}

/// An or of ands of state tests.
pub type Condition = Vec<Vec<StateTest>>;

/// Quarter turns about each axis, applied x then y then z.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuarterTurns {
    pub x: u8,
    pub y: u8,
    pub z: u8,
}

/// A geometry bone that draws only while `visible` holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BoneVisibility {
    pub bone: String,
    pub visible: Condition,
}

/// What a block looks like; `geometry` and material paths are absolute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Visual {
    pub geometry: Option<String>,
    pub materials: Vec<Material>,
    pub bones: Vec<BoneVisibility>,
    pub collision: Option<PixelBox>,
    pub selection: Option<PixelBox>,
    pub rotation: Option<QuarterTurns>,
}

/// The parts of a visual that replace the block's while `when` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Permutation {
    pub when: Condition,
    pub geometry: Option<String>,
    pub materials: Option<Vec<Material>>,
    pub bones: Option<Vec<BoneVisibility>>,
    pub collision: Option<PixelBox>,
    pub selection: Option<PixelBox>,
    pub rotation: Option<QuarterTurns>,
}

/// An item of the Experience; `icon` is absolute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDef {
    pub id: String,
    pub display_name: String,
    pub icon: String,
    pub max_stack: u8,
}
