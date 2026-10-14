//! What a renderer uploads for distant terrain, laid out once here so shader constants can
//! be generated from it: which crack lists each face draws ([`NeighbourLevels`]), cluster
//! and section records ([`records`]), packed biome grids ([`biomes`]) and the far-to-near
//! order of translucent sections ([`order`]).

pub mod biomes;
pub mod order;
pub mod records;

use crate::{geometry::Face, mesh::MeshList};

/// How the region across one face of a drawn section is drawn, which picks the crack list
/// drawn with the main list there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum NeighbourLevel {
    /// Nothing is drawn across the face: unexplored, out of range or left to resident
    /// chunks. Only the main list is drawn.
    None = 0,
    /// The same-level neighbour is drawn. Only the main list is drawn.
    Same = 1,
    /// A coarser section covers the neighbour: [`MeshList::ToCoarser`] is drawn too.
    Coarser = 2,
    /// Finer sections cover the neighbour: [`MeshList::ToFiner`] is drawn too.
    Finer = 3,
}

impl NeighbourLevel {
    /// Every value in code order.
    pub const ALL: [Self; 4] = [Self::None, Self::Same, Self::Coarser, Self::Finer];

    /// The value of a [`NEIGHBOUR_LEVEL_BITS`]-bit code; codes past 3 never occur.
    #[must_use]
    pub const fn from_code(code: u16) -> Self {
        Self::ALL[(code & NEIGHBOUR_LEVEL_MASK) as usize]
    }

    /// True when `list` is drawn on a face whose neighbour region is drawn this way.
    #[must_use]
    pub const fn draws(self, list: MeshList) -> bool {
        matches!(
            (list, self),
            (MeshList::Main, _)
                | (MeshList::ToCoarser, Self::Coarser)
                | (MeshList::ToFiner, Self::Finer)
        )
    }
}

/// Bits of one face's [`NeighbourLevel`] code in [`NeighbourLevels`].
pub const NEIGHBOUR_LEVEL_BITS: u32 = 2;
const NEIGHBOUR_LEVEL_MASK: u16 = (1 << NEIGHBOUR_LEVEL_BITS) - 1;
/// Bits of a whole [`NeighbourLevels`] word.
pub const NEIGHBOUR_LEVELS_BITS: u32 = NEIGHBOUR_LEVEL_BITS * Face::ALL.len() as u32;

const _: () = assert!(NeighbourLevel::ALL.len() <= 1 << NEIGHBOUR_LEVEL_BITS);
const _: () = assert!(NEIGHBOUR_LEVELS_BITS <= u16::BITS);

/// The [`NeighbourLevel`] of each face of a drawn section: face `f`'s code in bits
/// `2f..2f + 2`, by [`Face::index`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct NeighbourLevels(u16);

impl NeighbourLevels {
    /// Nothing drawn across any face.
    pub const NONE: Self = Self(0);

    /// The levels from one value per face, by [`Face::index`].
    #[must_use]
    pub fn new(levels: [NeighbourLevel; 6]) -> Self {
        let mut out = Self::NONE;
        for face in Face::ALL {
            out = out.with(face, levels[face.index()]);
        }
        out
    }

    /// This word with `face` set to `level`.
    #[must_use]
    pub const fn with(self, face: Face, level: NeighbourLevel) -> Self {
        let shift = face.index() as u32 * NEIGHBOUR_LEVEL_BITS;
        Self(self.0 & !(NEIGHBOUR_LEVEL_MASK << shift) | (level as u16) << shift)
    }

    /// The level across `face`.
    #[must_use]
    pub const fn get(self, face: Face) -> NeighbourLevel {
        NeighbourLevel::from_code(self.0 >> (face.index() as u32 * NEIGHBOUR_LEVEL_BITS))
    }

    /// True when the quads of `list` facing `face` are drawn.
    #[must_use]
    pub const fn draws(self, list: MeshList, face: Face) -> bool {
        self.get(face).draws(list)
    }

    /// The packed word.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_round_trip_per_face_and_pick_crack_lists() {
        let levels = NeighbourLevels::new([
            NeighbourLevel::None,
            NeighbourLevel::Same,
            NeighbourLevel::Coarser,
            NeighbourLevel::Finer,
            NeighbourLevel::Same,
            NeighbourLevel::Coarser,
        ]);
        assert_eq!(levels.get(Face::NegX), NeighbourLevel::None);
        assert_eq!(levels.get(Face::NegY), NeighbourLevel::Coarser);
        assert_eq!(levels.get(Face::PosY), NeighbourLevel::Finer);
        assert_eq!(levels.get(Face::PosZ), NeighbourLevel::Coarser);
        assert!(levels.bits() < 1 << NEIGHBOUR_LEVELS_BITS);
        for face in Face::ALL {
            assert!(levels.draws(MeshList::Main, face));
        }
        assert!(levels.draws(MeshList::ToCoarser, Face::NegY));
        assert!(!levels.draws(MeshList::ToFiner, Face::NegY));
        assert!(levels.draws(MeshList::ToFiner, Face::PosY));
        assert!(!levels.draws(MeshList::ToCoarser, Face::PosX));
        let changed = levels.with(Face::NegY, NeighbourLevel::Same);
        assert_eq!(changed.get(Face::NegY), NeighbourLevel::Same);
        assert_eq!(changed.get(Face::PosY), NeighbourLevel::Finer);
    }
}
