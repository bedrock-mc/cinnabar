//! Block classes supplied by the caller, so this crate never reads runtime assets.

use std::collections::HashMap;

use crate::voxel::{MaterialId, Voxel};

/// How a block takes part in sampling and mipping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockClass {
    /// Empty space, including unknown blocks.
    Air,
    /// A full block that hides its neighbours' faces.
    Opaque,
    /// Foliage: see-through at level 0, opaque when mipped.
    Leaves,
    /// Water source or flow.
    Water,
    /// Any other non-air block light passes through: glass, plants, partial shapes.
    SeeThrough,
}

impl BlockClass {
    /// True when a neighbouring voxel's face next to this one can be seen when mipping.
    #[must_use]
    pub const fn exposes_neighbours(self) -> bool {
        matches!(self, Self::Air | Self::Water | Self::SeeThrough)
    }

    /// True for blocks whose light the mip averages.
    #[must_use]
    pub const fn is_see_through(self) -> bool {
        self.exposes_neighbours()
    }
}

/// A block's store-local material and class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SampledBlock {
    pub material: MaterialId,
    pub class: BlockClass,
}

impl SampledBlock {
    /// Air, the classification of every unknown value.
    pub const AIR: Self = Self {
        material: MaterialId::AIR,
        class: BlockClass::Air,
    };

    /// Makes air consistent: an air class or air material yields [`SampledBlock::AIR`].
    /// The reserved hidden id is not a block material and also yields air.
    #[must_use]
    pub fn normalized(self) -> Self {
        if self.class == BlockClass::Air || !self.material.is_block() {
            Self::AIR
        } else {
            self
        }
    }
}

/// Resolves raw palette values to materials and classes while sampling level 0.
///
/// The sampler calls this once per distinct palette value per column, so implementations
/// may assign new material ids as they meet new block states.
pub trait BlockClassifier {
    /// Material and class of one raw palette value of the column's id mode.
    fn classify(&mut self, runtime_id: u32) -> SampledBlock;
}

/// Classes of store-local materials, resolved from current assets by the caller.
pub trait MaterialClasses {
    /// Class of one block material; never called for air or hidden. A stored material is
    /// never air, so materials the current assets no longer resolve should be opaque; an
    /// [`BlockClass::Air`] answer is read as opaque by [`voxel_class`].
    fn class(&self, material: MaterialId) -> BlockClass;
}

/// The mip class of a stored voxel: air is air, hidden is opaque, blocks ask `classes`.
/// A block material is never air, so occupancy survives a classifier that answers air.
#[must_use]
pub fn voxel_class(classes: &impl MaterialClasses, voxel: Voxel) -> BlockClass {
    let material = voxel.material();
    if material == MaterialId::AIR {
        BlockClass::Air
    } else if material == MaterialId::HIDDEN {
        BlockClass::Opaque
    } else {
        match classes.class(material) {
            BlockClass::Air => BlockClass::Opaque,
            class => class,
        }
    }
}

/// A fixed runtime-id table for tests, benches and tools; unknown runtime ids are air and
/// unknown materials are opaque.
#[derive(Debug, Clone, Default)]
pub struct TableClassifier {
    by_runtime_id: HashMap<u32, SampledBlock>,
    classes: HashMap<MaterialId, BlockClass>,
}

impl TableClassifier {
    /// Maps `runtime_id` to `block`, replacing an earlier entry.
    pub fn insert(&mut self, runtime_id: u32, block: SampledBlock) -> &mut Self {
        let block = block.normalized();
        self.by_runtime_id.insert(runtime_id, block);
        if block.material.is_block() {
            self.classes.insert(block.material, block.class);
        }
        self
    }
}

impl BlockClassifier for TableClassifier {
    fn classify(&mut self, runtime_id: u32) -> SampledBlock {
        self.by_runtime_id
            .get(&runtime_id)
            .copied()
            .unwrap_or(SampledBlock::AIR)
    }
}

impl MaterialClasses for TableClassifier {
    fn class(&self, material: MaterialId) -> BlockClass {
        self.classes
            .get(&material)
            .copied()
            .unwrap_or(BlockClass::Opaque)
    }
}
