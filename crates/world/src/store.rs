use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex, Weak},
};

use crate::{
    BiomeStorage, BlockEntityError, BlockEntityKey, BlockEntityNbt, BlockIds, BlockUpdate, Chunk,
    ChunkKey, CollisionRevisionError, DecodeError, DecodedBiomeColumn, DecodedBlockEntities,
    DecodedSubChunk, MAX_BLOCK_ENTITIES_PER_CHUNK, MAX_BLOCK_ENTITY_BYTES_PER_CHUNK, MutationError,
    SubChunk, SubChunkKey,
    collision_revision::{CollisionRevisionAllocator, process_collision_revisions},
};

mod collision_snapshot;
mod helpers;
mod level_chunk;
use self::helpers::*;
pub use self::level_chunk::{DecodedLevelChunk, DimensionSlots, decode_column_tail};

/// Monotonic identity for the collision-relevant contents of one loaded column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkCollisionRevision {
    pub chunk: ChunkKey,
    pub revision: u64,
}

/// Result of atomically replacing a full inline chunk column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyLevelChunk {
    /// Exact changed source keys before mesh-dependency expansion, sorted and
    /// deduplicated for dependency-aware invalidation by the app.
    pub changed: Vec<SubChunkKey>,
    /// Changed keys plus their mesh dependents, deduplicated and sorted. Block
    /// changes include face-adjacent consumers; biome changes additionally
    /// include their same-Y horizontal 3x3 blend consumers. These keys are
    /// ready to enqueue without another expansion.
    pub dirty: Vec<SubChunkKey>,
    /// Bytes occupied by block sub-chunks before biome/border/entity data.
    pub bytes_consumed: usize,
    /// Bytes occupied by block sub-chunks before the biome prefix.
    pub block_bytes_consumed: usize,
}

/// A fully validated packed sub-chunk replacement prepared off the gameplay
/// thread and ready for an infallible FIFO commit.
#[derive(Debug)]
pub struct PreparedSubChunkMutation {
    key: SubChunkKey,
    replacement: Option<SubChunk>,
    changed: bool,
}

impl PreparedSubChunkMutation {
    #[must_use]
    pub const fn key(&self) -> SubChunkKey {
        self.key
    }

    #[must_use]
    pub const fn replacement(&self) -> Option<&SubChunk> {
        self.replacement.as_ref()
    }

    #[must_use]
    pub const fn changed(&self) -> bool {
        self.changed
    }
}

/// Sparse client-side chunk store.
#[derive(Debug)]
pub struct ChunkStore {
    chunks: HashMap<ChunkKey, Chunk>,
    loaded_chunks: BTreeSet<ChunkKey>,
    authoritative_sub_chunks: HashMap<ChunkKey, BTreeSet<i32>>,
    collision_revisions: HashMap<ChunkKey, ChunkCollisionRevision>,
    collision_revision_allocator: Arc<CollisionRevisionAllocator>,
    collision_snapshot: Mutex<Weak<Self>>,
}

impl Default for ChunkStore {
    fn default() -> Self {
        Self {
            chunks: HashMap::new(),
            loaded_chunks: BTreeSet::new(),
            authoritative_sub_chunks: HashMap::new(),
            collision_revisions: HashMap::new(),
            collision_revision_allocator: process_collision_revisions(),
            collision_snapshot: Mutex::default(),
        }
    }
}

