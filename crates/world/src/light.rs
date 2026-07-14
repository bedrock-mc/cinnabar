use std::{collections::BTreeMap, sync::Arc};

use thiserror::Error;

use crate::{ChunkKey, SubChunkKey};

/// Number of light samples in one 16x16x16 sub-chunk.
pub const LIGHT_SAMPLES_PER_SUB_CHUNK: usize = 16 * 16 * 16;
const PACKED_LIGHT_BYTES: usize = LIGHT_SAMPLES_PER_SUB_CHUNK / 2;

/// Errors produced by bounded nibble-light storage operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum LightStorageError {
    #[error("light value {value} exceeds the four-bit maximum of 15")]
    ValueOutOfRange { value: u8 },
    #[error("light sample index {index} exceeds the sub-chunk maximum of 4095")]
    IndexOutOfRange { index: usize },
}

/// One allocation-free uniform or copy-on-write packed light channel.
///
/// Its representation is deliberately private so safe callers cannot forge
/// an out-of-range uniform value or a non-canonical all-equal packed payload.
///
/// ```compile_fail
/// use world::LightNibbleStorage;
/// let _ = LightNibbleStorage::Uniform(255);
/// ```
///
/// ```compile_fail
/// use std::sync::Arc;
/// use world::LightNibbleStorage;
/// let _ = LightNibbleStorage::Packed(Arc::new([0_u8; 2048]));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightNibbleStorage {
    representation: LightNibbleRepresentation,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LightNibbleRepresentation {
    Uniform(u8),
    Packed(Arc<[u8; PACKED_LIGHT_BYTES]>),
}

impl LightNibbleStorage {
    /// Creates a uniform channel after validating the nibble value.
    pub fn uniform(value: u8) -> Result<Self, LightStorageError> {
        validate_light(value)?;
        Ok(Self {
            representation: LightNibbleRepresentation::Uniform(value),
        })
    }

    /// Reads one linear sample.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<u8> {
        if index >= LIGHT_SAMPLES_PER_SUB_CHUNK {
            return None;
        }
        match &self.representation {
            LightNibbleRepresentation::Uniform(value) => Some(*value),
            LightNibbleRepresentation::Packed(bytes) => {
                let byte = bytes[index / 2];
                Some(if index & 1 == 0 {
                    byte & 0x0f
                } else {
                    byte >> 4
                })
            }
        }
    }

    /// Writes one sample, allocating packed bytes only when the value differs.
    pub fn set(&mut self, index: usize, value: u8) -> Result<bool, LightStorageError> {
        validate_light(value)?;
        if index >= LIGHT_SAMPLES_PER_SUB_CHUNK {
            return Err(LightStorageError::IndexOutOfRange { index });
        }
        if self.get(index) == Some(value) {
            return Ok(false);
        }
        if let LightNibbleRepresentation::Uniform(uniform) = &self.representation {
            let byte = *uniform | (*uniform << 4);
            self.representation =
                LightNibbleRepresentation::Packed(Arc::new([byte; PACKED_LIGHT_BYTES]));
        }
        let LightNibbleRepresentation::Packed(bytes) = &mut self.representation else {
            unreachable!("a differing uniform write always promotes to packed storage")
        };
        let bytes = Arc::make_mut(bytes);
        let slot = &mut bytes[index / 2];
        if index & 1 == 0 {
            *slot = (*slot & 0xf0) | value;
        } else {
            *slot = (*slot & 0x0f) | (value << 4);
        }
        self.collapse_if_uniform();
        Ok(true)
    }

    /// Replaces every sample and releases any packed allocation.
    pub fn fill(&mut self, value: u8) -> Result<(), LightStorageError> {
        validate_light(value)?;
        self.representation = LightNibbleRepresentation::Uniform(value);
        Ok(())
    }

    /// Returns whether the channel currently uses its allocation-free form.
    #[must_use]
    pub const fn is_uniform(&self) -> bool {
        matches!(&self.representation, LightNibbleRepresentation::Uniform(_))
    }

    /// Heap bytes owned by this storage shape, excluding shared control metadata.
    #[must_use]
    pub const fn allocated_bytes(&self) -> usize {
        match &self.representation {
            LightNibbleRepresentation::Uniform(_) => 0,
            LightNibbleRepresentation::Packed(_) => PACKED_LIGHT_BYTES,
        }
    }

    /// Returns whether two snapshots share the same packed payload.
    #[must_use]
    pub fn shares_packed_bytes_with(&self, other: &Self) -> bool {
        matches!(
            (&self.representation, &other.representation),
            (LightNibbleRepresentation::Packed(a), LightNibbleRepresentation::Packed(b))
                if Arc::ptr_eq(a, b)
        )
    }

    fn collapse_if_uniform(&mut self) {
        let LightNibbleRepresentation::Packed(bytes) = &self.representation else {
            return;
        };
        let first = bytes[0] & 0x0f;
        let repeated = first | (first << 4);
        if bytes.iter().all(|&byte| byte == repeated) {
            self.representation = LightNibbleRepresentation::Uniform(first);
        }
    }
}

