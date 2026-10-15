//! Per-mesh memo of the block facts halo queries read, kept per storage palette value.
//!
//! AO, shade and leaf-plane predicates ask about thousands of cells around one
//! sub-chunk, but those cells hold only a few distinct palette values. Each
//! answer is memoized per `(snapshot slot, storage, palette index)`, so a value
//! is resolved against the registry once per mesh instead of once per query.

use std::cell::{Cell, OnceCell};

use assets::{
    BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF, BLOCK_VISUAL_VARIANT_SEASONAL_LEAF,
    BLOCK_VISUAL_VARIANT_TOP_SNOW, BlockFlags, NetworkIdMode, RuntimeAssets,
};
use world::{MeshNeighbourhood, SubChunk};

use crate::{
    BlockClassifier, SIDE,
    contributors::{packed_palette_index, palette_entry_of_block},
};

/// Native cached solid-render bit: the registered visual occludes a full face and is not
/// TopSnow, whose constructor clears solid-render for every height.
pub(crate) const SOLID_RENDER: u8 = 1 << 0;
/// The registered visual is a leaf, which darkens shade whatever its solidity.
pub(crate) const SHADE_LEAF: u8 = 1 << 1;
/// The resolved meshing entry keeps [`BlockFlags::LEAF_MODEL`].
pub(crate) const ENTRY_LEAF: u8 = 1 << 2;
/// The resolved meshing entry keeps [`BlockFlags::OCCLUDES_FULL_FACE`].
pub(crate) const ENTRY_OCCLUDES: u8 = 1 << 3;
/// The resolved variant is a seasonal or non-seasonal native leaf.
pub(crate) const ENTRY_LEAF_VARIANT: u8 = 1 << 4;
/// The resolved variant carries TopSnow.
pub(crate) const ENTRY_TOP_SNOW: u8 = 1 << 5;
/// Memo state: the value is unregistered, so every query resolves it again and is
/// counted as a registry miss exactly as an unmemoized query would be.
const UNKNOWN: u8 = 1 << 6;
/// Memo state: the cell holds the value's bits.
const KNOWN: u8 = 1 << 7;

const WIDTH: usize = 3;
const SLOTS: usize = WIDTH * WIDTH * WIDTH;

/// Lazily filled per-palette-value bits for every sub-chunk of one mesh snapshot.
pub(crate) struct NeighbourhoodBlockBits<'a, 'n> {
    classifier: BlockClassifier,
    assets: &'a RuntimeAssets,
    mode: NetworkIdMode,
    neighbourhood: &'a MeshNeighbourhood<'n>,
    /// Per snapshot slot, one memo cell per palette entry of every storage, storages
    /// concatenated in layer order.
    slots: [OnceCell<Box<[Cell<u8>]>>; SLOTS],
}

impl<'a, 'n> NeighbourhoodBlockBits<'a, 'n> {
    pub(crate) fn new(
        classifier: BlockClassifier,
        assets: &'a RuntimeAssets,
        mode: NetworkIdMode,
        neighbourhood: &'a MeshNeighbourhood<'n>,
    ) -> Self {
        Self {
            classifier,
            assets,
            mode,
            neighbourhood,
            slots: std::array::from_fn(|_| OnceCell::new()),
        }
    }

    /// Bits of storage `layer` at a snapshot coordinate. Coordinates outside the
    /// snapshot, absent sub-chunks or layers, and indices with no palette value read
    /// as zero, as the open space [`MeshNeighbourhood::sample`] reports for them.
    pub(crate) fn layer(&self, layer: usize, coordinate: [i32; 3]) -> u8 {
        self.locate(coordinate)
            .map_or(0, |(slot, sub_chunk, local)| {
                self.storage_bits(slot, sub_chunk, layer, local)
            })
    }

