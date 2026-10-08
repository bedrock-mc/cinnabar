use super::WorldAuthority;
use std::collections::BTreeSet;
use world::*;

impl WorldAuthority {
    /// Clears a decoded all-air slot and returns its changed key.
    pub fn apply_all_air(
        &mut self,
        key: SubChunkKey,
    ) -> Result<Option<SubChunkKey>, CollisionRevisionError> {
        self.terrain.apply_all_air(key)
    }
    /// Applies authoritative air for a slot omitted by request-mode terrain.
    pub fn apply_request_mode_air(
        &mut self,
        key: SubChunkKey,
    ) -> Result<Option<SubChunkKey>, CollisionRevisionError> {
        self.terrain.apply_request_mode_air(key)
    }
    /// Replaces a biome column and returns affected subchunks.
    pub fn commit_biome_column(
        &mut self,
        key: ChunkKey,
        replacement: DecodedBiomeColumn,
    ) -> Vec<SubChunkKey> {
        self.terrain.commit_biome_column(key, replacement)
    }
    /// Applies decoded block-entity data and reports whether it changed.
    pub fn commit_block_entity_update(
        &mut self,
        key: BlockEntityKey,
        nbt: BlockEntityNbt,
    ) -> Result<bool, BlockEntityError> {
        self.terrain.commit_block_entity_update(key, nbt)
    }
    /// Replaces the block entities decoded with a terrain column.
    pub fn commit_chunk_block_entities(
        &mut self,
        key: ChunkKey,
        replacement: DecodedBlockEntities,
    ) {
        self.terrain.commit_chunk_block_entities(key, replacement)
    }
    /// Commits one decoded subchunk with its collision revision checks.
    pub fn commit_decoded_sub_chunk(
        &mut self,
        key: SubChunkKey,
        decoded: DecodedSubChunk,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        self.terrain.commit_decoded_sub_chunk(key, decoded)
    }
    /// Commits a decoded column and returns its changed and dirty subchunks.
    pub fn commit_level_chunk(
        &mut self,
        key: ChunkKey,
        decoded: DecodedLevelChunk,
    ) -> Result<ApplyLevelChunk, DecodeError> {
        self.terrain.commit_level_chunk(key, decoded)
    }
    /// Atomically applies prepared block mutations and returns changed subchunks.
    pub fn commit_prepared_block_updates(
        &mut self,
        prepared: Vec<PreparedSubChunkMutation>,
    ) -> Result<Vec<SubChunkKey>, MutationError> {
        self.terrain.commit_prepared_block_updates(prepared)
    }
    /// Commits a packed subchunk and returns its changed key.
    pub fn commit_sub_chunk(
        &mut self,
        key: SubChunkKey,
        decoded: SubChunk,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        self.terrain.commit_sub_chunk(key, decoded)
    }
    /// Removes retained columns and returns their keys and owned storage.
    pub fn detach_chunks(&mut self, keys: &BTreeSet<ChunkKey>) -> (Vec<SubChunkKey>, Vec<Chunk>) {
        self.terrain.detach_chunks(keys)
    }
    /// Records that a column is loaded, updating collision revisions.
    pub fn mark_chunk_loaded(&mut self, key: ChunkKey) -> Result<bool, CollisionRevisionError> {
        self.terrain.mark_chunk_loaded(key)
    }
    /// Records that a subchunk is loaded, updating collision revisions.
    pub fn mark_sub_chunk_loaded(
        &mut self,
        key: SubChunkKey,
    ) -> Result<bool, CollisionRevisionError> {
        self.terrain.mark_sub_chunk_loaded(key)
    }
    /// Applies a synchronous block prediction with collision revision checks.
    pub fn update_block(
        &mut self,
        key: SubChunkKey,
        update: BlockUpdate,
        air_runtime_id: u32,
    ) -> Result<Option<SubChunkKey>, MutationError> {
        self.terrain.update_block(key, update, air_runtime_id)
    }
}
