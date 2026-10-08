//! Allocation-free primary-layer lookup, separate from all-layer collision queries.

use super::{PaletteWorld, SubChunkKey, WorldQueryError};

impl PaletteWorld<'_> {
    /// The first-layer runtime id of a loaded block.
    pub fn primary_runtime_id(&self, [x, y, z]: [i32; 3]) -> Result<u32, WorldQueryError> {
        let key = SubChunkKey::new(self.dimension, x >> 4, y >> 4, z >> 4);
        if !self.store.is_sub_chunk_loaded(key) {
            return Err(WorldQueryError::UnloadedChunk(key.chunk()));
        }
        let Some(sub_chunk) = self.store.sub_chunk(key) else {
            return Ok(self.registry.air_runtime_id);
        };
        if sub_chunk.storages().is_empty() {
            return Ok(self.registry.air_runtime_id);
        }
        Ok(sub_chunk
            .runtime_id(
                0,
                x.rem_euclid(16) as u8,
                y.rem_euclid(16) as u8,
                z.rem_euclid(16) as u8,
            )
            .expect("validated palette storage resolves every local coordinate"))
    }
}
