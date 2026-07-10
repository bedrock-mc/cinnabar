use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use crate::{BlockUpdate, Chunk, ChunkKey, DecodeError, MutationError, SubChunk, SubChunkKey};

/// Maximum sub-chunks accepted in one full inline LevelChunk payload.
pub const MAX_LEVEL_SUBCHUNKS: usize = 64;

/// Result of atomically replacing a full inline chunk column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyLevelChunk {
    /// Changed keys plus their face-adjacent mesh dependents, deduplicated and
    /// sorted. These keys are ready to enqueue without another expansion.
    pub dirty: Vec<SubChunkKey>,
    /// Bytes occupied by block sub-chunks before biome/border/entity data.
    pub bytes_consumed: usize,
}

/// A fully validated packed sub-chunk replacement prepared off the gameplay
/// thread and ready for an infallible FIFO commit.
#[derive(Debug)]
pub struct PreparedSubChunkMutation {
    key: SubChunkKey,
    replacement: Option<SubChunk>,
    changed: bool,
}

/// A completely validated full-column block decode ready for a cheap commit.
///
/// Packed sub-chunks are wrapped in `Arc`s during decode so this value can be
/// produced on a worker and transferred to the main thread without copying
/// chunk data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedLevelChunk {
    sub_chunks: BTreeMap<i32, Arc<SubChunk>>,
    bytes_consumed: usize,
}

impl DecodedLevelChunk {
    /// Purely decodes and validates every block sub-chunk in a LevelChunk
    /// prefix. No [`ChunkStore`] is touched if any later sub-chunk is malformed.
    pub fn decode(
        first_sub_chunk_y: i32,
        sub_chunk_count: usize,
        payload: &[u8],
    ) -> Result<Self, DecodeError> {
        if sub_chunk_count > MAX_LEVEL_SUBCHUNKS {
            return Err(DecodeError::TooManySubChunks {
                count: sub_chunk_count,
                max: MAX_LEVEL_SUBCHUNKS,
            });
        }

        let mut sub_chunks = BTreeMap::new();
        let mut consumed = 0;
        for offset in 0..sub_chunk_count {
            let offset_i32 = i32::try_from(offset).map_err(|_| DecodeError::SubChunkYOverflow {
                first: first_sub_chunk_y,
                offset,
            })?;
            let expected_y = first_sub_chunk_y.checked_add(offset_i32).ok_or(
                DecodeError::SubChunkYOverflow {
                    first: first_sub_chunk_y,
                    offset,
                },
            )?;
            let (sub_chunk, used) = SubChunk::decode_prefix(&payload[consumed..])?;
            if let Some(actual) = sub_chunk.y_index() {
                let actual = i32::from(actual);
                if actual != expected_y {
                    return Err(DecodeError::SubChunkIndexMismatch {
                        expected: expected_y,
                        actual,
                    });
                }
            }
            consumed += used;
            if !sub_chunk.has_no_storages() {
                sub_chunks.insert(expected_y, Arc::new(sub_chunk));
            }
        }
        Ok(Self {
            sub_chunks,
            bytes_consumed: consumed,
        })
    }

    #[must_use]
    pub fn bytes_consumed(&self) -> usize {
        self.bytes_consumed
    }

    /// Returns an immutable worker-produced snapshot for one Y index.
    #[must_use]
    pub fn sub_chunk(&self, y: i32) -> Option<Arc<SubChunk>> {
        self.sub_chunks.get(&y).cloned()
    }

    pub fn sub_chunks(&self) -> impl ExactSizeIterator<Item = (i32, Arc<SubChunk>)> + '_ {
        self.sub_chunks
            .iter()
            .map(|(&y, sub_chunk)| (y, Arc::clone(sub_chunk)))
    }
}

/// Sparse client-side chunk store.
#[derive(Debug, Default)]
pub struct ChunkStore {
    chunks: HashMap<ChunkKey, Chunk>,
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

    /// Returns an `Arc` snapshot suitable for handing to a mesh worker.
    #[must_use]
    pub fn sub_chunk(&self, key: SubChunkKey) -> Option<Arc<SubChunk>> {
        self.chunk(key.chunk())?.sub_chunk(key.y)
    }

    /// Prefix-decodes and applies one successful SubChunk response payload.
    ///
    /// Legitimate block-entity NBT may follow the serialized sub-chunk, so
    /// this ingestion path intentionally does not require exact EOF. The
    /// returned key identifies the changed storage; call
    /// [`SubChunkKey::mesh_dependents`] before scheduling culling meshes.
    pub fn apply_sub_chunk(
        &mut self,
        key: SubChunkKey,
        payload: &[u8],
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        let (decoded, _) = SubChunk::decode_prefix(payload)?;
        self.commit_sub_chunk(key, decoded)
    }

