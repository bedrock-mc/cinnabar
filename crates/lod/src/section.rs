//! The stored section: position, packed voxels, biome grid and child existence.

use crate::{
    biome_grid::BiomeGrid,
    geometry::{Octant, SectionPos},
    storage::{DenseSection, PackScratch, PackedVoxels, StorageError},
};

/// What is known about one child section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChildState {
    /// Never sampled or loaded; unexplored space, not air.
    Unknown,
    /// Known to hold only air.
    Empty,
    /// Known to hold at least one non-air voxel.
    NonEmpty,
}

impl ChildState {
    /// The state of a sampled level-0 section as a child: known once any of its sub-chunks
    /// was sampled, and non-empty when any voxel holds a material.
    #[must_use]
    pub fn of_level_zero(section: &DenseSection) -> Self {
        if section.is_all_air() {
            Self::Empty
        } else {
            Self::NonEmpty
        }
    }
}

/// Tri-state existence of a section's eight children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ChildMask {
    known: u8,
    non_empty: u8,
}

impl ChildMask {
    /// Every child unknown; also the mask of every level-0 section.
    pub const UNKNOWN: Self = Self {
        known: 0,
        non_empty: 0,
    };

    /// Decodes [`ChildMask::to_bits`]; a non-empty child must also be known.
    pub fn from_bits(bits: u16) -> Result<Self, StorageError> {
        let [known, non_empty] = bits.to_le_bytes();
        if non_empty & !known != 0 {
            return Err(StorageError::ChildMask(bits));
        }
        Ok(Self { known, non_empty })
    }

    /// Known bits in the low byte, non-empty bits in the high byte, by octant index.
    #[must_use]
    pub const fn to_bits(self) -> u16 {
        u16::from_le_bytes([self.known, self.non_empty])
    }

    /// The recorded state of the child at `octant`.
    #[must_use]
    pub const fn get(self, octant: Octant) -> ChildState {
        let bit = 1 << octant.index();
        if self.known & bit == 0 {
            ChildState::Unknown
        } else if self.non_empty & bit == 0 {
            ChildState::Empty
        } else {
            ChildState::NonEmpty
        }
    }

    /// Records `state`; returns true when the recorded state changed, which the parent's
    /// own state ([`Self::summary`]) may then reflect.
    pub fn set(&mut self, octant: Octant, state: ChildState) -> bool {
        let before = self.get(octant);
        let bit = 1 << octant.index();
        match state {
            ChildState::Unknown => {
                self.known &= !bit;
                self.non_empty &= !bit;
            }
            ChildState::Empty => {
                self.known |= bit;
                self.non_empty &= !bit;
            }
            ChildState::NonEmpty => {
                self.known |= bit;
                self.non_empty |= bit;
            }
        }
        before != state
    }

    /// True when any child is known non-empty.
    #[must_use]
    pub const fn any_non_empty(self) -> bool {
        self.non_empty != 0
    }

    /// This section's own state as a child of the next level: non-empty when any child is,
    /// empty only when all eight children are known empty, unknown otherwise. A partly
    /// explored section is never empty, so its parent never fills unexplored space with
    /// sky-lit air.
    #[must_use]
    pub const fn summary(self) -> ChildState {
        if self.non_empty != 0 {
            ChildState::NonEmpty
        } else if self.all_known() {
            ChildState::Empty
        } else {
            ChildState::Unknown
        }
    }

    /// True when every child's state is known.
    #[must_use]
    pub const fn all_known(self) -> bool {
        self.known == u8::MAX
    }
}

/// One section in its compact resident and stored form. A level-0 section has no children,
/// so its mask is always [`ChildMask::UNKNOWN`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pos: SectionPos,
    voxels: PackedVoxels,
    biomes: BiomeGrid,
    children: ChildMask,
}

impl Section {
    /// Assembles validated parts; fails when a level-0 section is given a child mask.
    pub fn new(
        pos: SectionPos,
        voxels: PackedVoxels,
        biomes: BiomeGrid,
        children: ChildMask,
    ) -> Result<Self, StorageError> {
        check_children(pos, children)?;
        Ok(Self {
            pos,
            voxels,
            biomes,
            children,
        })
    }

    /// Packs a dense working section into canonical form, reusing `scratch`; fails as
    /// [`Self::new`] does.
    pub fn from_dense(
        pos: SectionPos,
        dense: &DenseSection,
        children: ChildMask,
        scratch: &mut PackScratch,
    ) -> Result<Self, StorageError> {
        Self::new(
            pos,
            PackedVoxels::from_dense(dense.voxels(), scratch),
            dense.biomes().canonical(),
            children,
        )
    }

    /// Expands into `out`, reusing its allocation.
    pub fn expand_into(&self, out: &mut DenseSection) {
        out.load(&self.voxels, &self.biomes);
    }

    /// Where the section lies.
    #[must_use]
    pub fn pos(&self) -> SectionPos {
        self.pos
    }

    /// The packed voxels.
    #[must_use]
    pub fn voxels(&self) -> &PackedVoxels {
        &self.voxels
    }

    /// The surface biome of each voxel column.
    #[must_use]
    pub fn biomes(&self) -> &BiomeGrid {
        &self.biomes
    }

