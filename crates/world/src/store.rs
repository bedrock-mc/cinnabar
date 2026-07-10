use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::Arc,
};

use crate::{Chunk, ChunkKey, DecodeError, SubChunk, SubChunkKey};

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
        if sub_chunk_count > MAX_LEVEL_SUBCHUNKS {
            return Err(DecodeError::TooManySubChunks {
                count: sub_chunk_count,
                max: MAX_LEVEL_SUBCHUNKS,
            });
        }

        let old = self.chunks.get(&key);
        let mut decoded = BTreeMap::new();
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
                let sub_chunk = old
                    .and_then(|chunk| chunk.sub_chunks.get(&expected_y))
                    .filter(|previous| previous.as_ref() == &sub_chunk)
                    .cloned()
                    .unwrap_or_else(|| Arc::new(sub_chunk));
                decoded.insert(expected_y, sub_chunk);
            }
        }

        let ys = old
            .into_iter()
            .flat_map(|chunk| chunk.sub_chunks.keys().copied())
            .chain(decoded.keys().copied())
            .collect::<BTreeSet<_>>();
        let changed = ys
            .into_iter()
            .filter(|y| {
                let previous = old.and_then(|chunk| chunk.sub_chunks.get(y));
                let replacement = decoded.get(y);
                match (previous, replacement) {
                    (Some(previous), Some(replacement)) => {
                        previous.as_ref() != replacement.as_ref()
                    }
                    (None, None) => false,
                    _ => true,
                }
            })
            .map(|y| SubChunkKey::from_chunk(key, y))
            .collect::<Vec<_>>();
        let dirty = expand_mesh_dependents(changed);

        if decoded.is_empty() {
            self.chunks.remove(&key);
        } else {
            self.chunks.insert(
                key,
                Chunk {
                    sub_chunks: decoded,
                },
            );
        }
        Ok(ApplyLevelChunk {
            dirty,
            bytes_consumed: consumed,
        })
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
