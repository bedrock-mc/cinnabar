//! Generated block state types.
//! Do not edit: regenerate with valentine_gen.

#![allow(clippy::manual_is_multiple_of)]

use valentine_bedrock_core::block::BlockState;

// ===== SHARED ENUMS =====

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Attachment {
    #[default]
    Standing = 0,
    Hanging = 1,
    Side = 2,
    Multiple = 3,
}

impl Attachment {
    pub const COUNT: u32 = 4;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Standing),
            1 => Some(Self::Hanging),
            2 => Some(Self::Side),
            3 => Some(Self::Multiple),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BambooLeafSize {
    #[default]
    NoLeaves = 0,
    SmallLeaves = 1,
    LargeLeaves = 2,
}

impl BambooLeafSize {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::NoLeaves),
            1 => Some(Self::SmallLeaves),
            2 => Some(Self::LargeLeaves),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BambooStalkThickness {
    #[default]
    Thin = 0,
    Thick = 1,
}

impl BambooStalkThickness {
    pub const COUNT: u32 = 2;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Thin),
            1 => Some(Self::Thick),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BigDripleafTilt {
    #[default]
    None = 0,
    Unstable = 1,
    PartialTilt = 2,
    FullTilt = 3,
}

impl BigDripleafTilt {
    pub const COUNT: u32 = 4;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Unstable),
            2 => Some(Self::PartialTilt),
            3 => Some(Self::FullTilt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum BlockFace {
    #[default]
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl BlockFace {
    pub const COUNT: u32 = 6;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            2 => Some(Self::North),
            3 => Some(Self::South),
            4 => Some(Self::West),
            5 => Some(Self::East),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum CardinalDirection {
    #[default]
    South = 0,
    West = 1,
    North = 2,
    East = 3,
}

impl CardinalDirection {
    pub const COUNT: u32 = 4;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::South),
            1 => Some(Self::West),
            2 => Some(Self::North),
            3 => Some(Self::East),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum CauldronLiquid {
    #[default]
    Water = 0,
    Lava = 1,
    PowderSnow = 2,
}

impl CauldronLiquid {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Water),
            1 => Some(Self::Lava),
            2 => Some(Self::PowderSnow),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum CrackedState {
    #[default]
    NoCracks = 0,
    Cracked = 1,
    MaxCracked = 2,
}

impl CrackedState {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::NoCracks),
            1 => Some(Self::Cracked),
            2 => Some(Self::MaxCracked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum CreakingHeartState {
    #[default]
    Uprooted = 0,
    Dormant = 1,
    Awake = 2,
}

impl CreakingHeartState {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Uprooted),
            1 => Some(Self::Dormant),
            2 => Some(Self::Awake),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum DripstoneThickness {
    #[default]
    Tip = 0,
    Frustum = 1,
    Middle = 2,
    Base = 3,
    Merge = 4,
}

impl DripstoneThickness {
    pub const COUNT: u32 = 5;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Tip),
            1 => Some(Self::Frustum),
            2 => Some(Self::Middle),
            3 => Some(Self::Base),
            4 => Some(Self::Merge),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum FacingDirection {
    #[default]
    Down = 0,
    Up = 1,
    North = 2,
    South = 3,
    West = 4,
    East = 5,
}

impl FacingDirection {
    pub const COUNT: u32 = 6;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Down),
            1 => Some(Self::Up),
            2 => Some(Self::North),
            3 => Some(Self::South),
            4 => Some(Self::West),
            5 => Some(Self::East),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum LeverDirection {
    #[default]
    DownEastWest = 0,
    East = 1,
    West = 2,
    South = 3,
    North = 4,
    UpNorthSouth = 5,
    UpEastWest = 6,
    DownNorthSouth = 7,
}

impl LeverDirection {
    pub const COUNT: u32 = 8;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::DownEastWest),
            1 => Some(Self::East),
            2 => Some(Self::West),
            3 => Some(Self::South),
            4 => Some(Self::North),
            5 => Some(Self::UpNorthSouth),
            6 => Some(Self::UpEastWest),
            7 => Some(Self::DownNorthSouth),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Orientation {
    #[default]
    DownEast = 0,
    DownNorth = 1,
    DownSouth = 2,
    DownWest = 3,
    UpEast = 4,
    UpNorth = 5,
    UpSouth = 6,
    UpWest = 7,
    WestUp = 8,
    EastUp = 9,
    NorthUp = 10,
    SouthUp = 11,
}

impl Orientation {
    pub const COUNT: u32 = 12;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::DownEast),
            1 => Some(Self::DownNorth),
            2 => Some(Self::DownSouth),
            3 => Some(Self::DownWest),
            4 => Some(Self::UpEast),
            5 => Some(Self::UpNorth),
            6 => Some(Self::UpSouth),
            7 => Some(Self::UpWest),
            8 => Some(Self::WestUp),
            9 => Some(Self::EastUp),
            10 => Some(Self::NorthUp),
            11 => Some(Self::SouthUp),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PaleMossCarpetSideEast {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl PaleMossCarpetSideEast {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PaleMossCarpetSideNorth {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl PaleMossCarpetSideNorth {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PaleMossCarpetSideSouth {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl PaleMossCarpetSideSouth {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PaleMossCarpetSideWest {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl PaleMossCarpetSideWest {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PillarAxis {
    #[default]
    Y = 0,
    X = 1,
    Z = 2,
}

impl PillarAxis {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Y),
            1 => Some(Self::X),
            2 => Some(Self::Z),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum PortalAxis {
    #[default]
    Unknown = 0,
    X = 1,
    Z = 2,
}

impl PortalAxis {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::X),
            2 => Some(Self::Z),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum SeaGrassType {
    #[default]
    Default = 0,
    DoubleTop = 1,
    DoubleBot = 2,
}

impl SeaGrassType {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Default),
            1 => Some(Self::DoubleTop),
            2 => Some(Self::DoubleBot),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum StructureBlockType {
    #[default]
    Data = 0,
    Save = 1,
    Load = 2,
    Corner = 3,
    Invalid = 4,
    Export = 5,
}

impl StructureBlockType {
    pub const COUNT: u32 = 6;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Data),
            1 => Some(Self::Save),
            2 => Some(Self::Load),
            3 => Some(Self::Corner),
            4 => Some(Self::Invalid),
            5 => Some(Self::Export),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum TorchFacingDirection {
    #[default]
    Unknown = 0,
    West = 1,
    East = 2,
    North = 3,
    South = 4,
    Top = 5,
}

impl TorchFacingDirection {
    pub const COUNT: u32 = 6;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Unknown),
            1 => Some(Self::West),
            2 => Some(Self::East),
            3 => Some(Self::North),
            4 => Some(Self::South),
            5 => Some(Self::Top),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum TurtleEggCount {
    #[default]
    OneEgg = 0,
    TwoEgg = 1,
    ThreeEgg = 2,
    FourEgg = 3,
}

impl TurtleEggCount {
    pub const COUNT: u32 = 4;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::OneEgg),
            1 => Some(Self::TwoEgg),
            2 => Some(Self::ThreeEgg),
            3 => Some(Self::FourEgg),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum VaultState {
    #[default]
    Inactive = 0,
    Active = 1,
    Unlocking = 2,
    Ejecting = 3,
}

impl VaultState {
    pub const COUNT: u32 = 4;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Inactive),
            1 => Some(Self::Active),
            2 => Some(Self::Unlocking),
            3 => Some(Self::Ejecting),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum VerticalHalf {
    #[default]
    Bottom = 0,
    Top = 1,
}

impl VerticalHalf {
    pub const COUNT: u32 = 2;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::Bottom),
            1 => Some(Self::Top),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum WallConnectionTypeEast {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl WallConnectionTypeEast {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum WallConnectionTypeNorth {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl WallConnectionTypeNorth {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum WallConnectionTypeSouth {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl WallConnectionTypeSouth {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum WallConnectionTypeWest {
    #[default]
    None = 0,
    Short = 1,
    Tall = 2,
}

impl WallConnectionTypeWest {
    pub const COUNT: u32 = 3;
    pub fn from_raw(v: u8) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Short),
            2 => Some(Self::Tall),
            _ => None,
        }
    }
}

// ===== SHARED STATE STRUCTS =====

/// State shared by: ["pale_oak_double_slab", "polished_diorite_double_slab", "prismarine_slab", "tuff_brick_double_slab", "polished_blackstone_slab"]
/// ... and 119 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlabState {
    vertical_half: VerticalHalf,
}

impl SlabState {
    /// Create a new state with validation.
    pub fn new(
        vertical_half: VerticalHalf,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { vertical_half })
    }

    /// Get the vertical_half value.
    #[inline]
    pub fn vertical_half(&self) -> VerticalHalf {
        self.vertical_half
    }
}

impl BlockState for SlabState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.vertical_half as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let vertical_half = VerticalHalf::from_raw((rem % 2) as u8)?;
        Some(Self { vertical_half })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["quartz_pillar", "warped_stem", "stripped_cherry_wood", "stripped_birch_wood", "iron_chain"]
/// ... and 66 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PillarState {
    pillar_axis: PillarAxis,
}

impl PillarState {
    /// Create a new state with validation.
    pub fn new(pillar_axis: PillarAxis) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { pillar_axis })
    }

    /// Get the pillar_axis value.
    #[inline]
    pub fn pillar_axis(&self) -> PillarAxis {
        self.pillar_axis
    }
}

impl BlockState for PillarState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.pillar_axis as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 3 {
            return None;
        }
        let rem = offset;
        let pillar_axis = PillarAxis::from_raw((rem % 3) as u8)?;
        Some(Self { pillar_axis })
    }

    fn state_count() -> u32 {
        3
    }
}

/// State shared by: ["cherry_stairs", "spruce_stairs", "mangrove_stairs", "acacia_stairs", "dark_prismarine_stairs"]
/// ... and 53 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StairState {
    upside_down_bit: bool,
    weirdo_direction: u8,
}

impl StairState {
    /// Create a new state with validation.
    pub fn new(
        upside_down_bit: bool,
        weirdo_direction: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if weirdo_direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "weirdo_direction",
                value: weirdo_direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            upside_down_bit,
            weirdo_direction,
        })
    }

    /// Get the upside_down_bit value.
    #[inline]
    pub fn upside_down_bit(&self) -> bool {
        self.upside_down_bit
    }
    /// Get the weirdo_direction value.
    #[inline]
    pub fn weirdo_direction(&self) -> u8 {
        self.weirdo_direction
    }
}

impl BlockState for StairState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.upside_down_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.weirdo_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let upside_down_bit = (rem % 2) != 0;
        rem /= 2;
        let weirdo_direction = (rem % 4) as u8;
        Some(Self {
            upside_down_bit,
            weirdo_direction,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["cyan_glazed_terracotta", "wither_skeleton_skull", "darkoak_wall_sign", "light_blue_glazed_terracotta", "green_glazed_terracotta"]
/// ... and 37 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FacingState {
    facing_direction: u8,
}

impl FacingState {
    /// Create a new state with validation.
    pub fn new(facing_direction: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self { facing_direction })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
}

impl BlockState for FacingState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 6 {
            return None;
        }
        let rem = offset;
        let facing_direction = (rem % 6) as u8;
        Some(Self { facing_direction })
    }

    fn state_count() -> u32 {
        6
    }
}

/// State shared by: ["oxidized_copper_golem_statue", "weathered_copper_golem_statue", "waxed_copper_chest", "anvil", "blast_furnace"]
/// ... and 28 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CardinalState {
    cardinal_direction: CardinalDirection,
}

impl CardinalState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { cardinal_direction })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
}

impl BlockState for CardinalState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 4 {
            return None;
        }
        let rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        Some(Self { cardinal_direction })
    }

    fn state_count() -> u32 {
        4
    }
}

/// State shared by: ["tuff_brick_wall", "andesite_wall", "cobblestone_wall", "polished_deepslate_wall", "deepslate_brick_wall"]
/// ... and 22 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WallState {
    wall_connection_type_east: WallConnectionTypeEast,
    wall_connection_type_north: WallConnectionTypeNorth,
    wall_connection_type_south: WallConnectionTypeSouth,
    wall_connection_type_west: WallConnectionTypeWest,
    wall_post_bit: bool,
}