    /// What is known about the eight children.
    #[must_use]
    pub fn children(&self) -> ChildMask {
        self.children
    }

    /// Records one child's state as [`ChildMask::set`] does; fails, changing nothing, on a
    /// level-0 section unless `state` is unknown.
    pub fn set_child(&mut self, octant: Octant, state: ChildState) -> Result<bool, StorageError> {
        let mut children = self.children;
        let changed = children.set(octant, state);
        check_children(self.pos, children)?;
        self.children = children;
        Ok(changed)
    }

    /// True when the section holds only air.
    #[must_use]
    pub fn is_all_air(&self) -> bool {
        self.voxels.is_all_air()
    }
}

/// Rejects a child mask on a level-0 section.
fn check_children(pos: SectionPos, children: ChildMask) -> Result<(), StorageError> {
    if pos.level() == 0 && children != ChildMask::UNKNOWN {
        return Err(StorageError::LevelZeroChildren(children.to_bits()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        biome_grid::UNKNOWN_BIOME,
        storage::PackScratch,
        voxel::{Light, Voxel},
    };

    #[test]
    fn child_mask_tracks_three_states_and_reports_bubbling_changes() {
        let mut mask = ChildMask::UNKNOWN;
        let octant = Octant::ALL[5];
        assert_eq!(mask.get(octant), ChildState::Unknown);
        assert!(mask.set(octant, ChildState::Empty));
        assert!(!mask.set(octant, ChildState::Empty));
        assert!(!mask.any_non_empty());
        assert!(mask.set(octant, ChildState::NonEmpty));
        assert!(mask.any_non_empty());
        assert_eq!(mask.get(Octant::ALL[4]), ChildState::Unknown);
        assert_eq!(ChildMask::from_bits(mask.to_bits()), Ok(mask));
        assert!(mask.set(octant, ChildState::Unknown));
        assert_eq!(mask, ChildMask::UNKNOWN);
    }

    #[test]
    fn child_mask_rejects_non_empty_unknown_children() {
        assert_eq!(
            ChildMask::from_bits(0x0100),
            Err(StorageError::ChildMask(0x0100))
        );
        let all = ChildMask::from_bits(0x01ff).expect("valid mask");
        assert!(all.all_known());
        assert_eq!(all.get(Octant::ALL[0]), ChildState::NonEmpty);
        assert_eq!(all.get(Octant::ALL[1]), ChildState::Empty);
    }

    #[test]
    fn level_zero_sections_reject_child_masks() {
        let dense = DenseSection::filled(Voxel::air(Light::FULL_SKY), UNKNOWN_BIOME);
        let mut scratch = PackScratch::default();
        let level0 = SectionPos::new(0, 1, -2, 3).expect("pos");
        let mut mask = ChildMask::UNKNOWN;
        mask.set(Octant::ALL[2], ChildState::Empty);
        assert_eq!(
            Section::from_dense(level0, &dense, mask, &mut scratch),
            Err(StorageError::LevelZeroChildren(mask.to_bits()))
        );
        let mut section = Section::from_dense(level0, &dense, ChildMask::UNKNOWN, &mut scratch)
            .expect("unknown mask");
        assert_eq!(
            section.set_child(Octant::ALL[2], ChildState::NonEmpty),
            Err(StorageError::LevelZeroChildren(0x0404))
        );
        assert_eq!(section.children(), ChildMask::UNKNOWN);
        assert_eq!(
            section.set_child(Octant::ALL[2], ChildState::Unknown),
            Ok(false)
        );

        let level1 = level0.parent().expect("level 1");
        let mut parent = Section::from_dense(level1, &dense, ChildMask::UNKNOWN, &mut scratch)
            .expect("unknown mask");
        assert_eq!(
            parent.set_child(Octant::ALL[2], ChildState::Empty),
            Ok(true)
        );
        assert_eq!(parent.children(), mask);
    }

    #[test]
    fn summaries_report_a_section_as_a_child() {
        let mut mask = ChildMask::UNKNOWN;
        assert_eq!(mask.summary(), ChildState::Unknown);
        for (index, octant) in Octant::ALL.into_iter().enumerate() {
            assert_eq!(
                mask.summary(),
                ChildState::Unknown,
                "{index} empty children leave the rest unexplored"
            );
            mask.set(octant, ChildState::Empty);
        }
        assert_eq!(mask.summary(), ChildState::Empty);
        mask.set(Octant::ALL[6], ChildState::Unknown);
        assert_eq!(mask.summary(), ChildState::Unknown);
        mask.set(Octant::ALL[6], ChildState::NonEmpty);
        assert_eq!(mask.summary(), ChildState::NonEmpty);
        let mut partial = ChildMask::UNKNOWN;
        partial.set(Octant::ALL[3], ChildState::NonEmpty);
        assert_eq!(partial.summary(), ChildState::NonEmpty);

        let mut dense = DenseSection::filled(Voxel::AIR, UNKNOWN_BIOME);
        assert_eq!(ChildState::of_level_zero(&dense), ChildState::Empty);
        dense.set(3, 4, 5, Voxel::HIDDEN);
        assert_eq!(ChildState::of_level_zero(&dense), ChildState::NonEmpty);
    }
}
