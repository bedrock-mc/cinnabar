//! Native stair direction identity, shared by collision and visual geometry.

/// `weirdo_direction` is not Bedrock's ordinary four-way direction encoding.
///
/// Stair step and inner pieces: direction zero occupies the +X half, one -X, two +Z, three -Z.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StairDirection {
    East,
    West,
    South,
    North,
}

impl StairDirection {
    #[must_use]
    pub const fn from_raw(value: u32) -> Option<Self> {
        match value {
            0 => Some(Self::East),
            1 => Some(Self::West),
            2 => Some(Self::South),
            3 => Some(Self::North),
            _ => None,
        }
    }

    /// Clockwise quarter turns in authored +X/+Z block coordinates.
    #[must_use]
    pub const fn turns_from_east(self) -> u32 {
        match self {
            Self::East => 0,
            Self::West => 2,
            Self::South => 1,
            Self::North => 3,
        }
    }

    /// The compiled stair template points north; collision pieces point east.
    #[must_use]
    pub const fn turns_from_north(self) -> u32 {
        (self.turns_from_east() + 1) & 3
    }
}