impl WallState {
    /// Create a new state with validation.
    pub fn new(
        wall_connection_type_east: WallConnectionTypeEast,
        wall_connection_type_north: WallConnectionTypeNorth,
        wall_connection_type_south: WallConnectionTypeSouth,
        wall_connection_type_west: WallConnectionTypeWest,
        wall_post_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            wall_connection_type_east,
            wall_connection_type_north,
            wall_connection_type_south,
            wall_connection_type_west,
            wall_post_bit,
        })
    }

    /// Get the wall_connection_type_east value.
    #[inline]
    pub fn wall_connection_type_east(&self) -> WallConnectionTypeEast {
        self.wall_connection_type_east
    }
    /// Get the wall_connection_type_north value.
    #[inline]
    pub fn wall_connection_type_north(&self) -> WallConnectionTypeNorth {
        self.wall_connection_type_north
    }
    /// Get the wall_connection_type_south value.
    #[inline]
    pub fn wall_connection_type_south(&self) -> WallConnectionTypeSouth {
        self.wall_connection_type_south
    }
    /// Get the wall_connection_type_west value.
    #[inline]
    pub fn wall_connection_type_west(&self) -> WallConnectionTypeWest {
        self.wall_connection_type_west
    }
    /// Get the wall_post_bit value.
    #[inline]
    pub fn wall_post_bit(&self) -> bool {
        self.wall_post_bit
    }
}

impl BlockState for WallState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.wall_connection_type_east as u32) * multiplier;
        multiplier *= 3;
        offset += (self.wall_connection_type_north as u32) * multiplier;
        multiplier *= 3;
        offset += (self.wall_connection_type_south as u32) * multiplier;
        multiplier *= 3;
        offset += (self.wall_connection_type_west as u32) * multiplier;
        multiplier *= 3;
        offset += (self.wall_post_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 162 {
            return None;
        }
        let mut rem = offset;
        let wall_connection_type_east = WallConnectionTypeEast::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let wall_connection_type_north = WallConnectionTypeNorth::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let wall_connection_type_south = WallConnectionTypeSouth::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let wall_connection_type_west = WallConnectionTypeWest::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let wall_post_bit = (rem % 2) != 0;
        Some(Self {
            wall_connection_type_east,
            wall_connection_type_north,
            wall_connection_type_south,
            wall_connection_type_west,
            wall_post_bit,
        })
    }

    fn state_count() -> u32 {
        162
    }
}

/// State shared by: ["dark_oak_door", "cherry_door", "wooden_door", "crimson_door", "mangrove_door"]
/// ... and 16 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DoorState {
    door_hinge_bit: bool,
    cardinal_direction: CardinalDirection,
    open_bit: bool,
    upper_block_bit: bool,
}

impl DoorState {
    /// Create a new state with validation.
    pub fn new(
        door_hinge_bit: bool,
        cardinal_direction: CardinalDirection,
        open_bit: bool,
        upper_block_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            door_hinge_bit,
            cardinal_direction,
            open_bit,
            upper_block_bit,
        })
    }

    /// Get the door_hinge_bit value.
    #[inline]
    pub fn door_hinge_bit(&self) -> bool {
        self.door_hinge_bit
    }
    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the open_bit value.
    #[inline]
    pub fn open_bit(&self) -> bool {
        self.open_bit
    }
    /// Get the upper_block_bit value.
    #[inline]
    pub fn upper_block_bit(&self) -> bool {
        self.upper_block_bit
    }
}

impl BlockState for DoorState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.door_hinge_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.open_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.upper_block_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 32 {
            return None;
        }
        let mut rem = offset;
        let door_hinge_bit = (rem % 2) != 0;
        rem /= 2;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let open_bit = (rem % 2) != 0;
        rem /= 2;
        let upper_block_bit = (rem % 2) != 0;
        Some(Self {
            door_hinge_bit,
            cardinal_direction,
            open_bit,
            upper_block_bit,
        })
    }

    fn state_count() -> u32 {
        32
    }
}

/// State shared by: ["waxed_weathered_copper_trapdoor", "spruce_trapdoor", "acacia_trapdoor", "exposed_copper_trapdoor", "mangrove_trapdoor"]
/// ... and 16 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrapdoorState {
    direction: u8,
    open_bit: bool,
    upside_down_bit: bool,
}

impl TrapdoorState {
    /// Create a new state with validation.
    pub fn new(
        direction: u8,
        open_bit: bool,
        upside_down_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            direction,
            open_bit,
            upside_down_bit,
        })
    }

    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
    /// Get the open_bit value.
    #[inline]
    pub fn open_bit(&self) -> bool {
        self.open_bit
    }
    /// Get the upside_down_bit value.
    #[inline]
    pub fn upside_down_bit(&self) -> bool {
        self.upside_down_bit
    }
}

impl BlockState for TrapdoorState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.open_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.upside_down_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let direction = (rem % 4) as u8;
        rem /= 4;
        let open_bit = (rem % 2) != 0;
        rem /= 2;
        let upside_down_bit = (rem % 2) != 0;
        Some(Self {
            direction,
            open_bit,
            upside_down_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["jungle_pressure_plate", "pale_oak_pressure_plate", "daylight_detector", "daylight_detector_inverted", "stone_pressure_plate"]
/// ... and 14 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PressurePlateState {
    redstone_signal: u8,
}

impl PressurePlateState {
    /// Create a new state with validation.
    pub fn new(redstone_signal: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if redstone_signal > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "redstone_signal",
                value: redstone_signal as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self { redstone_signal })
    }

    /// Get the redstone_signal value.
    #[inline]
    pub fn redstone_signal(&self) -> u8 {
        self.redstone_signal
    }
}

impl BlockState for PressurePlateState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.redstone_signal as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let rem = offset;
        let redstone_signal = (rem % 16) as u8;
        Some(Self { redstone_signal })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["candle", "white_candle", "light_blue_candle", "yellow_candle", "light_gray_candle"]
/// ... and 12 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandleState {
    candles: u8,
    lit: bool,
}

impl CandleState {
    /// Create a new state with validation.
    pub fn new(candles: u8, lit: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if candles > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "candles",
                value: candles as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self { candles, lit })
    }

    /// Get the candles value.
    #[inline]
    pub fn candles(&self) -> u8 {
        self.candles
    }
    /// Get the lit value.
    #[inline]
    pub fn lit(&self) -> bool {
        self.lit
    }
}

impl BlockState for CandleState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.candles as u32) * multiplier;
        multiplier *= 4;
        offset += (self.lit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let candles = (rem % 4) as u8;
        rem /= 4;
        let lit = (rem % 2) != 0;
        Some(Self { candles, lit })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["light_blue_candle_cake", "yellow_candle_cake", "magenta_candle_cake", "blue_candle_cake", "pink_candle_cake"]
/// ... and 12 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CandleCakeState {
    lit: bool,
}

impl CandleCakeState {
    /// Create a new state with validation.
    pub fn new(lit: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { lit })
    }

    /// Get the lit value.
    #[inline]
    pub fn lit(&self) -> bool {
        self.lit
    }
}

impl BlockState for CandleCakeState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.lit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let lit = (rem % 2) != 0;
        Some(Self { lit })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["pale_oak_button", "jungle_button", "crimson_button", "polished_blackstone_button", "stone_button"]
/// ... and 9 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ButtonState {
    button_pressed_bit: bool,
    facing_direction: u8,
}

impl ButtonState {
    /// Create a new state with validation.
    pub fn new(
        button_pressed_bit: bool,
        facing_direction: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            button_pressed_bit,
            facing_direction,
        })
    }

    /// Get the button_pressed_bit value.
    #[inline]
    pub fn button_pressed_bit(&self) -> bool {
        self.button_pressed_bit
    }
    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
}

impl BlockState for ButtonState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.button_pressed_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.facing_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let button_pressed_bit = (rem % 2) != 0;
        rem /= 2;
        let facing_direction = (rem % 6) as u8;
        Some(Self {
            button_pressed_bit,
            facing_direction,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["cherry_standing_sign", "mangrove_standing_sign", "darkoak_standing_sign", "jungle_standing_sign", "pale_oak_standing_sign"]
/// ... and 8 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StandingSignState {
    ground_sign_direction: u8,
}

impl StandingSignState {
    /// Create a new state with validation.
    pub fn new(
        ground_sign_direction: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if ground_sign_direction > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "ground_sign_direction",
                value: ground_sign_direction as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self {
            ground_sign_direction,
        })
    }

    /// Get the ground_sign_direction value.
    #[inline]
    pub fn ground_sign_direction(&self) -> u8 {
        self.ground_sign_direction
    }
}

impl BlockState for StandingSignState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.ground_sign_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let rem = offset;
        let ground_sign_direction = (rem % 16) as u8;
        Some(Self {
            ground_sign_direction,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["birch_fence_gate", "jungle_fence_gate", "crimson_fence_gate", "mangrove_fence_gate", "bamboo_fence_gate"]
/// ... and 7 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FenceGateState {
    in_wall_bit: bool,
    cardinal_direction: CardinalDirection,
    open_bit: bool,
}

impl FenceGateState {
    /// Create a new state with validation.
    pub fn new(
        in_wall_bit: bool,
        cardinal_direction: CardinalDirection,
        open_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            in_wall_bit,
            cardinal_direction,
            open_bit,
        })
    }

    /// Get the in_wall_bit value.
    #[inline]
    pub fn in_wall_bit(&self) -> bool {
        self.in_wall_bit
    }
    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the open_bit value.
    #[inline]
    pub fn open_bit(&self) -> bool {
        self.open_bit
    }
}

impl BlockState for FenceGateState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.in_wall_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.open_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let in_wall_bit = (rem % 2) != 0;
        rem /= 2;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let open_bit = (rem % 2) != 0;
        Some(Self {
            in_wall_bit,
            cardinal_direction,
            open_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["birch_hanging_sign", "acacia_hanging_sign", "dark_oak_hanging_sign", "cherry_hanging_sign", "bamboo_hanging_sign"]
/// ... and 7 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HangingSignState {
    attached_bit: bool,
    facing_direction: u8,
    ground_sign_direction: u8,
    hanging: bool,
}

impl HangingSignState {
    /// Create a new state with validation.
    pub fn new(
        attached_bit: bool,
        facing_direction: u8,
        ground_sign_direction: u8,
        hanging: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        if ground_sign_direction > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "ground_sign_direction",
                value: ground_sign_direction as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self {
            attached_bit,
            facing_direction,
            ground_sign_direction,
            hanging,
        })
    }

    /// Get the attached_bit value.
    #[inline]
    pub fn attached_bit(&self) -> bool {
        self.attached_bit
    }
    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the ground_sign_direction value.
    #[inline]
    pub fn ground_sign_direction(&self) -> u8 {
        self.ground_sign_direction
    }
    /// Get the hanging value.
    #[inline]
    pub fn hanging(&self) -> bool {
        self.hanging
    }
}

impl BlockState for HangingSignState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.attached_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.ground_sign_direction as u32) * multiplier;
        multiplier *= 16;
        offset += (self.hanging as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 384 {
            return None;
        }
        let mut rem = offset;
        let attached_bit = (rem % 2) != 0;
        rem /= 2;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let ground_sign_direction = (rem % 16) as u8;
        rem /= 16;
        let hanging = (rem % 2) != 0;
        Some(Self {
            attached_bit,
            facing_direction,
            ground_sign_direction,
            hanging,
        })
    }

    fn state_count() -> u32 {
        384
    }
}

/// State shared by: ["warped_shelf", "cherry_shelf", "mangrove_shelf", "acacia_shelf", "jungle_shelf"]
/// ... and 7 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ShelfState {
    cardinal_direction: CardinalDirection,
    powered_bit: bool,
    powered_shelf_type: u8,
}

impl ShelfState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        powered_bit: bool,
        powered_shelf_type: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if powered_shelf_type > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "powered_shelf_type",
                value: powered_shelf_type as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            cardinal_direction,
            powered_bit,
            powered_shelf_type,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the powered_bit value.
    #[inline]
    pub fn powered_bit(&self) -> bool {
        self.powered_bit
    }
    /// Get the powered_shelf_type value.
    #[inline]
    pub fn powered_shelf_type(&self) -> u8 {
        self.powered_shelf_type
    }
}