    /// Whether any storage at the coordinate carries [`SOLID_RENDER`], testing layers in
    /// order and stopping at the first, like the direct per-layer registry scan.
    pub(crate) fn solid_render(&self, coordinate: [i32; 3]) -> bool {
        let Some((slot, sub_chunk, local)) = self.locate(coordinate) else {
            return false;
        };
        (0..sub_chunk.storages().len())
            .any(|layer| self.storage_bits(slot, sub_chunk, layer, local) & SOLID_RENDER != 0)
    }

    /// Snapshot slot, source sub-chunk and local coordinate, split exactly as
    /// [`MeshNeighbourhood::block_source`] splits it.
    fn locate(&self, coordinate: [i32; 3]) -> Option<(usize, &'n SubChunk, [usize; 3])> {
        let side = SIDE as i32;
        let offset = coordinate.map(|value| value.div_euclid(side));
        if offset.iter().any(|value| !(-1..=1).contains(value)) {
            return None;
        }
        let sub_chunk = self
            .neighbourhood
            .sub_chunk(offset.map(|value| value as i8))?;
        let [x, y, z] = offset.map(|value| (value + 1) as usize);
        Some((
            (x * WIDTH + y) * WIDTH + z,
            sub_chunk,
            coordinate.map(|value| value.rem_euclid(side) as usize),
        ))
    }

    fn storage_bits(
        &self,
        slot: usize,
        sub_chunk: &SubChunk,
        layer: usize,
        [x, y, z]: [usize; 3],
    ) -> u8 {
        let storages = sub_chunk.storages();
        let Some(storage) = storages.get(layer) else {
            return 0;
        };
        let Some(index) = packed_palette_index(storage, x, y, z) else {
            return 0;
        };
        let Some(&value) = storage.palette().values().get(index) else {
            return 0;
        };
        let memo = self.slots[slot].get_or_init(|| {
            let len = storages.iter().map(|storage| storage.palette().len()).sum();
            vec![Cell::new(0); len].into_boxed_slice()
        });
        let start: usize = storages[..layer]
            .iter()
            .map(|storage| storage.palette().len())
            .sum();
        let cell = &memo[start + index];
        let mut state = cell.get();
        if state == 0 {
            state = self.value_bits(value);
            cell.set(state);
        } else if state & UNKNOWN != 0 {
            let _ = self.assets.resolve(self.mode, value);
        }
        if state & UNKNOWN != 0 {
            return 0;
        }
        state & !KNOWN
    }

    /// Computes one palette value's bits with [`KNOWN`], or [`UNKNOWN`] for an
    /// unregistered value, whose registered facts are all clear.
    fn value_bits(&self, value: u32) -> u8 {
        if self.classifier.is_air(value) {
            return KNOWN;
        }
        let block = self.assets.resolve(self.mode, value);
        if !block.is_known() {
            return UNKNOWN;
        }
        let flags = block.flags();
        let entry = palette_entry_of_block(self.assets, self.mode, value, block);
        let mut bits = KNOWN;
        if block.variant() != BLOCK_VISUAL_VARIANT_TOP_SNOW
            && flags.contains(BlockFlags::OCCLUDES_FULL_FACE)
        {
            bits |= SOLID_RENDER;
        }
        if flags.contains(BlockFlags::LEAF_MODEL) {
            bits |= SHADE_LEAF;
        }
        if entry.flags.contains(BlockFlags::LEAF_MODEL) {
            bits |= ENTRY_LEAF;
        }
        if entry.flags.contains(BlockFlags::OCCLUDES_FULL_FACE) {
            bits |= ENTRY_OCCLUDES;
        }
        if entry.variant
            & (BLOCK_VISUAL_VARIANT_SEASONAL_LEAF | BLOCK_VISUAL_VARIANT_NONSEASONAL_LEAF)
            != 0
        {
            bits |= ENTRY_LEAF_VARIANT;
        }
        if entry.variant & BLOCK_VISUAL_VARIANT_TOP_SNOW != 0 {
            bits |= ENTRY_TOP_SNOW;
        }
        bits
    }
}