fn validate_light(value: u8) -> Result<(), LightStorageError> {
    if value <= 15 {
        Ok(())
    } else {
        Err(LightStorageError::ValueOutOfRange { value })
    }
}

/// Independently stored block- and sky-light channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightChannel {
    Block,
    Sky,
}

/// Sparse light state for one sub-chunk generation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubChunkLight {
    block: LightNibbleStorage,
    sky: LightNibbleStorage,
    generation: u64,
}

impl SubChunkLight {
    /// Creates allocation-free zeroed channels.
    #[must_use]
    pub fn dark(generation: u64) -> Self {
        Self {
            block: LightNibbleStorage {
                representation: LightNibbleRepresentation::Uniform(0),
            },
            sky: LightNibbleStorage {
                representation: LightNibbleRepresentation::Uniform(0),
            },
            generation,
        }
    }

    /// Returns the generation that produced this light volume.
    #[must_use]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Reads one local coordinate from a channel.
    #[must_use]
    pub fn get(&self, channel: LightChannel, x: u8, y: u8, z: u8) -> Option<u8> {
        let index = local_index(x, y, z)?;
        self.channel(channel).get(index)
    }

    /// Writes one local coordinate to a channel.
    pub fn set(
        &mut self,
        channel: LightChannel,
        x: u8,
        y: u8,
        z: u8,
        value: u8,
    ) -> Result<bool, LightStorageError> {
        let index = local_index(x, y, z).ok_or(LightStorageError::IndexOutOfRange {
            index: LIGHT_SAMPLES_PER_SUB_CHUNK,
        })?;
        self.channel_mut(channel).set(index, value)
    }

    /// Returns a channel without exposing mutable packed bytes.
    #[must_use]
    pub const fn channel(&self, channel: LightChannel) -> &LightNibbleStorage {
        match channel {
            LightChannel::Block => &self.block,
            LightChannel::Sky => &self.sky,
        }
    }

    pub(crate) fn channel_mut(&mut self, channel: LightChannel) -> &mut LightNibbleStorage {
        match channel {
            LightChannel::Block => &mut self.block,
            LightChannel::Sky => &mut self.sky,
        }
    }
}

fn local_index(x: u8, y: u8, z: u8) -> Option<usize> {
    if x >= 16 || y >= 16 || z >= 16 {
        return None;
    }
    Some((usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y))
}

/// Streaming knowledge for one light-bearing sub-chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightSubChunkKind {
    Unknown,
    KnownAir,
    Resident,
}

#[derive(Debug, Clone)]
struct StoredSubChunkLight {
    kind: LightSubChunkKind,
    light: Arc<SubChunkLight>,
}