impl BlockState for ShelfState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.powered_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.powered_shelf_type as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 32 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let powered_bit = (rem % 2) != 0;
        rem /= 2;
        let powered_shelf_type = (rem % 4) as u8;
        Some(Self {
            cardinal_direction,
            powered_bit,
            powered_shelf_type,
        })
    }

    fn state_count() -> u32 {
        32
    }
}

/// State shared by: ["cherry_leaves", "pale_oak_leaves", "acacia_leaves", "dark_oak_leaves", "oak_leaves"]
/// ... and 6 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeavesState {
    persistent_bit: bool,
    update_bit: bool,
}

impl LeavesState {
    /// Create a new state with validation.
    pub fn new(
        persistent_bit: bool,
        update_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            persistent_bit,
            update_bit,
        })
    }

    /// Get the persistent_bit value.
    #[inline]
    pub fn persistent_bit(&self) -> bool {
        self.persistent_bit
    }
    /// Get the update_bit value.
    #[inline]
    pub fn update_bit(&self) -> bool {
        self.update_bit
    }
}

impl BlockState for LeavesState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.persistent_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.update_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 4 {
            return None;
        }
        let mut rem = offset;
        let persistent_bit = (rem % 2) != 0;
        rem /= 2;
        let update_bit = (rem % 2) != 0;
        Some(Self {
            persistent_bit,
            update_bit,
        })
    }

    fn state_count() -> u32 {
        4
    }
}

/// State shared by: ["dead_fire_coral_wall_fan", "dead_horn_coral_wall_fan", "fire_coral_wall_fan", "horn_coral_wall_fan", "dead_bubble_coral_wall_fan"]
/// ... and 5 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoralWallFanState {
    coral_direction: u8,
}

impl CoralWallFanState {
    /// Create a new state with validation.
    pub fn new(coral_direction: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if coral_direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "coral_direction",
                value: coral_direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self { coral_direction })
    }

    /// Get the coral_direction value.
    #[inline]
    pub fn coral_direction(&self) -> u8 {
        self.coral_direction
    }
}

impl BlockState for CoralWallFanState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.coral_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 4 {
            return None;
        }
        let rem = offset;
        let coral_direction = (rem % 4) as u8;
        Some(Self { coral_direction })
    }

    fn state_count() -> u32 {
        4
    }
}

/// State shared by: ["copper_lantern", "waxed_copper_lantern", "lantern", "soul_lantern", "waxed_exposed_copper_lantern"]
/// ... and 5 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LanternState {
    hanging: bool,
}

impl LanternState {
    /// Create a new state with validation.
    pub fn new(hanging: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { hanging })
    }

    /// Get the hanging value.
    #[inline]
    pub fn hanging(&self) -> bool {
        self.hanging
    }
}

impl BlockState for LanternState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.hanging as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let hanging = (rem % 2) != 0;
        Some(Self { hanging })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["dead_fire_coral_fan", "fire_coral_fan", "dead_tube_coral_fan", "tube_coral_fan", "horn_coral_fan"]
/// ... and 5 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoralFanState {
    coral_fan_direction: u8,
}

impl CoralFanState {
    /// Create a new state with validation.
    pub fn new(coral_fan_direction: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            coral_fan_direction,
        })
    }

    /// Get the coral_fan_direction value.
    #[inline]
    pub fn coral_fan_direction(&self) -> u8 {
        self.coral_fan_direction
    }
}

impl BlockState for CoralFanState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.coral_fan_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let coral_fan_direction = (rem % 2) as u8;
        Some(Self {
            coral_fan_direction,
        })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["redstone_torch", "colored_torch_green", "unlit_redstone_torch", "colored_torch_red", "copper_torch"]
/// ... and 5 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TorchState {
    torch_facing_direction: TorchFacingDirection,
}

impl TorchState {
    /// Create a new state with validation.
    pub fn new(
        torch_facing_direction: TorchFacingDirection,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            torch_facing_direction,
        })
    }

    /// Get the torch_facing_direction value.
    #[inline]
    pub fn torch_facing_direction(&self) -> TorchFacingDirection {
        self.torch_facing_direction
    }
}

impl BlockState for TorchState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.torch_facing_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 6 {
            return None;
        }
        let rem = offset;
        let torch_facing_direction = TorchFacingDirection::from_raw((rem % 6) as u8)?;
        Some(Self {
            torch_facing_direction,
        })
    }

    fn state_count() -> u32 {
        6
    }
}

/// State shared by: ["oak_sapling", "spruce_sapling", "acacia_sapling", "bamboo_sapling", "cherry_sapling"]
/// ... and 4 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SaplingState {
    age_bit: bool,
}

impl SaplingState {
    /// Create a new state with validation.
    pub fn new(age_bit: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { age_bit })
    }

    /// Get the age_bit value.
    #[inline]
    pub fn age_bit(&self) -> bool {
        self.age_bit
    }
}

impl BlockState for SaplingState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.age_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let age_bit = (rem % 2) != 0;
        Some(Self { age_bit })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["oxidized_lightning_rod", "waxed_oxidized_lightning_rod", "waxed_lightning_rod", "lightning_rod", "weathered_lightning_rod"]
/// ... and 3 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LightningRodState {
    facing_direction: u8,
    powered_bit: bool,
}

impl LightningRodState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: u8,
        powered_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            facing_direction,
            powered_bit,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the powered_bit value.
    #[inline]
    pub fn powered_bit(&self) -> bool {
        self.powered_bit
    }
}

impl BlockState for LightningRodState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.powered_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let powered_bit = (rem % 2) != 0;
        Some(Self {
            facing_direction,
            powered_bit,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["waxed_weathered_copper_bulb", "weathered_copper_bulb", "copper_bulb", "waxed_oxidized_copper_bulb", "waxed_exposed_copper_bulb"]
/// ... and 3 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CopperBulbState {
    lit: bool,
    powered_bit: bool,
}

impl CopperBulbState {
    /// Create a new state with validation.
    pub fn new(
        lit: bool,
        powered_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { lit, powered_bit })
    }

    /// Get the lit value.
    #[inline]
    pub fn lit(&self) -> bool {
        self.lit
    }
    /// Get the powered_bit value.
    #[inline]
    pub fn powered_bit(&self) -> bool {
        self.powered_bit
    }
}

impl BlockState for CopperBulbState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.lit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.powered_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 4 {
            return None;
        }
        let mut rem = offset;
        let lit = (rem % 2) != 0;
        rem /= 2;
        let powered_bit = (rem % 2) != 0;
        Some(Self { lit, powered_bit })
    }

    fn state_count() -> u32 {
        4
    }
}

/// State shared by: ["sunflower", "rose_bush", "pitcher_plant", "tall_grass", "lilac"]
/// ... and 2 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DoublePlantState {
    upper_block_bit: bool,
}

impl DoublePlantState {
    /// Create a new state with validation.
    pub fn new(upper_block_bit: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { upper_block_bit })
    }

    /// Get the upper_block_bit value.
    #[inline]
    pub fn upper_block_bit(&self) -> bool {
        self.upper_block_bit
    }
}

impl BlockState for DoublePlantState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.upper_block_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let upper_block_bit = (rem % 2) != 0;
        Some(Self { upper_block_bit })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["torchflower_crop", "beetroot", "carrots", "wheat", "sweet_berry_bush"]
/// ... and 1 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CropState {
    growth: u8,
}

impl CropState {
    /// Create a new state with validation.
    pub fn new(growth: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if growth > 7 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "growth",
                value: growth as u32,
                min: 0,
                max: 7,
            });
        }
        Ok(Self { growth })
    }

    /// Get the growth value.
    #[inline]
    pub fn growth(&self) -> u8 {
        self.growth
    }
}

impl BlockState for CropState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.growth as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let rem = offset;
        let growth = (rem % 8) as u8;
        Some(Self { growth })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["material_reducer", "lab_table", "loom", "element_constructor", "decorated_pot"]
/// ... and 1 more blocks
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirectionState {
    direction: u8,
}

impl DirectionState {
    /// Create a new state with validation.
    pub fn new(direction: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self { direction })
    }

    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
}

impl BlockState for DirectionState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 4 {
            return None;
        }
        let rem = offset;
        let direction = (rem % 4) as u8;
        Some(Self { direction })
    }

    fn state_count() -> u32 {
        4
    }
}

/// State shared by: ["flowing_water", "water", "flowing_lava", "lava"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LiquidState {
    liquid_depth: u8,
}

impl LiquidState {
    /// Create a new state with validation.
    pub fn new(liquid_depth: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if liquid_depth > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "liquid_depth",
                value: liquid_depth as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self { liquid_depth })
    }

    /// Get the liquid_depth value.
    #[inline]
    pub fn liquid_depth(&self) -> u8 {
        self.liquid_depth
    }
}

impl BlockState for LiquidState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.liquid_depth as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let rem = offset;
        let liquid_depth = (rem % 16) as u8;
        Some(Self { liquid_depth })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["reeds", "fire", "soul_fire", "cactus"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgeState {
    age: u8,
}

impl AgeState {
    /// Create a new state with validation.
    pub fn new(age: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if age > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "age",
                value: age as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self { age })
    }

    /// Get the age value.
    #[inline]
    pub fn age(&self) -> u8 {
        self.age
    }
}

impl BlockState for AgeState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.age as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let rem = offset;
        let age = (rem % 16) as u8;
        Some(Self { age })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["medium_amethyst_bud", "large_amethyst_bud", "amethyst_cluster", "small_amethyst_bud"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockFaceState {
    block_face: BlockFace,
}

impl BlockFaceState {
    /// Create a new state with validation.
    pub fn new(block_face: BlockFace) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { block_face })
    }

    /// Get the block_face value.
    #[inline]
    pub fn block_face(&self) -> BlockFace {
        self.block_face
    }
}

impl BlockState for BlockFaceState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.block_face as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 6 {
            return None;
        }
        let rem = offset;
        let block_face = BlockFace::from_raw((rem % 6) as u8)?;
        Some(Self { block_face })
    }

    fn state_count() -> u32 {
        6
    }
}

/// State shared by: ["activator_rail", "golden_rail", "detector_rail"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RailState {
    rail_data_bit: bool,
    rail_direction: u8,
}

impl RailState {
    /// Create a new state with validation.
    pub fn new(
        rail_data_bit: bool,
        rail_direction: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if rail_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "rail_direction",
                value: rail_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            rail_data_bit,
            rail_direction,
        })
    }

    /// Get the rail_data_bit value.
    #[inline]
    pub fn rail_data_bit(&self) -> bool {
        self.rail_data_bit
    }
    /// Get the rail_direction value.
    #[inline]
    pub fn rail_direction(&self) -> u8 {
        self.rail_direction
    }
}

impl BlockState for RailState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.rail_data_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.rail_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let rail_data_bit = (rem % 2) != 0;
        rem /= 2;
        let rail_direction = (rem % 6) as u8;
        Some(Self {
            rail_data_bit,
            rail_direction,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["glow_lichen", "resin_clump", "sculk_vein"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MultiFaceState {
    multi_face_direction_bits: u8,
}

impl MultiFaceState {
    /// Create a new state with validation.
    pub fn new(
        multi_face_direction_bits: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if multi_face_direction_bits > 63 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "multi_face_direction_bits",
                value: multi_face_direction_bits as u32,
                min: 0,
                max: 63,
            });
        }
        Ok(Self {
            multi_face_direction_bits,
        })
    }

    /// Get the multi_face_direction_bits value.
    #[inline]
    pub fn multi_face_direction_bits(&self) -> u8 {
        self.multi_face_direction_bits
    }
}

impl BlockState for MultiFaceState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.multi_face_direction_bits as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 64 {
            return None;
        }
        let rem = offset;
        let multi_face_direction_bits = (rem % 64) as u8;
        Some(Self {
            multi_face_direction_bits,
        })
    }

    fn state_count() -> u32 {
        64
    }
}

/// State shared by: ["leaf_litter", "wildflowers", "pink_petals"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PetalsState {
    growth: u8,
    cardinal_direction: CardinalDirection,
}

impl PetalsState {
    /// Create a new state with validation.
    pub fn new(
        growth: u8,
        cardinal_direction: CardinalDirection,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if growth > 7 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "growth",
                value: growth as u32,
                min: 0,
                max: 7,
            });
        }
        Ok(Self {
            growth,
            cardinal_direction,
        })
    }

    /// Get the growth value.
    #[inline]
    pub fn growth(&self) -> u8 {
        self.growth
    }
    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
}

