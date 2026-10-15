//! Bounded copies of the terrain data used by surface extraction.

use std::{mem::size_of, ops::Bound};

use world::{BiomeStorage, PalettedStorage};

use crate::{ColumnSnapshot, extract::SURFACE_LAYERS};

/// Owned input for one extraction step. It retains no section, registry or wire-cache Arc.
pub struct SurfaceSlice {
    pub(crate) sections: Box<[SurfaceSection]>,
    pub(crate) next: Option<i32>,
    bytes: usize,
}

pub(crate) struct SurfaceSection {
    pub(crate) y: i32,
    pub(crate) storages: Box<[PalettedStorage]>,
    pub(crate) biome: Option<BiomeStorage>,
}

impl SurfaceSlice {
    /// Space sufficient for two decoded or edited visible layers and one biome layer,
    /// including retained physical slots and a possible canonical word cache.
    pub const MIN_BYTES: usize = size_of::<Self>()
        + size_of::<SurfaceSection>()
        + SURFACE_LAYERS
            * (size_of::<PalettedStorage>() + PalettedStorage::MAX_RETAINED_HEAP_BYTES)
        + BiomeStorage::MAX_RETAINED_HEAP_BYTES;

    /// Conservative retained input bytes, including metadata, physical storage and lazy caches.
    /// Shared allocations are charged in full; observing words cannot grow beyond this reservation.
    pub fn retained_bytes(&self) -> usize {
        self.bytes
    }

    /// Upper-exclusive section cursor for the next step, or none when the source is exhausted.
    pub fn next(&self) -> Option<i32> {
        self.next
    }
}

impl ColumnSnapshot {
    /// Copies a top-down slice with at most `limit` bytes, starting below `before`.
    /// The minimum covers even the largest legal single section, so tall or multilayered
    /// sources always make progress without retaining the original section allocations.
    pub fn surface_slice(&self, before: Option<i32>, limit: usize) -> SurfaceSlice {
        assert!(limit >= SurfaceSlice::MIN_BYTES);
        let upper = before.map_or(Bound::Unbounded, Bound::Excluded);
        let mut slice = SurfaceSlice {
            sections: Box::new([]),
            next: None,
            bytes: size_of::<SurfaceSlice>(),
        };
        let mut sections = Vec::new();
        for (&y, section) in self.sections.range((Bound::Unbounded, upper)).rev() {
            let storages = section.storages();
            let biome = self.biome(y);
            let payload = storages
                .iter()
                .take(SURFACE_LAYERS)
                .map(PalettedStorage::retained_heap_bytes_bound)
                .sum::<usize>()
                + biome.map_or(0, BiomeStorage::retained_heap_bytes_bound);
            let added = size_of::<SurfaceSection>()
                + storages.len().min(SURFACE_LAYERS) * size_of::<PalettedStorage>()
                + payload;
            if slice.bytes + added > limit {
                break;
            }
            sections.push(SurfaceSection {
                y,
                storages: storages.iter().take(SURFACE_LAYERS).cloned().collect(),
                biome: biome.cloned(),
            });
            slice.bytes += added;
            slice.next = Some(y);
        }
        slice.sections = sections.into_boxed_slice();
        if let Some(last) = slice.next
            && self.sections.range(..last).next().is_none()
        {
            slice.next = None;
        }
        slice
    }

    /// Conservative retained block/biome payload in the source backlog, counting shared data again.
    /// Includes all layers, map entries and Arc headers; registry allocations remain owned by
    /// the session context. This is source retention, separate from admitted slice copies.
    pub fn retained_payload_bytes(&self) -> usize {
        size_of::<Self>()
            + 2 * size_of::<usize>()
            + self
                .sections
                .values()
                .map(|section| {
                    // A separate B-tree node per entry is a conservative map overhead estimate.
                    16 * size_of::<usize>()
                        + size_of::<world::SubChunk>()
                        + std::mem::size_of_val(section.storages())
                        + section
                            .storages()
                            .iter()
                            .map(PalettedStorage::retained_heap_bytes_bound)
                            .sum::<usize>()
                })
                .sum::<usize>()
            + self.biomes.capacity() * size_of::<(i32, std::sync::Arc<BiomeStorage>)>()
            + self
                .biomes
                .iter()
                .map(|(_, biome)| {
                    2 * size_of::<usize>()
                        + size_of::<BiomeStorage>()
                        + biome.retained_heap_bytes_bound()
                })
                .sum::<usize>()
    }
}

#[cfg(test)]
mod tests;