/// Immutable copy-on-write snapshot used by worker jobs.
#[derive(Debug, Clone, Default)]
pub struct LightStoreSnapshot {
    entries: BTreeMap<SubChunkKey, StoredSubChunkLight>,
}

impl LightStoreSnapshot {
    /// Returns the explicit boundary kind; absent entries remain unknown.
    #[must_use]
    pub fn kind(&self, key: SubChunkKey) -> LightSubChunkKind {
        self.entries
            .get(&key)
            .map_or(LightSubChunkKind::Unknown, |entry| entry.kind)
    }

    /// Returns a shared immutable light volume when the boundary is known.
    #[must_use]
    pub fn light(&self, key: SubChunkKey) -> Option<&Arc<SubChunkLight>> {
        self.entries.get(&key).map(|entry| &entry.light)
    }
}

/// Sparse light store kept separate from palette-native block storage.
#[derive(Debug, Default)]
pub struct LightStore {
    entries: BTreeMap<SubChunkKey, StoredSubChunkLight>,
}

impl LightStore {
    /// Returns the explicit boundary kind; absent entries remain unknown.
    #[must_use]
    pub fn kind(&self, key: SubChunkKey) -> LightSubChunkKind {
        self.entries
            .get(&key)
            .map_or(LightSubChunkKind::Unknown, |entry| entry.kind)
    }

    /// Returns the current shared light volume for a known boundary.
    #[must_use]
    pub fn light(&self, key: SubChunkKey) -> Option<&Arc<SubChunkLight>> {
        self.entries.get(&key).map(|entry| &entry.light)
    }

    /// Marks an explicitly received empty sub-chunk and stores its light.
    pub fn insert_known_air(&mut self, key: SubChunkKey, light: SubChunkLight) {
        self.insert(key, LightSubChunkKind::KnownAir, light);
    }

    /// Marks a palette-resident sub-chunk and stores its independent light.
    pub fn insert_resident(&mut self, key: SubChunkKey, light: SubChunkLight) {
        self.insert(key, LightSubChunkKind::Resident, light);
    }

    /// Commits only if the currently stored generation matches the snapshot.
    pub fn commit_if_generation(
        &mut self,
        key: SubChunkKey,
        expected: Option<u64>,
        replacement: SubChunkLight,
    ) -> bool {
        let current = self.light(key).map(|light| light.generation());
        if current != expected {
            return false;
        }
        let kind = self.kind(key);
        if kind == LightSubChunkKind::Unknown {
            return false;
        }
        self.insert(key, kind, replacement);
        true
    }

    /// Creates a cheap immutable snapshot by cloning only `Arc` handles.
    #[must_use]
    pub fn snapshot(&self) -> LightStoreSnapshot {
        LightStoreSnapshot {
            entries: self.entries.clone(),
        }
    }

    /// Creates an immutable snapshot containing only explicitly requested keys.
    pub fn snapshot_keys(&self, keys: impl IntoIterator<Item = SubChunkKey>) -> LightStoreSnapshot {
        LightStoreSnapshot {
            entries: keys
                .into_iter()
                .filter_map(|key| self.entries.get(&key).cloned().map(|entry| (key, entry)))
                .collect(),
        }
    }

    /// Removes one sub-chunk without disturbing other light in its column.
    pub fn remove(&mut self, key: SubChunkKey) -> bool {
        self.entries.remove(&key).is_some()
    }

    /// Removes all light state in one horizontal chunk column.
    pub fn evict_chunk(&mut self, key: ChunkKey) -> Vec<SubChunkKey> {
        let removed = self
            .entries
            .keys()
            .copied()
            .filter(|candidate| candidate.chunk() == key)
            .collect::<Vec<_>>();
        for candidate in &removed {
            self.entries.remove(candidate);
        }
        removed
    }

    fn insert(&mut self, key: SubChunkKey, kind: LightSubChunkKind, light: SubChunkLight) {
        self.entries.insert(
            key,
            StoredSubChunkLight {
                kind,
                light: Arc::new(light),
            },
        );
    }
}