impl BlockState for PetalsState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.growth as u32) * multiplier;
        multiplier *= 8;
        offset += (self.cardinal_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 32 {
            return None;
        }
        let mut rem = offset;
        let growth = (rem % 8) as u8;
        rem /= 8;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        Some(Self {
            growth,
            cardinal_direction,
        })
    }

    fn state_count() -> u32 {
        32
    }
}

/// State shared by: ["cave_vines_body_with_berries", "cave_vines", "cave_vines_head_with_berries"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GrowingPlantState {
    growing_plant_age: u8,
}

impl GrowingPlantState {
    /// Create a new state with validation.
    pub fn new(growing_plant_age: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if growing_plant_age > 25 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "growing_plant_age",
                value: growing_plant_age as u32,
                min: 0,
                max: 25,
            });
        }
        Ok(Self { growing_plant_age })
    }

    /// Get the growing_plant_age value.
    #[inline]
    pub fn growing_plant_age(&self) -> u8 {
        self.growing_plant_age
    }
}

impl BlockState for GrowingPlantState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.growing_plant_age as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 26 {
            return None;
        }
        let rem = offset;
        let growing_plant_age = (rem % 26) as u8;
        Some(Self { growing_plant_age })
    }

    fn state_count() -> u32 {
        26
    }
}

/// State shared by: ["red_mushroom_block", "brown_mushroom_block", "mushroom_stem"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MushroomState {
    huge_mushroom_bits: u8,
}

impl MushroomState {
    /// Create a new state with validation.
    pub fn new(huge_mushroom_bits: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if huge_mushroom_bits > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "huge_mushroom_bits",
                value: huge_mushroom_bits as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self { huge_mushroom_bits })
    }

    /// Get the huge_mushroom_bits value.
    #[inline]
    pub fn huge_mushroom_bits(&self) -> u8 {
        self.huge_mushroom_bits
    }
}

impl BlockState for MushroomState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.huge_mushroom_bits as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let rem = offset;
        let huge_mushroom_bits = (rem % 16) as u8;
        Some(Self { huge_mushroom_bits })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["chain_command_block", "repeating_command_block", "command_block"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommandState {
    conditional_bit: bool,
    facing_direction: u8,
}

impl CommandState {
    /// Create a new state with validation.
    pub fn new(
        conditional_bit: bool,
        facing_direction: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            conditional_bit,
            facing_direction,
        })
    }

    /// Get the conditional_bit value.
    #[inline]
    pub fn conditional_bit(&self) -> bool {
        self.conditional_bit
    }
    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
}

impl BlockState for CommandState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.conditional_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.facing_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let conditional_bit = (rem % 2) != 0;
        rem /= 2;
        let facing_direction = (rem % 6) as u8;
        Some(Self {
            conditional_bit,
            facing_direction,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["powered_comparator", "unpowered_comparator"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComparatorState {
    cardinal_direction: CardinalDirection,
    output_lit_bit: bool,
    output_subtract_bit: bool,
}

impl ComparatorState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        output_lit_bit: bool,
        output_subtract_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            cardinal_direction,
            output_lit_bit,
            output_subtract_bit,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the output_lit_bit value.
    #[inline]
    pub fn output_lit_bit(&self) -> bool {
        self.output_lit_bit
    }
    /// Get the output_subtract_bit value.
    #[inline]
    pub fn output_subtract_bit(&self) -> bool {
        self.output_subtract_bit
    }
}

impl BlockState for ComparatorState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.output_lit_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.output_subtract_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let output_lit_bit = (rem % 2) != 0;
        rem /= 2;
        let output_subtract_bit = (rem % 2) != 0;
        Some(Self {
            cardinal_direction,
            output_lit_bit,
            output_subtract_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["melon_stem", "pumpkin_stem"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StemState {
    facing_direction: u8,
    growth: u8,
}

impl StemState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: u8,
        growth: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        if growth > 7 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "growth",
                value: growth as u32,
                min: 0,
                max: 7,
            });
        }
        Ok(Self {
            facing_direction,
            growth,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the growth value.
    #[inline]
    pub fn growth(&self) -> u8 {
        self.growth
    }
}

impl BlockState for StemState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.growth as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 48 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let growth = (rem % 8) as u8;
        Some(Self {
            facing_direction,
            growth,
        })
    }

    fn state_count() -> u32 {
        48
    }
}

/// State shared by: ["unpowered_repeater", "powered_repeater"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RepeaterState {
    cardinal_direction: CardinalDirection,
    repeater_delay: u8,
}

impl RepeaterState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        repeater_delay: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if repeater_delay > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "repeater_delay",
                value: repeater_delay as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            cardinal_direction,
            repeater_delay,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the repeater_delay value.
    #[inline]
    pub fn repeater_delay(&self) -> u8 {
        self.repeater_delay
    }
}

impl BlockState for RepeaterState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.repeater_delay as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let repeater_delay = (rem % 4) as u8;
        Some(Self {
            cardinal_direction,
            repeater_delay,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["frosted_ice", "nether_wart"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AgeBlockState {
    age: u8,
}

impl AgeBlockState {
    /// Create a new state with validation.
    pub fn new(age: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if age > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "age",
                value: age as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self { age })
    }

    /// Get the age value.
    #[inline]
    pub fn age(&self) -> u8 {
        self.age
    }
}

impl BlockState for AgeBlockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.age as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 4 {
            return None;
        }
        let rem = offset;
        let age = (rem % 4) as u8;
        Some(Self { age })
    }

    fn state_count() -> u32 {
        4
    }
}

/// State shared by: ["tnt", "underwater_tnt"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TntState {
    explode_bit: bool,
}

impl TntState {
    /// Create a new state with validation.
    pub fn new(explode_bit: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { explode_bit })
    }

    /// Get the explode_bit value.
    #[inline]
    pub fn explode_bit(&self) -> bool {
        self.explode_bit
    }
}

impl BlockState for TntState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.explode_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let explode_bit = (rem % 2) != 0;
        Some(Self { explode_bit })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["suspicious_gravel", "suspicious_sand"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BrushableState {
    brushed_progress: u8,
    hanging: bool,
}

impl BrushableState {
    /// Create a new state with validation.
    pub fn new(
        brushed_progress: u8,
        hanging: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if brushed_progress > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "brushed_progress",
                value: brushed_progress as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            brushed_progress,
            hanging,
        })
    }

    /// Get the brushed_progress value.
    #[inline]
    pub fn brushed_progress(&self) -> u8 {
        self.brushed_progress
    }
    /// Get the hanging value.
    #[inline]
    pub fn hanging(&self) -> bool {
        self.hanging
    }
}

impl BlockState for BrushableState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.brushed_progress as u32) * multiplier;
        multiplier *= 4;
        offset += (self.hanging as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let brushed_progress = (rem % 4) as u8;
        rem /= 4;
        let hanging = (rem % 2) != 0;
        Some(Self {
            brushed_progress,
            hanging,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["dispenser", "dropper"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DispenserState {
    facing_direction: u8,
    triggered_bit: bool,
}

impl DispenserState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: u8,
        triggered_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            facing_direction,
            triggered_bit,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the triggered_bit value.
    #[inline]
    pub fn triggered_bit(&self) -> bool {
        self.triggered_bit
    }
}

impl BlockState for DispenserState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.triggered_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let triggered_bit = (rem % 2) != 0;
        Some(Self {
            facing_direction,
            triggered_bit,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["soul_campfire", "campfire"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CampfireState {
    extinguished: bool,
    cardinal_direction: CardinalDirection,
}

impl CampfireState {
    /// Create a new state with validation.
    pub fn new(
        extinguished: bool,
        cardinal_direction: CardinalDirection,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            extinguished,
            cardinal_direction,
        })
    }

    /// Get the extinguished value.
    #[inline]
    pub fn extinguished(&self) -> bool {
        self.extinguished
    }
    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
}

impl BlockState for CampfireState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.extinguished as u32) * multiplier;
        multiplier *= 2;
        offset += (self.cardinal_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let extinguished = (rem % 2) != 0;
        rem /= 2;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        Some(Self {
            extinguished,
            cardinal_direction,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["glow_frame", "frame"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameState {
    facing_direction: u8,
    item_frame_map_bit: bool,
    item_frame_photo_bit: bool,
}

impl FrameState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: u8,
        item_frame_map_bit: bool,
        item_frame_photo_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            facing_direction,
            item_frame_map_bit,
            item_frame_photo_bit,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the item_frame_map_bit value.
    #[inline]
    pub fn item_frame_map_bit(&self) -> bool {
        self.item_frame_map_bit
    }
    /// Get the item_frame_photo_bit value.
    #[inline]
    pub fn item_frame_photo_bit(&self) -> bool {
        self.item_frame_photo_bit
    }
}

impl BlockState for FrameState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.item_frame_map_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.item_frame_photo_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 24 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let item_frame_map_bit = (rem % 2) != 0;
        rem /= 2;
        let item_frame_photo_bit = (rem % 2) != 0;
        Some(Self {
            facing_direction,
            item_frame_map_bit,
            item_frame_photo_bit,
        })
    }

    fn state_count() -> u32 {
        24
    }
}

/// State shared by: ["bone_block", "hay_block"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeprecatedPillarState {
    deprecated: u8,
    pillar_axis: PillarAxis,
}

impl DeprecatedPillarState {
    /// Create a new state with validation.
    pub fn new(
        deprecated: u8,
        pillar_axis: PillarAxis,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if deprecated > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "deprecated",
                value: deprecated as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            deprecated,
            pillar_axis,
        })
    }

    /// Get the deprecated value.
    #[inline]
    pub fn deprecated(&self) -> u8 {
        self.deprecated
    }
    /// Get the pillar_axis value.
    #[inline]
    pub fn pillar_axis(&self) -> PillarAxis {
        self.pillar_axis
    }
}

impl BlockState for DeprecatedPillarState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.deprecated as u32) * multiplier;
        multiplier *= 4;
        offset += (self.pillar_axis as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let deprecated = (rem % 4) as u8;
        rem /= 4;
        let pillar_axis = PillarAxis::from_raw((rem % 3) as u8)?;
        Some(Self {
            deprecated,
            pillar_axis,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["bee_nest", "beehive"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BeehiveState {
    direction: u8,
    honey_level: u8,
}

impl BeehiveState {
    /// Create a new state with validation.
    pub fn new(
        direction: u8,
        honey_level: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        if honey_level > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "honey_level",
                value: honey_level as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            direction,
            honey_level,
        })
    }

    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
    /// Get the honey_level value.
    #[inline]
    pub fn honey_level(&self) -> u8 {
        self.honey_level
    }
}

impl BlockState for BeehiveState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.honey_level as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 24 {
            return None;
        }
        let mut rem = offset;
        let direction = (rem % 4) as u8;
        rem /= 4;
        let honey_level = (rem % 6) as u8;
        Some(Self {
            direction,
            honey_level,
        })
    }

    fn state_count() -> u32 {
        24
    }
}

/// State shared by: ["bell"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BellState {
    attachment: Attachment,
    direction: u8,
    toggle_bit: bool,
}

impl BellState {
    /// Create a new state with validation.
    pub fn new(
        attachment: Attachment,
        direction: u8,
        toggle_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            attachment,
            direction,
            toggle_bit,
        })
    }

    /// Get the attachment value.
    #[inline]
    pub fn attachment(&self) -> Attachment {
        self.attachment
    }
    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
    /// Get the toggle_bit value.
    #[inline]
    pub fn toggle_bit(&self) -> bool {
        self.toggle_bit
    }
}

impl BlockState for BellState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.attachment as u32) * multiplier;
        multiplier *= 4;
        offset += (self.direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.toggle_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 32 {
            return None;
        }
        let mut rem = offset;
        let attachment = Attachment::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let direction = (rem % 4) as u8;
        rem /= 4;
        let toggle_bit = (rem % 2) != 0;
        Some(Self {
            attachment,
            direction,
            toggle_bit,
        })
    }

    fn state_count() -> u32 {
        32
    }
}

/// State shared by: ["rail"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RailBlockState {
    rail_direction: u8,
}

