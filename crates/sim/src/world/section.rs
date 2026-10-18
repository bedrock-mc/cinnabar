//! Cached sub-chunk reads shared by every per-cell palette scan.

use std::sync::Arc;

use world::{SubChunk, SubChunkKey};

use super::{PaletteWorld, WorldQueryError};

/// The sub-chunk a scan read last, so its cells look the section up once instead of once each.
#[derive(Default)]
pub(super) struct SectionCache {
    key: Option<SubChunkKey>,
    /// `None` when the section is unloaded; `Some(None)` when loaded with no palette storage.
    section: Option<Option<Arc<SubChunk>>>,
}

impl SectionCache {
    /// The loaded section at `key` (`Some(None)` for an all-air one), or `None` when unloaded.
    pub(super) fn get(
        &mut self,
        world: &PaletteWorld<'_>,
        key: SubChunkKey,
    ) -> Option<&Option<Arc<SubChunk>>> {
        world.record_cell_read(key).ok()?;
        if self.key != Some(key) {
            self.key = Some(key);
            self.section = world
                .store
                .is_sub_chunk_loaded(key)
                .then(|| world.store.sub_chunk(key));
        }
        self.section.as_ref()
    }
}

/// Every storage layer's runtime id at one block, borrowed from a [`SectionCache`].
///
/// A block without palette storage (an absent or storage-free section, or an unloaded
/// movement-boundary column) reads as one air layer.
#[derive(Clone, Copy)]
pub(super) struct BlockLayers<'a> {
    section: Option<&'a SubChunk>,
    local: [u8; 3],
    air: u32,
}

impl BlockLayers<'_> {
    /// Number of layers; always at least one.
    pub(super) fn len(self) -> usize {
        self.section.map_or(1, |section| section.storages().len())
    }

    /// The runtime id stored on `layer`, or `None` past the last layer.
    pub(super) fn get(self, layer: usize) -> Option<u32> {
        match self.section {
            None => (layer == 0).then_some(self.air),
            Some(section) => {
                let [x, y, z] = self.local;
                section.runtime_id(layer, x, y, z)
            }
        }
    }

    /// Runtime ids from the primary layer upward.
    pub(super) fn iter(self) -> impl Iterator<Item = u32> {
        (0..self.len()).map(move |layer| {
            self.get(layer)
                .expect("validated palette storage resolves every local coordinate")
        })
    }
}

impl PaletteWorld<'_> {
    /// Reads the layers of a loaded block through `section`.
    ///
    /// An unloaded section reads as air only inside an unloaded movement-boundary column;
    /// otherwise it fails with [`WorldQueryError::UnloadedChunk`] for its column.
    pub(super) fn layers_at<'c>(
        &self,
        [x, y, z]: [i32; 3],
        section: &'c mut SectionCache,
    ) -> Result<BlockLayers<'c>, WorldQueryError> {
        let key = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
        let air = self.registry.air_runtime_id;
        let Some(loaded) = section.get(self, key) else {
            if self.unloaded_boundary_revision(key.chunk()).is_some() {
                return Ok(BlockLayers {
                    section: None,
                    local: [0; 3],
                    air,
                });
            }
            return Err(WorldQueryError::UnloadedChunk(key.chunk()));
        };
        Ok(BlockLayers {
            section: loaded
                .as_deref()
                .filter(|section| !section.storages().is_empty()),
            local: [x, y, z].map(|coordinate| coordinate.rem_euclid(16) as u8),
            air,
        })
    }
}