    /// Commits a previously decoded SubChunk response without decoding on the
    /// calling thread.
    pub fn commit_sub_chunk(
        &mut self,
        key: SubChunkKey,
        decoded: SubChunk,
    ) -> Result<Option<SubChunkKey>, DecodeError> {
        if let Some(actual) = decoded.y_index() {
            let actual = i32::from(actual);
            if actual != key.y {
                return Err(DecodeError::SubChunkIndexMismatch {
                    expected: key.y,
                    actual,
                });
            }
        }

        if decoded.has_no_storages() {
            return Ok(self.remove_sub_chunk(key));
        }

        let chunk = self.chunks.entry(key.chunk()).or_default();
        if chunk
            .sub_chunks
            .get(&key.y)
            .is_some_and(|current| current.as_ref() == &decoded)
        {
            return Ok(None);
        }
        chunk.sub_chunks.insert(key.y, Arc::new(decoded));
        Ok(Some(key))
    }

    /// Applies a successful-all-air SubChunk response without allocating a
    /// 4,096-block representation.
    ///
    /// As with [`Self::apply_sub_chunk`], the returned changed key must be
    /// expanded with [`SubChunkKey::mesh_dependents`] before scheduling meshes.
    pub fn apply_all_air(&mut self, key: SubChunkKey) -> Option<SubChunkKey> {
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
        Ok(self.commit_prepared_block_updates(vec![prepared]))
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
    ) -> Vec<SubChunkKey> {
        let mut changed = Vec::new();
        for mutation in prepared {
            if !mutation.changed {
                continue;
            }
            match mutation.replacement {
                Some(replacement) => {
                    self.chunks
                        .entry(mutation.key.chunk())
                        .or_default()
                        .sub_chunks
                        .insert(mutation.key.y, Arc::new(replacement));
                }
                None => {
                    self.remove_sub_chunk(mutation.key);
                }
            }
            changed.push(mutation.key);
        }
        changed
    }

    /// Removes a complete column and returns its stored sub-chunk keys sorted
    /// by Y. External `Arc<SubChunk>` snapshots remain valid.
    pub fn evict_chunk(&mut self, key: ChunkKey) -> Vec<SubChunkKey> {
        self.chunks
            .remove(&key)
            .into_iter()
            .flat_map(|chunk| chunk.sub_chunks.into_keys())
            .map(|y| SubChunkKey::from_chunk(key, y))
            .collect()
    }

    fn remove_sub_chunk(&mut self, key: SubChunkKey) -> Option<SubChunkKey> {
        let chunk_key = key.chunk();
        let chunk = self.chunks.get_mut(&chunk_key)?;
        let removed = chunk.sub_chunks.remove(&key.y).is_some();
        if chunk.sub_chunks.is_empty() {
            self.chunks.remove(&chunk_key);
        }
        removed.then_some(key)
    }

    /// Prefix-decodes all block sub-chunks in an inline LevelChunk and swaps
    /// the complete column only after every input has validated.
    pub fn apply_level_chunk(
        &mut self,
        key: ChunkKey,
        first_sub_chunk_y: i32,
        sub_chunk_count: usize,
        payload: &[u8],
    ) -> Result<ApplyLevelChunk, DecodeError> {
        let decoded = DecodedLevelChunk::decode(first_sub_chunk_y, sub_chunk_count, payload)?;
        Ok(self.commit_level_chunk(key, decoded))
    }

    /// Atomically swaps a fully worker-decoded column into the store.
    pub fn commit_level_chunk(
        &mut self,
        key: ChunkKey,
        decoded: DecodedLevelChunk,
    ) -> ApplyLevelChunk {
        let old = self.chunks.get(&key);
        let DecodedLevelChunk {
            mut sub_chunks,
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

        let ys = old
            .into_iter()
            .flat_map(|chunk| chunk.sub_chunks.keys().copied())
            .chain(sub_chunks.keys().copied())
            .collect::<BTreeSet<_>>();
        let changed = ys
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
            .collect::<Vec<_>>();
        let dirty = expand_mesh_dependents(changed);

        if sub_chunks.is_empty() {
            self.chunks.remove(&key);
        } else {
            self.chunks.insert(key, Chunk { sub_chunks });
        }
        ApplyLevelChunk {
            dirty,
            bytes_consumed,
        }
    }
}

fn expand_mesh_dependents(changed: Vec<SubChunkKey>) -> Vec<SubChunkKey> {
    changed
        .into_iter()
        .flat_map(SubChunkKey::mesh_dependents)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}