impl RailBlockState {
    /// Create a new state with validation.
    pub fn new(rail_direction: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if rail_direction > 9 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "rail_direction",
                value: rail_direction as u32,
                min: 0,
                max: 9,
            });
        }
        Ok(Self { rail_direction })
    }

    /// Get the rail_direction value.
    #[inline]
    pub fn rail_direction(&self) -> u8 {
        self.rail_direction
    }
}

impl BlockState for RailBlockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.rail_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 10 {
            return None;
        }
        let rem = offset;
        let rail_direction = (rem % 10) as u8;
        Some(Self { rail_direction })
    }

    fn state_count() -> u32 {
        10
    }
}

/// State shared by: ["cauldron"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CauldronState {
    cauldron_liquid: CauldronLiquid,
    fill_level: u8,
}

impl CauldronState {
    /// Create a new state with validation.
    pub fn new(
        cauldron_liquid: CauldronLiquid,
        fill_level: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if fill_level > 6 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "fill_level",
                value: fill_level as u32,
                min: 0,
                max: 6,
            });
        }
        Ok(Self {
            cauldron_liquid,
            fill_level,
        })
    }

    /// Get the cauldron_liquid value.
    #[inline]
    pub fn cauldron_liquid(&self) -> CauldronLiquid {
        self.cauldron_liquid
    }
    /// Get the fill_level value.
    #[inline]
    pub fn fill_level(&self) -> u8 {
        self.fill_level
    }
}

impl BlockState for CauldronState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cauldron_liquid as u32) * multiplier;
        multiplier *= 3;
        offset += (self.fill_level as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 21 {
            return None;
        }
        let mut rem = offset;
        let cauldron_liquid = CauldronLiquid::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let fill_level = (rem % 7) as u8;
        Some(Self {
            cauldron_liquid,
            fill_level,
        })
    }

    fn state_count() -> u32 {
        21
    }
}

/// State shared by: ["turtle_egg"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurtleEggState {
    cracked_state: CrackedState,
    turtle_egg_count: TurtleEggCount,
}

impl TurtleEggState {
    /// Create a new state with validation.
    pub fn new(
        cracked_state: CrackedState,
        turtle_egg_count: TurtleEggCount,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            cracked_state,
            turtle_egg_count,
        })
    }

    /// Get the cracked_state value.
    #[inline]
    pub fn cracked_state(&self) -> CrackedState {
        self.cracked_state
    }
    /// Get the turtle_egg_count value.
    #[inline]
    pub fn turtle_egg_count(&self) -> TurtleEggCount {
        self.turtle_egg_count
    }
}

impl BlockState for TurtleEggState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cracked_state as u32) * multiplier;
        multiplier *= 3;
        offset += (self.turtle_egg_count as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let cracked_state = CrackedState::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let turtle_egg_count = TurtleEggCount::from_raw((rem % 4) as u8)?;
        Some(Self {
            cracked_state,
            turtle_egg_count,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["pointed_dripstone"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PointedDripstoneState {
    dripstone_thickness: DripstoneThickness,
    hanging: bool,
}

impl PointedDripstoneState {
    /// Create a new state with validation.
    pub fn new(
        dripstone_thickness: DripstoneThickness,
        hanging: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            dripstone_thickness,
            hanging,
        })
    }

    /// Get the dripstone_thickness value.
    #[inline]
    pub fn dripstone_thickness(&self) -> DripstoneThickness {
        self.dripstone_thickness
    }
    /// Get the hanging value.
    #[inline]
    pub fn hanging(&self) -> bool {
        self.hanging
    }
}

impl BlockState for PointedDripstoneState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.dripstone_thickness as u32) * multiplier;
        multiplier *= 5;
        offset += (self.hanging as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 10 {
            return None;
        }
        let mut rem = offset;
        let dripstone_thickness = DripstoneThickness::from_raw((rem % 5) as u8)?;
        rem /= 5;
        let hanging = (rem % 2) != 0;
        Some(Self {
            dripstone_thickness,
            hanging,
        })
    }

    fn state_count() -> u32 {
        10
    }
}

/// State shared by: ["seagrass"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeagrassState {
    sea_grass_type: SeaGrassType,
}

impl SeagrassState {
    /// Create a new state with validation.
    pub fn new(
        sea_grass_type: SeaGrassType,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { sea_grass_type })
    }

    /// Get the sea_grass_type value.
    #[inline]
    pub fn sea_grass_type(&self) -> SeaGrassType {
        self.sea_grass_type
    }
}

impl BlockState for SeagrassState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.sea_grass_type as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 3 {
            return None;
        }
        let rem = offset;
        let sea_grass_type = SeaGrassType::from_raw((rem % 3) as u8)?;
        Some(Self { sea_grass_type })
    }

    fn state_count() -> u32 {
        3
    }
}

/// State shared by: ["bubble_column"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BubbleColumnState {
    drag_down: bool,
}

impl BubbleColumnState {
    /// Create a new state with validation.
    pub fn new(drag_down: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { drag_down })
    }

    /// Get the drag_down value.
    #[inline]
    pub fn drag_down(&self) -> bool {
        self.drag_down
    }
}

impl BlockState for BubbleColumnState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.drag_down as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let drag_down = (rem % 2) != 0;
        Some(Self { drag_down })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["weeping_vines"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WeepingVinesState {
    weeping_vines_age: u8,
}

impl WeepingVinesState {
    /// Create a new state with validation.
    pub fn new(weeping_vines_age: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if weeping_vines_age > 25 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "weeping_vines_age",
                value: weeping_vines_age as u32,
                min: 0,
                max: 25,
            });
        }
        Ok(Self { weeping_vines_age })
    }

    /// Get the weeping_vines_age value.
    #[inline]
    pub fn weeping_vines_age(&self) -> u8 {
        self.weeping_vines_age
    }
}

impl BlockState for WeepingVinesState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.weeping_vines_age as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 26 {
            return None;
        }
        let rem = offset;
        let weeping_vines_age = (rem % 26) as u8;
        Some(Self { weeping_vines_age })
    }

    fn state_count() -> u32 {
        26
    }
}

/// State shared by: ["trip_wire"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TripWireState {
    attached_bit: bool,
    disarmed_bit: bool,
    powered_bit: bool,
    suspended_bit: bool,
}

impl TripWireState {
    /// Create a new state with validation.
    pub fn new(
        attached_bit: bool,
        disarmed_bit: bool,
        powered_bit: bool,
        suspended_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            attached_bit,
            disarmed_bit,
            powered_bit,
            suspended_bit,
        })
    }

    /// Get the attached_bit value.
    #[inline]
    pub fn attached_bit(&self) -> bool {
        self.attached_bit
    }
    /// Get the disarmed_bit value.
    #[inline]
    pub fn disarmed_bit(&self) -> bool {
        self.disarmed_bit
    }
    /// Get the powered_bit value.
    #[inline]
    pub fn powered_bit(&self) -> bool {
        self.powered_bit
    }
    /// Get the suspended_bit value.
    #[inline]
    pub fn suspended_bit(&self) -> bool {
        self.suspended_bit
    }
}

impl BlockState for TripWireState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.attached_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.disarmed_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.powered_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.suspended_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let attached_bit = (rem % 2) != 0;
        rem /= 2;
        let disarmed_bit = (rem % 2) != 0;
        rem /= 2;
        let powered_bit = (rem % 2) != 0;
        rem /= 2;
        let suspended_bit = (rem % 2) != 0;
        Some(Self {
            attached_bit,
            disarmed_bit,
            powered_bit,
            suspended_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["sea_pickle"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeaPickleState {
    cluster_count: u8,
    dead_bit: bool,
}

impl SeaPickleState {
    /// Create a new state with validation.
    pub fn new(
        cluster_count: u8,
        dead_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if cluster_count > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "cluster_count",
                value: cluster_count as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            cluster_count,
            dead_bit,
        })
    }

    /// Get the cluster_count value.
    #[inline]
    pub fn cluster_count(&self) -> u8 {
        self.cluster_count
    }
    /// Get the dead_bit value.
    #[inline]
    pub fn dead_bit(&self) -> bool {
        self.dead_bit
    }
}

impl BlockState for SeaPickleState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cluster_count as u32) * multiplier;
        multiplier *= 4;
        offset += (self.dead_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let cluster_count = (rem % 4) as u8;
        rem /= 4;
        let dead_bit = (rem % 2) != 0;
        Some(Self {
            cluster_count,
            dead_bit,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["portal"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PortalState {
    portal_axis: PortalAxis,
}

impl PortalState {
    /// Create a new state with validation.
    pub fn new(portal_axis: PortalAxis) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { portal_axis })
    }

    /// Get the portal_axis value.
    #[inline]
    pub fn portal_axis(&self) -> PortalAxis {
        self.portal_axis
    }
}

impl BlockState for PortalState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.portal_axis as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 3 {
            return None;
        }
        let rem = offset;
        let portal_axis = PortalAxis::from_raw((rem % 3) as u8)?;
        Some(Self { portal_axis })
    }

    fn state_count() -> u32 {
        3
    }
}

/// State shared by: ["sculk_sensor"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SculkSensorState {
    sculk_sensor_phase: u8,
}

impl SculkSensorState {
    /// Create a new state with validation.
    pub fn new(sculk_sensor_phase: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if sculk_sensor_phase > 2 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "sculk_sensor_phase",
                value: sculk_sensor_phase as u32,
                min: 0,
                max: 2,
            });
        }
        Ok(Self { sculk_sensor_phase })
    }

    /// Get the sculk_sensor_phase value.
    #[inline]
    pub fn sculk_sensor_phase(&self) -> u8 {
        self.sculk_sensor_phase
    }
}

impl BlockState for SculkSensorState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.sculk_sensor_phase as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 3 {
            return None;
        }
        let rem = offset;
        let sculk_sensor_phase = (rem % 3) as u8;
        Some(Self { sculk_sensor_phase })
    }

    fn state_count() -> u32 {
        3
    }
}

/// State shared by: ["kelp"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KelpState {
    kelp_age: u8,
}

impl KelpState {
    /// Create a new state with validation.
    pub fn new(kelp_age: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if kelp_age > 25 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "kelp_age",
                value: kelp_age as u32,
                min: 0,
                max: 25,
            });
        }
        Ok(Self { kelp_age })
    }

    /// Get the kelp_age value.
    #[inline]
    pub fn kelp_age(&self) -> u8 {
        self.kelp_age
    }
}

impl BlockState for KelpState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.kelp_age as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 26 {
            return None;
        }
        let rem = offset;
        let kelp_age = (rem % 26) as u8;
        Some(Self { kelp_age })
    }

    fn state_count() -> u32 {
        26
    }
}

/// State shared by: ["respawn_anchor"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RespawnAnchorState {
    respawn_anchor_charge: u8,
}

impl RespawnAnchorState {
    /// Create a new state with validation.
    pub fn new(
        respawn_anchor_charge: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if respawn_anchor_charge > 4 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "respawn_anchor_charge",
                value: respawn_anchor_charge as u32,
                min: 0,
                max: 4,
            });
        }
        Ok(Self {
            respawn_anchor_charge,
        })
    }

    /// Get the respawn_anchor_charge value.
    #[inline]
    pub fn respawn_anchor_charge(&self) -> u8 {
        self.respawn_anchor_charge
    }
}

impl BlockState for RespawnAnchorState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.respawn_anchor_charge as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 5 {
            return None;
        }
        let rem = offset;
        let respawn_anchor_charge = (rem % 5) as u8;
        Some(Self {
            respawn_anchor_charge,
        })
    }

    fn state_count() -> u32 {
        5
    }
}

/// State shared by: ["cocoa"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CocoaState {
    age: u8,
    direction: u8,
}

impl CocoaState {
    /// Create a new state with validation.
    pub fn new(age: u8, direction: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if age > 2 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "age",
                value: age as u32,
                min: 0,
                max: 2,
            });
        }
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self { age, direction })
    }

    /// Get the age value.
    #[inline]
    pub fn age(&self) -> u8 {
        self.age
    }
    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
}