impl ChunkStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a chunk column, if block data has been received for it.
    #[must_use]
    pub fn chunk(&self, key: ChunkKey) -> Option<&Chunk> {
        self.chunks.get(&key)
    }

    /// Includes authoritative all-air columns that allocate no sparse storage.
    #[must_use]
    pub fn contains_column(&self, key: ChunkKey) -> bool {
        self.loaded_chunks.contains(&key)
            || self.chunks.contains_key(&key)
            || self.authoritative_sub_chunks.contains_key(&key)
    }

    /// Returns whether a complete LevelChunk for this column has been
    /// committed and not subsequently evicted.
    ///
    /// This is deliberately independent of [`Self::chunk`]: an all-air
    /// column remains absent from the sparse palette store while still being
    /// known world data for fail-closed collision simulation.
    #[must_use]
    pub fn is_chunk_loaded(&self, key: ChunkKey) -> bool {
        self.loaded_chunks.contains(&key)
    }

    /// Returns whether this exact 16³ block scope has authoritative data.
    ///
    /// Inline LevelChunk columns cover every Y. Request-mode columns become
    /// usable one successful SubChunk response at a time, including all-air
    /// responses that intentionally allocate no sparse palette storage.
    #[must_use]
    pub fn is_sub_chunk_loaded(&self, key: SubChunkKey) -> bool {
        self.loaded_chunks.contains(&key.chunk())
            || self
                .authoritative_sub_chunks
                .get(&key.chunk())
                .is_some_and(|ys| ys.contains(&key.y))
    }

    /// Returns the collision identity for a currently loaded column.
    #[must_use]
    pub fn collision_revision(&self, key: ChunkKey) -> Option<ChunkCollisionRevision> {
        self.collision_revisions.get(&key).copied()
    }

    /// Marks a request-mode column as completely known without allocating
    /// sparse block storage for an all-air result.
    pub fn mark_chunk_loaded(&mut self, key: ChunkKey) -> Result<bool, CollisionRevisionError> {
        if self.loaded_chunks.contains(&key) {
            return Ok(false);
        }
        let revision = self.collision_revision_allocator.allocate()?;
        self.loaded_chunks.insert(key);
        self.authoritative_sub_chunks.remove(&key);
        self.set_collision_revision(key, revision);
        Ok(true)
    }

    /// Marks one successful request-mode SubChunk response as authoritative.
    ///
    /// This is separate from sparse block storage so an all-air response still
    /// permits fail-closed collision queries within that exact 16³ scope.
    pub fn mark_sub_chunk_loaded(
        &mut self,
        key: SubChunkKey,
    ) -> Result<bool, CollisionRevisionError> {
        if self.is_sub_chunk_loaded(key) {
            return Ok(false);
        }
        let revision = self.collision_revision_allocator.allocate()?;
        self.authoritative_sub_chunks
            .entry(key.chunk())
            .or_default()
            .insert(key.y);
        self.set_collision_revision(key.chunk(), revision);
        Ok(true)
    }

    /// Checks residency without acquiring a reference-counted section handle.
    pub fn contains_sub_chunk(&self, key: SubChunkKey) -> bool {
        self.chunks
            .get(&key.chunk())
            .is_some_and(|chunk| chunk.sub_chunks.contains_key(&key.y))
    }

    /// Returns an `Arc` snapshot suitable for handing to a mesh worker.
    #[must_use]
    pub fn sub_chunk(&self, key: SubChunkKey) -> Option<Arc<SubChunk>> {
        self.chunk(key.chunk())?.sub_chunk(key.y)
    }

    /// Returns a palette-native biome storage snapshot for a mesh worker.
    #[must_use]
    pub fn biome_storage(&self, key: SubChunkKey) -> Option<Arc<BiomeStorage>> {
        self.chunk(key.chunk())?.biome_storage(key.y)
    }

    /// Looks up one raw biome ID without expanding its packed storage.
    #[must_use]
    pub fn biome_id(&self, key: SubChunkKey, x: u8, y: u8, z: u8) -> Option<u32> {
        self.biome_storage(key)?.biome_id(x, y, z)
    }

    /// Returns one immutable sparse block-entity snapshot.
    #[must_use]
    pub fn block_entity(&self, key: BlockEntityKey) -> Option<Arc<BlockEntityNbt>> {
        self.chunk(key.chunk())?.block_entity(key)
    }

    /// Decodes and applies one successful SubChunk response payload. The
    /// returned key identifies the changed storage; call
    /// [`SubChunkKey::mesh_dependents`] before scheduling culling meshes.
    pub fn apply_sub_chunk(
        &mut self,
        key: SubChunkKey,
        payload: &[u8],
        ids: &dyn BlockIds,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        self.commit_sub_chunk(key, SubChunk::decode(payload, ids))
    }

    /// Commits a previously decoded SubChunk response at the requested Y,
    /// which vanilla uses whatever the payload's Y byte says.
    pub fn commit_sub_chunk(
        &mut self,
        key: SubChunkKey,
        decoded: SubChunk,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        if decoded.has_no_storages() {
            return self.remove_sub_chunk(key).map_err(Into::into);
        }

        if self
            .chunks
            .get(&key.chunk())
            .and_then(|chunk| chunk.sub_chunks.get(&key.y))
            .is_some_and(|current| current.as_ref() == &decoded)
        {
            return Ok(None);
        }
        let revision = self.reserve_loaded_change(key)?;
        self.invalidate_collision_snapshot();
        Arc::make_mut(&mut self.chunks.entry(key.chunk()).or_default().sub_chunks)
            .insert(key.y, Arc::new(decoded));
        self.apply_reserved_revision(key.chunk(), revision);
        Ok(Some(key))
    }

    /// Atomically commits one worker-decoded SubChunk block prefix and replaces
    /// only the sparse block entities belonging to that exact 16³ scope.
    pub fn commit_decoded_sub_chunk(
        &mut self,
        key: SubChunkKey,
        decoded: DecodedSubChunk,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        let (sub_chunk, block_entities) = decoded.into_parts();
        let replacement_bytes =
            self.validate_sub_chunk_block_entity_replacement(key, &block_entities)?;
        let changed = self.commit_sub_chunk(key, sub_chunk)?;
        self.replace_sub_chunk_block_entities(key, block_entities, replacement_bytes);
        Ok(changed)
    }

    /// Atomically upserts one packet-56 block entity without touching packed
    /// block or biome state. Equal snapshots retain the existing `Arc`.
    pub fn commit_block_entity_update(
        &mut self,
        key: BlockEntityKey,
        nbt: BlockEntityNbt,
    ) -> Result<bool, BlockEntityError> {
        if let Some(actual) = nbt.embedded_position()
            && actual != key.position()
        {
            return Err(BlockEntityError::PositionMismatch {
                expected: key.position(),
                actual,
            });
        }
        if self
            .block_entity(key)
            .is_some_and(|current| current.as_ref() == &nbt)
        {
            return Ok(false);
        }
        let previous = self.chunks.get(&key.chunk());
        let replacing = previous.and_then(|chunk| chunk.block_entities.get(&key));
        let previous_count = previous.map_or(0, |chunk| chunk.block_entities.len());
        if replacing.is_none() && previous_count == MAX_BLOCK_ENTITIES_PER_CHUNK {
            return Err(BlockEntityError::TooManyEntities {
                max: MAX_BLOCK_ENTITIES_PER_CHUNK,
            });
        }
        let previous_bytes = replacing.map_or(0, |previous| previous.bytes().len());
        let retained_bytes = previous
            .map_or(0, |chunk| chunk.block_entity_bytes)
            .checked_sub(previous_bytes)
            .expect("stored block-entity byte total matches its sparse map");
        let replacement_bytes = retained_bytes.saturating_add(nbt.bytes().len());
        ensure_chunk_block_entity_bytes(replacement_bytes)?;
        let chunk = self.chunks.entry(key.chunk()).or_default();
        Arc::make_mut(&mut chunk.block_entities).insert(key, Arc::new(nbt));
        chunk.block_entity_bytes = replacement_bytes;
        Ok(true)
    }

    /// Atomically replaces the complete sparse block-entity map for a chunk.
    /// Equal records retain their previous `Arc` snapshots.
    pub fn commit_chunk_block_entities(
        &mut self,
        key: ChunkKey,
        replacement: DecodedBlockEntities,
    ) {
        let mut replacement = replacement.into_entities();
        if let Some(previous) = self.chunks.get(&key) {
            for (&entity_key, entity) in &mut replacement {
                if let Some(previous) = previous
                    .block_entities
                    .get(&entity_key)
                    .filter(|previous| previous.as_ref() == entity.as_ref())
                {
                    *entity = Arc::clone(previous);
                }
            }
        }
        if replacement.is_empty() && !self.chunks.contains_key(&key) {
            return;
        }
        let chunk = self.chunks.entry(key).or_default();
        chunk.block_entity_bytes = replacement
            .values()
            .map(|entity| entity.bytes().len())
            .sum();
        chunk.block_entities = Arc::new(replacement);
        if chunk.sub_chunks.is_empty() && chunk.biomes.is_none() && chunk.block_entities.is_empty()
        {
            self.chunks.remove(&key);
        }
    }

    /// Applies a successful-all-air SubChunk response without allocating a
    /// 4,096-block representation.
    ///
    /// As with [`Self::apply_sub_chunk`], the returned changed key must be
    /// expanded with [`SubChunkKey::mesh_dependents`] before scheduling meshes.
    pub fn apply_all_air(
        &mut self,
        key: SubChunkKey,
    ) -> Result<Option<SubChunkKey>, CollisionRevisionError> {
        let changed = self.remove_sub_chunk(key)?;
        self.clear_sub_chunk_block_entities(key);
        Ok(changed)
    }

    /// Removes stored block data for authoritative request-mode air while
    /// preserving the independently supplied LevelChunk block-entity map.
    pub fn apply_request_mode_air(
        &mut self,
        key: SubChunkKey,
    ) -> Result<Option<SubChunkKey>, CollisionRevisionError> {
        self.remove_sub_chunk(key)
    }

    /// Applies one `UpdateBlock`-style change without expanding block data.
    ///
    /// `air_runtime_id` comes from the active registry because raw IDs may be
    /// sequential runtime IDs or network hashes. Callers expand a returned key
    /// through [`SubChunkKey::mesh_dependents`] before remeshing.
    pub fn update_block(
        &mut self,
        key: SubChunkKey,
        update: BlockUpdate,
        air_runtime_id: u32,
    ) -> Result<Option<SubChunkKey>, MutationError> {
        Ok(self
            .update_sub_chunk_blocks(key, std::slice::from_ref(&update), air_runtime_id)?
            .into_iter()
            .next())
    }

    /// Atomically applies an `UpdateSubChunkBlocks`-style batch.
    ///
    /// Every entry is validated before the stored `Arc` is replaced. Packed
    /// words are repacked directly; no flat 4,096-block array is created.
    pub fn update_sub_chunk_blocks(
        &mut self,
        key: SubChunkKey,
        updates: &[BlockUpdate],
        air_runtime_id: u32,
    ) -> Result<Vec<SubChunkKey>, MutationError> {
        let previous = self.sub_chunk(key);
        let prepared =
            Self::prepare_sub_chunk_blocks(key, previous.as_deref(), updates, air_runtime_id)?;
        self.commit_prepared_block_updates(vec![prepared])
    }

    /// Builds one packed replacement without mutating a store. Duplicate
    /// coordinates use their final value and each affected layer repacks once.
    pub fn prepare_sub_chunk_blocks(
        key: SubChunkKey,
        previous: Option<&SubChunk>,
        updates: &[BlockUpdate],
        air_runtime_id: u32,
    ) -> Result<PreparedSubChunkMutation, MutationError> {
        for &update in updates {
            update.validate()?;
        }
        if updates.is_empty() {
            return Ok(PreparedSubChunkMutation {
                key,
                replacement: previous.cloned(),
                changed: false,
            });
        }

        let mut replacement = previous
            .cloned()
            .unwrap_or_else(|| SubChunk::for_block_updates(key.y));
        replacement.apply_block_updates(updates, air_runtime_id);
        let replacement = (!replacement.has_no_storages()).then_some(replacement);
        let changed = match (previous, replacement.as_ref()) {
            (Some(previous), Some(replacement)) => previous != replacement,
            (None, None) => false,
            (Some(_), None) | (None, Some(_)) => true,
        };
        Ok(PreparedSubChunkMutation {
            key,
            replacement,
            changed,
        })
    }

    /// Commits a completely prepared batch without validation or allocation
    /// failure points, so observers can only see the state before or after it.
    pub fn commit_prepared_block_updates(
        &mut self,
        prepared: Vec<PreparedSubChunkMutation>,
    ) -> Result<Vec<SubChunkKey>, MutationError> {
        let changed_columns = prepared
            .iter()
            .filter(|mutation| mutation.changed && self.is_sub_chunk_loaded(mutation.key))
            .map(|mutation| mutation.key.chunk())
            .collect::<BTreeSet<_>>();
        let allocated = self
            .collision_revision_allocator
            .allocate_batch(changed_columns.len())?;
        let revisions = changed_columns
            .into_iter()
            .zip(allocated)
            .collect::<HashMap<_, _>>();
        let mut changed = Vec::new();
        for mutation in prepared {
            if !mutation.changed {
                continue;
            }
            match mutation.replacement {
                Some(replacement) => {
                    Arc::make_mut(
                        &mut self
                            .chunks
                            .entry(mutation.key.chunk())
                            .or_default()
                            .sub_chunks,
                    )
                    .insert(mutation.key.y, Arc::new(replacement));
                }
                None => {
                    self.remove_sub_chunk_without_revision(mutation.key);
                }
            }
            changed.push(mutation.key);
        }
        if !changed.is_empty() {
            self.invalidate_collision_snapshot();
        }
        for (chunk, revision) in revisions {
            self.set_collision_revision(chunk, revision);
        }
        Ok(changed)
    }

    /// Removes a complete column and returns its stored sub-chunk keys sorted
    /// by Y. External `Arc<SubChunk>` snapshots remain valid.
    pub fn evict_chunk(&mut self, key: ChunkKey) -> Vec<SubChunkKey> {
        self.detach_chunks(&BTreeSet::from([key])).0
    }

    /// Removes column authority together and returns owned data for deferred destruction.
    pub fn detach_chunks(&mut self, keys: &BTreeSet<ChunkKey>) -> (Vec<SubChunkKey>, Vec<Chunk>) {
        let mut removed = Vec::new();
        let mut retired = Vec::new();
        if !keys.is_empty() {
            self.invalidate_collision_snapshot();
        }
        for &key in keys {
            self.authoritative_sub_chunks.remove(&key);
            self.loaded_chunks.remove(&key);
            self.collision_revisions.remove(&key);
            if let Some(chunk) = self.chunks.remove(&key) {
                removed.extend(
                    chunk
                        .sub_chunks
                        .keys()
                        .map(|&y| SubChunkKey::from_chunk(key, y)),
                );
                retired.push(chunk);
            }
        }
        (removed, retired)
    }

    fn remove_sub_chunk(
        &mut self,
        key: SubChunkKey,
    ) -> Result<Option<SubChunkKey>, CollisionRevisionError> {
        let present = self
            .chunks
            .get(&key.chunk())
            .is_some_and(|chunk| chunk.sub_chunks.contains_key(&key.y));
        if !present {
            return Ok(None);
        }
        let revision = self.reserve_loaded_change(key)?;
        let removed = self.remove_sub_chunk_without_revision(key);
        self.apply_reserved_revision(key.chunk(), revision);
        Ok(removed)
    }

    fn remove_sub_chunk_without_revision(&mut self, key: SubChunkKey) -> Option<SubChunkKey> {
        self.invalidate_collision_snapshot();
        let chunk_key = key.chunk();
        let chunk = self.chunks.get_mut(&chunk_key)?;
        let removed = Arc::make_mut(&mut chunk.sub_chunks)
            .remove(&key.y)
            .is_some();
        if chunk.sub_chunks.is_empty() && chunk.biomes.is_none() && chunk.block_entities.is_empty()
        {
            self.chunks.remove(&chunk_key);
        }
        removed.then_some(key)
    }

    fn reserve_loaded_change(
        &self,
        key: SubChunkKey,
    ) -> Result<Option<u64>, CollisionRevisionError> {
        self.is_sub_chunk_loaded(key)
            .then(|| self.collision_revision_allocator.allocate())
            .transpose()
    }

    fn apply_reserved_revision(&mut self, key: ChunkKey, revision: Option<u64>) {
        if let Some(revision) = revision {
            self.set_collision_revision(key, revision);
        }
    }

    fn set_collision_revision(&mut self, key: ChunkKey, revision: u64) {
        self.invalidate_collision_snapshot();
        self.collision_revisions.insert(
            key,
            ChunkCollisionRevision {
                chunk: key,
                revision,
            },
        );
    }

    fn replace_sub_chunk_block_entities(
        &mut self,
        key: SubChunkKey,
        replacement: DecodedBlockEntities,
        replacement_bytes: usize,
    ) {
        let mut replacement = replacement.into_entities();
        if let Some(previous) = self.chunks.get(&key.chunk()) {
            for (&entity_key, entity) in &mut replacement {
                if let Some(previous) = previous
                    .block_entities
                    .get(&entity_key)
                    .filter(|previous| previous.as_ref() == entity.as_ref())
                {
                    *entity = Arc::clone(previous);
                }
            }
        }

        let has_previous = self.chunks.get(&key.chunk()).is_some_and(|chunk| {
            chunk
                .block_entities
                .keys()
                .any(|entity| entity.sub_chunk() == key)
        });
        if replacement.is_empty() && !has_previous {
            return;
        }
        let chunk = self.chunks.entry(key.chunk()).or_default();
        let entities = Arc::make_mut(&mut chunk.block_entities);
        entities.retain(|entity, _| entity.sub_chunk() != key);
        entities.extend(replacement);
        chunk.block_entity_bytes = replacement_bytes;
        if chunk.sub_chunks.is_empty() && chunk.biomes.is_none() && chunk.block_entities.is_empty()
        {
            self.chunks.remove(&key.chunk());
        }
    }

    fn validate_sub_chunk_block_entity_replacement(
        &self,
        key: SubChunkKey,
        replacement: &DecodedBlockEntities,
    ) -> Result<usize, BlockEntityError> {
        let previous = self.chunks.get(&key.chunk());
        let (previous_count, previous_bytes) = previous.map_or((0, 0), |chunk| {
            (chunk.block_entities.len(), chunk.block_entity_bytes)
        });
        let (removed_count, removed_bytes) = previous.map_or((0, 0), |chunk| {
            chunk
                .block_entities
                .iter()
                .filter(|(entity_key, _)| entity_key.sub_chunk() == key)
                .fold((0_usize, 0_usize), |(count, bytes), (_, entity)| {
                    (count + 1, bytes + entity.bytes().len())
                })
        });
        let replacement_count = replacement.len();
        let proposed_count = previous_count - removed_count + replacement_count;
        if proposed_count > MAX_BLOCK_ENTITIES_PER_CHUNK {
            return Err(BlockEntityError::TooManyEntities {
                max: MAX_BLOCK_ENTITIES_PER_CHUNK,
            });
        }
        let replacement_bytes = replacement.entities().fold(0_usize, |bytes, (_, nbt)| {
            bytes.saturating_add(nbt.bytes().len())
        });
        let proposed_bytes = previous_bytes
            .checked_sub(removed_bytes)
            .expect("stored block-entity byte total matches its sparse map")
            .saturating_add(replacement_bytes);
        ensure_chunk_block_entity_bytes(proposed_bytes)?;
        Ok(proposed_bytes)
    }

    fn clear_sub_chunk_block_entities(&mut self, key: SubChunkKey) {
        let Some(chunk) = self.chunks.get_mut(&key.chunk()) else {
            return;
        };
        let mut removed_bytes = 0_usize;
        Arc::make_mut(&mut chunk.block_entities).retain(|entity, nbt| {
            let retain = entity.sub_chunk() != key;
            if !retain {
                removed_bytes = removed_bytes.saturating_add(nbt.bytes().len());
            }
            retain
        });
        chunk.block_entity_bytes = chunk
            .block_entity_bytes
            .checked_sub(removed_bytes)
            .expect("stored block-entity byte total matches its sparse map");
        if chunk.sub_chunks.is_empty() && chunk.biomes.is_none() && chunk.block_entities.is_empty()
        {
            self.chunks.remove(&key.chunk());
        }
    }

    /// Decodes an inline LevelChunk's block sub-chunks and swaps the column.
    pub fn apply_level_chunk(
        &mut self,
        key: ChunkKey,
        first_sub_chunk_y: i32,
        sub_chunk_count: usize,
        payload: &[u8],
        ids: &dyn BlockIds,
    ) -> Result<ApplyLevelChunk, DecodeError> {
        let decoded = DecodedLevelChunk::decode(first_sub_chunk_y, sub_chunk_count, payload, ids);
        self.commit_level_chunk(key, decoded)
    }

    /// Atomically replaces only the dense biome column received by a
    /// request-mode LevelChunk and returns affected mesh dependents.
    pub fn commit_biome_column(
        &mut self,
        key: ChunkKey,
        mut replacement: DecodedBiomeColumn,
    ) -> Vec<SubChunkKey> {
        let previous = self
            .chunks
            .get(&key)
            .and_then(|chunk| chunk.biomes.as_ref());
        reuse_equal_biome_arcs(&mut replacement, previous);
        let changed = changed_biome_ys(previous, Some(&replacement))
            .into_iter()
            .map(|y| SubChunkKey::from_chunk(key, y))
            .collect();
        self.chunks.entry(key).or_default().biomes = Some(replacement);
        expand_biome_mesh_dependents(changed)
    }

    /// Returns whether a request-mode biome replacement is byte-for-byte
    /// equivalent to the currently retained dense column.
    #[must_use]
    pub fn biome_column_matches(&self, key: ChunkKey, replacement: &DecodedBiomeColumn) -> bool {
        self.chunks
            .get(&key)
            .and_then(|chunk| chunk.biomes.as_ref())
            == Some(replacement)
    }

    /// Atomically swaps a fully worker-decoded column into the store.
    pub fn commit_level_chunk(
        &mut self,
        key: ChunkKey,
        decoded: DecodedLevelChunk,
    ) -> Result<ApplyLevelChunk, DecodeError> {
        let newly_loaded = !self.loaded_chunks.contains(&key);
        let old = self.chunks.get(&key);
        let DecodedLevelChunk {
            mut sub_chunks,
            mut biomes,
            block_entities,
            block_bytes_consumed,
            bytes_consumed,
        } = decoded;
        for (&y, replacement) in &mut sub_chunks {
            if let Some(previous) = old
                .and_then(|chunk| chunk.sub_chunks.get(&y))
                .filter(|previous| previous.as_ref() == replacement.as_ref())
            {
                *replacement = Arc::clone(previous);
            }
        }
        if biomes.is_none() {
            biomes = old.and_then(|chunk| chunk.biomes.clone());
        } else if let Some(replacement) = biomes.as_mut() {
            reuse_equal_biome_arcs(replacement, old.and_then(|chunk| chunk.biomes.as_ref()));
        }
        let mut block_entities = match block_entities {
            Some(replacement) => replacement.into_entities(),
            None => old
                .map(|chunk| std::collections::BTreeMap::clone(&chunk.block_entities))
                .unwrap_or_default(),
        };
        if let Some(previous) = old {
            for (&entity_key, replacement) in &mut block_entities {
                if let Some(previous) = previous
                    .block_entities
                    .get(&entity_key)
                    .filter(|previous| previous.as_ref() == replacement.as_ref())
                {
                    *replacement = Arc::clone(previous);
                }
            }
        }

        let ys = old
            .into_iter()
            .flat_map(|chunk| chunk.sub_chunks.keys().copied())
            .chain(sub_chunks.keys().copied())
            .collect::<BTreeSet<_>>();
        let block_changed = ys
            .into_iter()
            .filter(|y| {
                let previous = old.and_then(|chunk| chunk.sub_chunks.get(y));
                let replacement = sub_chunks.get(y);
                match (previous, replacement) {
                    (Some(previous), Some(replacement)) => !Arc::ptr_eq(previous, replacement),
                    (None, None) => false,
                    _ => true,
                }
            })
            .map(|y| SubChunkKey::from_chunk(key, y))
            .collect::<BTreeSet<_>>();
        let biome_changed =
            changed_biome_ys(old.and_then(|chunk| chunk.biomes.as_ref()), biomes.as_ref())
                .into_iter()
                .map(|y| SubChunkKey::from_chunk(key, y))
                .collect::<Vec<_>>();
        let collision_changed = !block_changed.is_empty();
        let revision = (newly_loaded || collision_changed)
            .then(|| self.collision_revision_allocator.allocate())
            .transpose()?;
        let mut changed = block_changed;
        changed.extend(biome_changed.iter().copied());
        let changed = changed.into_iter().collect::<Vec<_>>();
        let dirty = expand_mesh_dependents(changed.clone())
            .into_iter()
            .chain(expand_biome_mesh_dependents(biome_changed))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();

        if sub_chunks.is_empty() && biomes.is_none() && block_entities.is_empty() {
            self.chunks.remove(&key);
        } else {
            self.chunks.insert(
                key,
                Chunk {
                    sub_chunks: if collision_changed {
                        Arc::new(sub_chunks)
                    } else {
                        old.map_or_else(
                            || Arc::new(sub_chunks),
                            |chunk| Arc::clone(&chunk.sub_chunks),
                        )
                    },
                    biomes,
                    block_entity_bytes: block_entities
                        .values()
                        .map(|entity| entity.bytes().len())
                        .sum(),
                    block_entities: Arc::new(block_entities),
                },
            );
        }
        if newly_loaded {
            self.loaded_chunks.insert(key);
            self.authoritative_sub_chunks.remove(&key);
        }
        self.apply_reserved_revision(key, revision);
        Ok(ApplyLevelChunk {
            changed,
            dirty,
            bytes_consumed,
            block_bytes_consumed,
        })
    }
}

#[cfg(test)]
#[path = "store_revision_tests.rs"]
mod collision_revision_allocator_tests;