impl BlockState for CocoaState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.age as u32) * multiplier;
        multiplier *= 3;
        offset += (self.direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let age = (rem % 3) as u8;
        rem /= 3;
        let direction = (rem % 4) as u8;
        Some(Self { age, direction })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["sculk_shrieker"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SculkShriekerState {
    active: bool,
    can_summon: bool,
}

impl SculkShriekerState {
    /// Create a new state with validation.
    pub fn new(
        active: bool,
        can_summon: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { active, can_summon })
    }

    /// Get the active value.
    #[inline]
    pub fn active(&self) -> bool {
        self.active
    }
    /// Get the can_summon value.
    #[inline]
    pub fn can_summon(&self) -> bool {
        self.can_summon
    }
}

impl BlockState for SculkShriekerState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.active as u32) * multiplier;
        multiplier *= 2;
        offset += (self.can_summon as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 4 {
            return None;
        }
        let mut rem = offset;
        let active = (rem % 2) != 0;
        rem /= 2;
        let can_summon = (rem % 2) != 0;
        Some(Self { active, can_summon })
    }

    fn state_count() -> u32 {
        4
    }
}

/// State shared by: ["barrel"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BarrelState {
    facing_direction: u8,
    open_bit: bool,
}

impl BarrelState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: u8,
        open_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            facing_direction,
            open_bit,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the open_bit value.
    #[inline]
    pub fn open_bit(&self) -> bool {
        self.open_bit
    }
}

impl BlockState for BarrelState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.open_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let open_bit = (rem % 2) != 0;
        Some(Self {
            facing_direction,
            open_bit,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["end_portal_frame"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EndPortalFrameState {
    end_portal_eye_bit: bool,
    cardinal_direction: CardinalDirection,
}

impl EndPortalFrameState {
    /// Create a new state with validation.
    pub fn new(
        end_portal_eye_bit: bool,
        cardinal_direction: CardinalDirection,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            end_portal_eye_bit,
            cardinal_direction,
        })
    }

    /// Get the end_portal_eye_bit value.
    #[inline]
    pub fn end_portal_eye_bit(&self) -> bool {
        self.end_portal_eye_bit
    }
    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
}

impl BlockState for EndPortalFrameState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.end_portal_eye_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.cardinal_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let end_portal_eye_bit = (rem % 2) != 0;
        rem /= 2;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        Some(Self {
            end_portal_eye_bit,
            cardinal_direction,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["bedrock"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BedrockState {
    infiniburn_bit: bool,
}

impl BedrockState {
    /// Create a new state with validation.
    pub fn new(infiniburn_bit: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { infiniburn_bit })
    }

    /// Get the infiniburn_bit value.
    #[inline]
    pub fn infiniburn_bit(&self) -> bool {
        self.infiniburn_bit
    }
}

impl BlockState for BedrockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.infiniburn_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let infiniburn_bit = (rem % 2) != 0;
        Some(Self { infiniburn_bit })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["sculk_catalyst"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SculkCatalystState {
    bloom: bool,
}

impl SculkCatalystState {
    /// Create a new state with validation.
    pub fn new(bloom: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { bloom })
    }

    /// Get the bloom value.
    #[inline]
    pub fn bloom(&self) -> bool {
        self.bloom
    }
}

impl BlockState for SculkCatalystState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.bloom as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let bloom = (rem % 2) != 0;
        Some(Self { bloom })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["hopper"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct HopperState {
    facing_direction: u8,
    toggle_bit: bool,
}

impl HopperState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: u8,
        toggle_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            facing_direction,
            toggle_bit,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the toggle_bit value.
    #[inline]
    pub fn toggle_bit(&self) -> bool {
        self.toggle_bit
    }
}

impl BlockState for HopperState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.toggle_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let toggle_bit = (rem % 2) != 0;
        Some(Self {
            facing_direction,
            toggle_bit,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["small_dripleaf_block"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SmallDripleafBlockState {
    cardinal_direction: CardinalDirection,
    upper_block_bit: bool,
}

impl SmallDripleafBlockState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        upper_block_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            cardinal_direction,
            upper_block_bit,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the upper_block_bit value.
    #[inline]
    pub fn upper_block_bit(&self) -> bool {
        self.upper_block_bit
    }
}

impl BlockState for SmallDripleafBlockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.upper_block_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let upper_block_bit = (rem % 2) != 0;
        Some(Self {
            cardinal_direction,
            upper_block_bit,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["pale_moss_carpet"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaleMossCarpetState {
    pale_moss_carpet_side_east: PaleMossCarpetSideEast,
    pale_moss_carpet_side_north: PaleMossCarpetSideNorth,
    pale_moss_carpet_side_south: PaleMossCarpetSideSouth,
    pale_moss_carpet_side_west: PaleMossCarpetSideWest,
    upper_block_bit: bool,
}

impl PaleMossCarpetState {
    /// Create a new state with validation.
    pub fn new(
        pale_moss_carpet_side_east: PaleMossCarpetSideEast,
        pale_moss_carpet_side_north: PaleMossCarpetSideNorth,
        pale_moss_carpet_side_south: PaleMossCarpetSideSouth,
        pale_moss_carpet_side_west: PaleMossCarpetSideWest,
        upper_block_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            pale_moss_carpet_side_east,
            pale_moss_carpet_side_north,
            pale_moss_carpet_side_south,
            pale_moss_carpet_side_west,
            upper_block_bit,
        })
    }

    /// Get the pale_moss_carpet_side_east value.
    #[inline]
    pub fn pale_moss_carpet_side_east(&self) -> PaleMossCarpetSideEast {
        self.pale_moss_carpet_side_east
    }
    /// Get the pale_moss_carpet_side_north value.
    #[inline]
    pub fn pale_moss_carpet_side_north(&self) -> PaleMossCarpetSideNorth {
        self.pale_moss_carpet_side_north
    }
    /// Get the pale_moss_carpet_side_south value.
    #[inline]
    pub fn pale_moss_carpet_side_south(&self) -> PaleMossCarpetSideSouth {
        self.pale_moss_carpet_side_south
    }
    /// Get the pale_moss_carpet_side_west value.
    #[inline]
    pub fn pale_moss_carpet_side_west(&self) -> PaleMossCarpetSideWest {
        self.pale_moss_carpet_side_west
    }
    /// Get the upper_block_bit value.
    #[inline]
    pub fn upper_block_bit(&self) -> bool {
        self.upper_block_bit
    }
}

impl BlockState for PaleMossCarpetState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.pale_moss_carpet_side_east as u32) * multiplier;
        multiplier *= 3;
        offset += (self.pale_moss_carpet_side_north as u32) * multiplier;
        multiplier *= 3;
        offset += (self.pale_moss_carpet_side_south as u32) * multiplier;
        multiplier *= 3;
        offset += (self.pale_moss_carpet_side_west as u32) * multiplier;
        multiplier *= 3;
        offset += (self.upper_block_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 162 {
            return None;
        }
        let mut rem = offset;
        let pale_moss_carpet_side_east = PaleMossCarpetSideEast::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let pale_moss_carpet_side_north = PaleMossCarpetSideNorth::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let pale_moss_carpet_side_south = PaleMossCarpetSideSouth::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let pale_moss_carpet_side_west = PaleMossCarpetSideWest::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let upper_block_bit = (rem % 2) != 0;
        Some(Self {
            pale_moss_carpet_side_east,
            pale_moss_carpet_side_north,
            pale_moss_carpet_side_south,
            pale_moss_carpet_side_west,
            upper_block_bit,
        })
    }

    fn state_count() -> u32 {
        162
    }
}

/// State shared by: ["chalkboard"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DirectionBlockState {
    direction: u8,
}

impl DirectionBlockState {
    /// Create a new state with validation.
    pub fn new(direction: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self { direction })
    }

    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
}

impl BlockState for DirectionBlockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let rem = offset;
        let direction = (rem % 16) as u8;
        Some(Self { direction })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["sniffer_egg"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnifferEggState {
    cracked_state: CrackedState,
}

impl SnifferEggState {
    /// Create a new state with validation.
    pub fn new(
        cracked_state: CrackedState,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { cracked_state })
    }

    /// Get the cracked_state value.
    #[inline]
    pub fn cracked_state(&self) -> CrackedState {
        self.cracked_state
    }
}

impl BlockState for SnifferEggState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.cracked_state as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 3 {
            return None;
        }
        let rem = offset;
        let cracked_state = CrackedState::from_raw((rem % 3) as u8)?;
        Some(Self { cracked_state })
    }

    fn state_count() -> u32 {
        3
    }
}

/// State shared by: ["cake"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CakeState {
    bite_counter: u8,
}

impl CakeState {
    /// Create a new state with validation.
    pub fn new(bite_counter: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if bite_counter > 6 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "bite_counter",
                value: bite_counter as u32,
                min: 0,
                max: 6,
            });
        }
        Ok(Self { bite_counter })
    }

    /// Get the bite_counter value.
    #[inline]
    pub fn bite_counter(&self) -> u8 {
        self.bite_counter
    }
}

impl BlockState for CakeState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.bite_counter as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 7 {
            return None;
        }
        let rem = offset;
        let bite_counter = (rem % 7) as u8;
        Some(Self { bite_counter })
    }

    fn state_count() -> u32 {
        7
    }
}

/// State shared by: ["mangrove_propagule"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MangrovePropaguleState {
    hanging: bool,
    propagule_stage: u8,
}

impl MangrovePropaguleState {
    /// Create a new state with validation.
    pub fn new(
        hanging: bool,
        propagule_stage: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if propagule_stage > 4 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "propagule_stage",
                value: propagule_stage as u32,
                min: 0,
                max: 4,
            });
        }
        Ok(Self {
            hanging,
            propagule_stage,
        })
    }

    /// Get the hanging value.
    #[inline]
    pub fn hanging(&self) -> bool {
        self.hanging
    }
    /// Get the propagule_stage value.
    #[inline]
    pub fn propagule_stage(&self) -> u8 {
        self.propagule_stage
    }
}

impl BlockState for MangrovePropaguleState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.hanging as u32) * multiplier;
        multiplier *= 2;
        offset += (self.propagule_stage as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 10 {
            return None;
        }
        let mut rem = offset;
        let hanging = (rem % 2) != 0;
        rem /= 2;
        let propagule_stage = (rem % 5) as u8;
        Some(Self {
            hanging,
            propagule_stage,
        })
    }

    fn state_count() -> u32 {
        10
    }
}

/// State shared by: ["flower_pot"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlowerPotState {
    update_bit: bool,
}

impl FlowerPotState {
    /// Create a new state with validation.
    pub fn new(update_bit: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { update_bit })
    }

    /// Get the update_bit value.
    #[inline]
    pub fn update_bit(&self) -> bool {
        self.update_bit
    }
}

impl BlockState for FlowerPotState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.update_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let update_bit = (rem % 2) != 0;
        Some(Self { update_bit })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["vault"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VaultBlockState {
    cardinal_direction: CardinalDirection,
    ominous: bool,
    vault_state: VaultState,
}

impl VaultBlockState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        ominous: bool,
        vault_state: VaultState,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            cardinal_direction,
            ominous,
            vault_state,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the ominous value.
    #[inline]
    pub fn ominous(&self) -> bool {
        self.ominous
    }
    /// Get the vault_state value.
    #[inline]
    pub fn vault_state(&self) -> VaultState {
        self.vault_state
    }
}

impl BlockState for VaultBlockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.ominous as u32) * multiplier;
        multiplier *= 2;
        offset += (self.vault_state as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 32 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let ominous = (rem % 2) != 0;
        rem /= 2;
        let vault_state = VaultState::from_raw((rem % 4) as u8)?;
        Some(Self {
            cardinal_direction,
            ominous,
            vault_state,
        })
    }

    fn state_count() -> u32 {
        32
    }
}

/// State shared by: ["composter"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ComposterState {
    composter_fill_level: u8,
}

impl ComposterState {
    /// Create a new state with validation.
    pub fn new(
        composter_fill_level: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if composter_fill_level > 8 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "composter_fill_level",
                value: composter_fill_level as u32,
                min: 0,
                max: 8,
            });
        }
        Ok(Self {
            composter_fill_level,
        })
    }

    /// Get the composter_fill_level value.
    #[inline]
    pub fn composter_fill_level(&self) -> u8 {
        self.composter_fill_level
    }
}

impl BlockState for ComposterState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.composter_fill_level as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 9 {
            return None;
        }
        let rem = offset;
        let composter_fill_level = (rem % 9) as u8;
        Some(Self {
            composter_fill_level,
        })
    }

    fn state_count() -> u32 {
        9
    }
}

/// State shared by: ["bamboo"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BambooState {
    age_bit: bool,
    bamboo_leaf_size: BambooLeafSize,
    bamboo_stalk_thickness: BambooStalkThickness,
}

impl BambooState {
    /// Create a new state with validation.
    pub fn new(
        age_bit: bool,
        bamboo_leaf_size: BambooLeafSize,
        bamboo_stalk_thickness: BambooStalkThickness,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            age_bit,
            bamboo_leaf_size,
            bamboo_stalk_thickness,
        })
    }

    /// Get the age_bit value.
    #[inline]
    pub fn age_bit(&self) -> bool {
        self.age_bit
    }
    /// Get the bamboo_leaf_size value.
    #[inline]
    pub fn bamboo_leaf_size(&self) -> BambooLeafSize {
        self.bamboo_leaf_size
    }
    /// Get the bamboo_stalk_thickness value.
    #[inline]
    pub fn bamboo_stalk_thickness(&self) -> BambooStalkThickness {
        self.bamboo_stalk_thickness
    }
}

impl BlockState for BambooState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.age_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.bamboo_leaf_size as u32) * multiplier;
        multiplier *= 3;
        offset += (self.bamboo_stalk_thickness as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let age_bit = (rem % 2) != 0;
        rem /= 2;
        let bamboo_leaf_size = BambooLeafSize::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let bamboo_stalk_thickness = BambooStalkThickness::from_raw((rem % 2) as u8)?;
        Some(Self {
            age_bit,
            bamboo_leaf_size,
            bamboo_stalk_thickness,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["pitcher_crop"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PitcherCropState {
    growth: u8,
    upper_block_bit: bool,
}

impl PitcherCropState {
    /// Create a new state with validation.
    pub fn new(
        growth: u8,
        upper_block_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if growth > 7 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "growth",
                value: growth as u32,
                min: 0,
                max: 7,
            });
        }
        Ok(Self {
            growth,
            upper_block_bit,
        })
    }

    /// Get the growth value.
    #[inline]
    pub fn growth(&self) -> u8 {
        self.growth
    }
    /// Get the upper_block_bit value.
    #[inline]
    pub fn upper_block_bit(&self) -> bool {
        self.upper_block_bit
    }
}

impl BlockState for PitcherCropState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.growth as u32) * multiplier;
        multiplier *= 8;
        offset += (self.upper_block_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let growth = (rem % 8) as u8;
        rem /= 8;
        let upper_block_bit = (rem % 2) != 0;
        Some(Self {
            growth,
            upper_block_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["vine"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VineState {
    vine_direction_bits: u8,
}

impl VineState {
    /// Create a new state with validation.
    pub fn new(vine_direction_bits: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if vine_direction_bits > 15 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "vine_direction_bits",
                value: vine_direction_bits as u32,
                min: 0,
                max: 15,
            });
        }
        Ok(Self {
            vine_direction_bits,
        })
    }

    /// Get the vine_direction_bits value.
    #[inline]
    pub fn vine_direction_bits(&self) -> u8 {
        self.vine_direction_bits
    }
}

impl BlockState for VineState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.vine_direction_bits as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let rem = offset;
        let vine_direction_bits = (rem % 16) as u8;
        Some(Self {
            vine_direction_bits,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["trial_spawner"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TrialSpawnerState {
    ominous: bool,
    trial_spawner_state: u8,
}

impl TrialSpawnerState {
    /// Create a new state with validation.
    pub fn new(
        ominous: bool,
        trial_spawner_state: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if trial_spawner_state > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "trial_spawner_state",
                value: trial_spawner_state as u32,
                min: 0,
                max: 5,
            });
        }
        Ok(Self {
            ominous,
            trial_spawner_state,
        })
    }

    /// Get the ominous value.
    #[inline]
    pub fn ominous(&self) -> bool {
        self.ominous
    }
    /// Get the trial_spawner_state value.
    #[inline]
    pub fn trial_spawner_state(&self) -> u8 {
        self.trial_spawner_state
    }
}

impl BlockState for TrialSpawnerState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.ominous as u32) * multiplier;
        multiplier *= 2;
        offset += (self.trial_spawner_state as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let ominous = (rem % 2) != 0;
        rem /= 2;
        let trial_spawner_state = (rem % 6) as u8;
        Some(Self {
            ominous,
            trial_spawner_state,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["lectern"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LecternState {
    cardinal_direction: CardinalDirection,
    powered_bit: bool,
}

impl LecternState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        powered_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            cardinal_direction,
            powered_bit,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the powered_bit value.
    #[inline]
    pub fn powered_bit(&self) -> bool {
        self.powered_bit
    }
}

impl BlockState for LecternState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.powered_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let powered_bit = (rem % 2) != 0;
        Some(Self {
            cardinal_direction,
            powered_bit,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["twisting_vines"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TwistingVinesState {
    twisting_vines_age: u8,
}

impl TwistingVinesState {
    /// Create a new state with validation.
    pub fn new(twisting_vines_age: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if twisting_vines_age > 25 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "twisting_vines_age",
                value: twisting_vines_age as u32,
                min: 0,
                max: 25,
            });
        }
        Ok(Self { twisting_vines_age })
    }

    /// Get the twisting_vines_age value.
    #[inline]
    pub fn twisting_vines_age(&self) -> u8 {
        self.twisting_vines_age
    }
}

impl BlockState for TwistingVinesState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.twisting_vines_age as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 26 {
            return None;
        }
        let rem = offset;
        let twisting_vines_age = (rem % 26) as u8;
        Some(Self { twisting_vines_age })
    }

    fn state_count() -> u32 {
        26
    }
}

/// State shared by: ["creaking_heart"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CreakingHeartBlockState {
    creaking_heart_state: CreakingHeartState,
    natural: bool,
    pillar_axis: PillarAxis,
}

impl CreakingHeartBlockState {
    /// Create a new state with validation.
    pub fn new(
        creaking_heart_state: CreakingHeartState,
        natural: bool,
        pillar_axis: PillarAxis,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            creaking_heart_state,
            natural,
            pillar_axis,
        })
    }

    /// Get the creaking_heart_state value.
    #[inline]
    pub fn creaking_heart_state(&self) -> CreakingHeartState {
        self.creaking_heart_state
    }
    /// Get the natural value.
    #[inline]
    pub fn natural(&self) -> bool {
        self.natural
    }
    /// Get the pillar_axis value.
    #[inline]
    pub fn pillar_axis(&self) -> PillarAxis {
        self.pillar_axis
    }
}

impl BlockState for CreakingHeartBlockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.creaking_heart_state as u32) * multiplier;
        multiplier *= 3;
        offset += (self.natural as u32) * multiplier;
        multiplier *= 2;
        offset += (self.pillar_axis as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 18 {
            return None;
        }
        let mut rem = offset;
        let creaking_heart_state = CreakingHeartState::from_raw((rem % 3) as u8)?;
        rem /= 3;
        let natural = (rem % 2) != 0;
        rem /= 2;
        let pillar_axis = PillarAxis::from_raw((rem % 3) as u8)?;
        Some(Self {
            creaking_heart_state,
            natural,
            pillar_axis,
        })
    }

    fn state_count() -> u32 {
        18
    }
}

/// State shared by: ["observer"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObserverState {
    facing_direction: FacingDirection,
    powered_bit: bool,
}

impl ObserverState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: FacingDirection,
        powered_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            facing_direction,
            powered_bit,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> FacingDirection {
        self.facing_direction
    }
    /// Get the powered_bit value.
    #[inline]
    pub fn powered_bit(&self) -> bool {
        self.powered_bit
    }
}

impl BlockState for ObserverState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.powered_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = FacingDirection::from_raw((rem % 6) as u8)?;
        rem /= 6;
        let powered_bit = (rem % 2) != 0;
        Some(Self {
            facing_direction,
            powered_bit,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["scaffolding"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ScaffoldingState {
    stability: u8,
    stability_check: bool,
}

impl ScaffoldingState {
    /// Create a new state with validation.
    pub fn new(
        stability: u8,
        stability_check: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if stability > 7 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "stability",
                value: stability as u32,
                min: 0,
                max: 7,
            });
        }
        Ok(Self {
            stability,
            stability_check,
        })
    }

    /// Get the stability value.
    #[inline]
    pub fn stability(&self) -> u8 {
        self.stability
    }
    /// Get the stability_check value.
    #[inline]
    pub fn stability_check(&self) -> bool {
        self.stability_check
    }
}

impl BlockState for ScaffoldingState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.stability as u32) * multiplier;
        multiplier *= 8;
        offset += (self.stability_check as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let stability = (rem % 8) as u8;
        rem /= 8;
        let stability_check = (rem % 2) != 0;
        Some(Self {
            stability,
            stability_check,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["brewing_stand"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BrewingStandState {
    brewing_stand_slot_a_bit: bool,
    brewing_stand_slot_b_bit: bool,
    brewing_stand_slot_c_bit: bool,
}

impl BrewingStandState {
    /// Create a new state with validation.
    pub fn new(
        brewing_stand_slot_a_bit: bool,
        brewing_stand_slot_b_bit: bool,
        brewing_stand_slot_c_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            brewing_stand_slot_a_bit,
            brewing_stand_slot_b_bit,
            brewing_stand_slot_c_bit,
        })
    }

    /// Get the brewing_stand_slot_a_bit value.
    #[inline]
    pub fn brewing_stand_slot_a_bit(&self) -> bool {
        self.brewing_stand_slot_a_bit
    }
    /// Get the brewing_stand_slot_b_bit value.
    #[inline]
    pub fn brewing_stand_slot_b_bit(&self) -> bool {
        self.brewing_stand_slot_b_bit
    }
    /// Get the brewing_stand_slot_c_bit value.
    #[inline]
    pub fn brewing_stand_slot_c_bit(&self) -> bool {
        self.brewing_stand_slot_c_bit
    }
}

impl BlockState for BrewingStandState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.brewing_stand_slot_a_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.brewing_stand_slot_b_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.brewing_stand_slot_c_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let mut rem = offset;
        let brewing_stand_slot_a_bit = (rem % 2) != 0;
        rem /= 2;
        let brewing_stand_slot_b_bit = (rem % 2) != 0;
        rem /= 2;
        let brewing_stand_slot_c_bit = (rem % 2) != 0;
        Some(Self {
            brewing_stand_slot_a_bit,
            brewing_stand_slot_b_bit,
            brewing_stand_slot_c_bit,
        })
    }

    fn state_count() -> u32 {
        8
    }
}

/// State shared by: ["crafter"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CrafterState {
    crafting: bool,
    orientation: Orientation,
    triggered_bit: bool,
}

impl CrafterState {
    /// Create a new state with validation.
    pub fn new(
        crafting: bool,
        orientation: Orientation,
        triggered_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            crafting,
            orientation,
            triggered_bit,
        })
    }

    /// Get the crafting value.
    #[inline]
    pub fn crafting(&self) -> bool {
        self.crafting
    }
    /// Get the orientation value.
    #[inline]
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }
    /// Get the triggered_bit value.
    #[inline]
    pub fn triggered_bit(&self) -> bool {
        self.triggered_bit
    }
}

impl BlockState for CrafterState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.crafting as u32) * multiplier;
        multiplier *= 2;
        offset += (self.orientation as u32) * multiplier;
        multiplier *= 12;
        offset += (self.triggered_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 48 {
            return None;
        }
        let mut rem = offset;
        let crafting = (rem % 2) != 0;
        rem /= 2;
        let orientation = Orientation::from_raw((rem % 12) as u8)?;
        rem /= 12;
        let triggered_bit = (rem % 2) != 0;
        Some(Self {
            crafting,
            orientation,
            triggered_bit,
        })
    }

    fn state_count() -> u32 {
        48
    }
}

/// State shared by: ["pale_hanging_moss"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PaleHangingMossState {
    tip: bool,
}

impl PaleHangingMossState {
    /// Create a new state with validation.
    pub fn new(tip: bool) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self { tip })
    }

    /// Get the tip value.
    #[inline]
    pub fn tip(&self) -> bool {
        self.tip
    }
}

impl BlockState for PaleHangingMossState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.tip as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 2 {
            return None;
        }
        let rem = offset;
        let tip = (rem % 2) != 0;
        Some(Self { tip })
    }

    fn state_count() -> u32 {
        2
    }
}

/// State shared by: ["dried_ghast"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DriedGhastState {
    cardinal_direction: CardinalDirection,
    rehydration_level: u8,
}

impl DriedGhastState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        rehydration_level: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if rehydration_level > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "rehydration_level",
                value: rehydration_level as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            cardinal_direction,
            rehydration_level,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the rehydration_level value.
    #[inline]
    pub fn rehydration_level(&self) -> u8 {
        self.rehydration_level
    }
}

impl BlockState for DriedGhastState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.rehydration_level as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let rehydration_level = (rem % 4) as u8;
        Some(Self {
            cardinal_direction,
            rehydration_level,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["big_dripleaf"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BigDripleafState {
    big_dripleaf_head: bool,
    big_dripleaf_tilt: BigDripleafTilt,
    cardinal_direction: CardinalDirection,
}

impl BigDripleafState {
    /// Create a new state with validation.
    pub fn new(
        big_dripleaf_head: bool,
        big_dripleaf_tilt: BigDripleafTilt,
        cardinal_direction: CardinalDirection,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            big_dripleaf_head,
            big_dripleaf_tilt,
            cardinal_direction,
        })
    }

    /// Get the big_dripleaf_head value.
    #[inline]
    pub fn big_dripleaf_head(&self) -> bool {
        self.big_dripleaf_head
    }
    /// Get the big_dripleaf_tilt value.
    #[inline]
    pub fn big_dripleaf_tilt(&self) -> BigDripleafTilt {
        self.big_dripleaf_tilt
    }
    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
}

impl BlockState for BigDripleafState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.big_dripleaf_head as u32) * multiplier;
        multiplier *= 2;
        offset += (self.big_dripleaf_tilt as u32) * multiplier;
        multiplier *= 4;
        offset += (self.cardinal_direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 32 {
            return None;
        }
        let mut rem = offset;
        let big_dripleaf_head = (rem % 2) != 0;
        rem /= 2;
        let big_dripleaf_tilt = BigDripleafTilt::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        Some(Self {
            big_dripleaf_head,
            big_dripleaf_tilt,
            cardinal_direction,
        })
    }

    fn state_count() -> u32 {
        32
    }
}

/// State shared by: ["bed"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BedState {
    direction: u8,
    head_piece_bit: bool,
    occupied_bit: bool,
}

impl BedState {
    /// Create a new state with validation.
    pub fn new(
        direction: u8,
        head_piece_bit: bool,
        occupied_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            direction,
            head_piece_bit,
            occupied_bit,
        })
    }

    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
    /// Get the head_piece_bit value.
    #[inline]
    pub fn head_piece_bit(&self) -> bool {
        self.head_piece_bit
    }
    /// Get the occupied_bit value.
    #[inline]
    pub fn occupied_bit(&self) -> bool {
        self.occupied_bit
    }
}

impl BlockState for BedState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.head_piece_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.occupied_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let direction = (rem % 4) as u8;
        rem /= 4;
        let head_piece_bit = (rem % 2) != 0;
        rem /= 2;
        let occupied_bit = (rem % 2) != 0;
        Some(Self {
            direction,
            head_piece_bit,
            occupied_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["grindstone"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GrindstoneState {
    attachment: Attachment,
    direction: u8,
}

impl GrindstoneState {
    /// Create a new state with validation.
    pub fn new(
        attachment: Attachment,
        direction: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            attachment,
            direction,
        })
    }

    /// Get the attachment value.
    #[inline]
    pub fn attachment(&self) -> Attachment {
        self.attachment
    }
    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
}

impl BlockState for GrindstoneState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.attachment as u32) * multiplier;
        multiplier *= 4;
        offset += (self.direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let attachment = Attachment::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let direction = (rem % 4) as u8;
        Some(Self {
            attachment,
            direction,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["chiseled_bookshelf"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChiseledBookshelfState {
    books_stored: u8,
    direction: u8,
}

impl ChiseledBookshelfState {
    /// Create a new state with validation.
    pub fn new(
        books_stored: u8,
        direction: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if books_stored > 63 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "books_stored",
                value: books_stored as u32,
                min: 0,
                max: 63,
            });
        }
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            books_stored,
            direction,
        })
    }

    /// Get the books_stored value.
    #[inline]
    pub fn books_stored(&self) -> u8 {
        self.books_stored
    }
    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
}

impl BlockState for ChiseledBookshelfState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.books_stored as u32) * multiplier;
        multiplier *= 64;
        offset += (self.direction as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 256 {
            return None;
        }
        let mut rem = offset;
        let books_stored = (rem % 64) as u8;
        rem /= 64;
        let direction = (rem % 4) as u8;
        Some(Self {
            books_stored,
            direction,
        })
    }

    fn state_count() -> u32 {
        256
    }
}

/// State shared by: ["jigsaw"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct JigsawState {
    facing_direction: u8,
    rotation: u8,
}

impl JigsawState {
    /// Create a new state with validation.
    pub fn new(
        facing_direction: u8,
        rotation: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if facing_direction > 5 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "facing_direction",
                value: facing_direction as u32,
                min: 0,
                max: 5,
            });
        }
        if rotation > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "rotation",
                value: rotation as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            facing_direction,
            rotation,
        })
    }

    /// Get the facing_direction value.
    #[inline]
    pub fn facing_direction(&self) -> u8 {
        self.facing_direction
    }
    /// Get the rotation value.
    #[inline]
    pub fn rotation(&self) -> u8 {
        self.rotation
    }
}

impl BlockState for JigsawState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.facing_direction as u32) * multiplier;
        multiplier *= 6;
        offset += (self.rotation as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 24 {
            return None;
        }
        let mut rem = offset;
        let facing_direction = (rem % 6) as u8;
        rem /= 6;
        let rotation = (rem % 4) as u8;
        Some(Self {
            facing_direction,
            rotation,
        })
    }

    fn state_count() -> u32 {
        24
    }
}

/// State shared by: ["lever"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LeverState {
    lever_direction: LeverDirection,
    open_bit: bool,
}

impl LeverState {
    /// Create a new state with validation.
    pub fn new(
        lever_direction: LeverDirection,
        open_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            lever_direction,
            open_bit,
        })
    }

    /// Get the lever_direction value.
    #[inline]
    pub fn lever_direction(&self) -> LeverDirection {
        self.lever_direction
    }
    /// Get the open_bit value.
    #[inline]
    pub fn open_bit(&self) -> bool {
        self.open_bit
    }
}

impl BlockState for LeverState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.lever_direction as u32) * multiplier;
        multiplier *= 8;
        offset += (self.open_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let lever_direction = LeverDirection::from_raw((rem % 8) as u8)?;
        rem /= 8;
        let open_bit = (rem % 2) != 0;
        Some(Self {
            lever_direction,
            open_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["calibrated_sculk_sensor"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CalibratedSculkSensorState {
    cardinal_direction: CardinalDirection,
    sculk_sensor_phase: u8,
}

impl CalibratedSculkSensorState {
    /// Create a new state with validation.
    pub fn new(
        cardinal_direction: CardinalDirection,
        sculk_sensor_phase: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if sculk_sensor_phase > 2 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "sculk_sensor_phase",
                value: sculk_sensor_phase as u32,
                min: 0,
                max: 2,
            });
        }
        Ok(Self {
            cardinal_direction,
            sculk_sensor_phase,
        })
    }

    /// Get the cardinal_direction value.
    #[inline]
    pub fn cardinal_direction(&self) -> CardinalDirection {
        self.cardinal_direction
    }
    /// Get the sculk_sensor_phase value.
    #[inline]
    pub fn sculk_sensor_phase(&self) -> u8 {
        self.sculk_sensor_phase
    }
}

impl BlockState for CalibratedSculkSensorState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.cardinal_direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.sculk_sensor_phase as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 12 {
            return None;
        }
        let mut rem = offset;
        let cardinal_direction = CardinalDirection::from_raw((rem % 4) as u8)?;
        rem /= 4;
        let sculk_sensor_phase = (rem % 3) as u8;
        Some(Self {
            cardinal_direction,
            sculk_sensor_phase,
        })
    }

    fn state_count() -> u32 {
        12
    }
}

/// State shared by: ["structure_block"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct StructureBlockState {
    structure_block_type: StructureBlockType,
}

impl StructureBlockState {
    /// Create a new state with validation.
    pub fn new(
        structure_block_type: StructureBlockType,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        Ok(Self {
            structure_block_type,
        })
    }

    /// Get the structure_block_type value.
    #[inline]
    pub fn structure_block_type(&self) -> StructureBlockType {
        self.structure_block_type
    }
}

impl BlockState for StructureBlockState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.structure_block_type as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 6 {
            return None;
        }
        let rem = offset;
        let structure_block_type = StructureBlockType::from_raw((rem % 6) as u8)?;
        Some(Self {
            structure_block_type,
        })
    }

    fn state_count() -> u32 {
        6
    }
}

/// State shared by: ["tripwire_hook"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TripwireHookState {
    attached_bit: bool,
    direction: u8,
    powered_bit: bool,
}

impl TripwireHookState {
    /// Create a new state with validation.
    pub fn new(
        attached_bit: bool,
        direction: u8,
        powered_bit: bool,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if direction > 3 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "direction",
                value: direction as u32,
                min: 0,
                max: 3,
            });
        }
        Ok(Self {
            attached_bit,
            direction,
            powered_bit,
        })
    }

    /// Get the attached_bit value.
    #[inline]
    pub fn attached_bit(&self) -> bool {
        self.attached_bit
    }
    /// Get the direction value.
    #[inline]
    pub fn direction(&self) -> u8 {
        self.direction
    }
    /// Get the powered_bit value.
    #[inline]
    pub fn powered_bit(&self) -> bool {
        self.powered_bit
    }
}

impl BlockState for TripwireHookState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.attached_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.direction as u32) * multiplier;
        multiplier *= 4;
        offset += (self.powered_bit as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let attached_bit = (rem % 2) != 0;
        rem /= 2;
        let direction = (rem % 4) as u8;
        rem /= 4;
        let powered_bit = (rem % 2) != 0;
        Some(Self {
            attached_bit,
            direction,
            powered_bit,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["snow_layer"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SnowLayerState {
    covered_bit: bool,
    height: u8,
}

impl SnowLayerState {
    /// Create a new state with validation.
    pub fn new(
        covered_bit: bool,
        height: u8,
    ) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if height > 7 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "height",
                value: height as u32,
                min: 0,
                max: 7,
            });
        }
        Ok(Self {
            covered_bit,
            height,
        })
    }

    /// Get the covered_bit value.
    #[inline]
    pub fn covered_bit(&self) -> bool {
        self.covered_bit
    }
    /// Get the height value.
    #[inline]
    pub fn height(&self) -> u8 {
        self.height
    }
}

impl BlockState for SnowLayerState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let mut multiplier = 1u32;
        offset += (self.covered_bit as u32) * multiplier;
        multiplier *= 2;
        offset += (self.height as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 16 {
            return None;
        }
        let mut rem = offset;
        let covered_bit = (rem % 2) != 0;
        rem /= 2;
        let height = (rem % 8) as u8;
        Some(Self {
            covered_bit,
            height,
        })
    }

    fn state_count() -> u32 {
        16
    }
}

/// State shared by: ["farmland"]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FarmlandState {
    moisturized_amount: u8,
}

impl FarmlandState {
    /// Create a new state with validation.
    pub fn new(moisturized_amount: u8) -> Result<Self, valentine_bedrock_core::block::StateError> {
        if moisturized_amount > 7 {
            return Err(valentine_bedrock_core::block::StateError::OutOfRange {
                field: "moisturized_amount",
                value: moisturized_amount as u32,
                min: 0,
                max: 7,
            });
        }
        Ok(Self { moisturized_amount })
    }

    /// Get the moisturized_amount value.
    #[inline]
    pub fn moisturized_amount(&self) -> u8 {
        self.moisturized_amount
    }
}

impl BlockState for FarmlandState {
    fn state_offset(&self) -> u32 {
        let mut offset = 0u32;
        let multiplier = 1u32;
        offset += (self.moisturized_amount as u32) * multiplier;
        offset
    }

    fn from_offset(offset: u32) -> Option<Self> {
        if offset >= 8 {
            return None;
        }
        let rem = offset;
        let moisturized_amount = (rem % 8) as u8;
        Some(Self { moisturized_amount })
    }

    fn state_count() -> u32 {
        8
    }
}
