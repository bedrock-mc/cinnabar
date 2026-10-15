use std::sync::Arc;

use crate::{
    Palette, PalettedStorage,
    palette::PACKED_BITS,
    sub_chunk::{Reader, read_packed_storage},
};

/// Resolves raw biome ids the way the vanilla biome registry does.
pub trait BiomeIds {
    /// The dimension's fallback biome.
    fn default_biome(&self) -> u32;

    /// Returns the id when the registry knows it, otherwise the fallback biome.
    fn resolve(&self, biome_id: u16) -> u32;
}

/// Keeps every biome id; for decoders whose callers own id resolution.
#[derive(Debug, Clone, Copy)]
pub struct RawBiomeIds {
    pub default_biome: u32,
}

impl BiomeIds for RawBiomeIds {
    fn default_biome(&self) -> u32 {
        self.default_biome
    }

    fn resolve(&self, biome_id: u16) -> u32 {
        u32::from(biome_id)
    }
}

/// One packed 16x16x16 Bedrock biome storage.
///
/// Biome IDs remain palette-native and are looked up directly from packed
/// indices; the client never expands them into a 4,096-entry array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeStorage(PalettedStorage);

impl BiomeStorage {
    /// Maximum retained heap payload under the shared packed-storage representation.
    pub const MAX_RETAINED_HEAP_BYTES: usize = PalettedStorage::MAX_RETAINED_HEAP_BYTES;

    /// Current owned/shared heap payload, without canonicalizing or deduplicating shared data.
    #[must_use]
    pub fn retained_heap_bytes(&self) -> usize {
        self.0.retained_heap_bytes()
    }

    /// Conservative heap reservation, including any concurrently materialized canonical cache.
    #[must_use]
    pub fn retained_heap_bytes_bound(&self) -> usize {
        self.0.retained_heap_bytes_bound()
    }

    /// Number of bits occupied by each packed palette index.
    #[must_use]
    pub fn bits_per_index(&self) -> u8 {
        self.0.bits_per_index()
    }

    /// Packed little-endian words in Bedrock's padded-per-word layout.
    #[must_use]
    pub fn packed_words(&self) -> &[u32] {
        self.0.packed_words()
    }

    /// Raw biome IDs referenced by the packed indices.
    #[must_use]
    pub fn palette(&self) -> &Palette {
        self.0.palette()
    }

    /// Looks up a raw biome ID in Bedrock X-Z-Y linear order.
    #[must_use]
    pub fn biome_id(&self, x: u8, y: u8, z: u8) -> Option<u32> {
        self.0.runtime_id(x, y, z)
    }

    fn uniform(biome_id: u32) -> Self {
        Self(PalettedStorage::uniform(biome_id))
    }

    /// Every Y of each column repeats this storage's top layer.
    fn extruded_top_layer(&self) -> Self {
        let mut storage = PalettedStorage::uniform(self.biome_id(0, 15, 0).unwrap_or_default());
        let mut updates = Vec::with_capacity(crate::BLOCKS_PER_SUB_CHUNK);
        for x in 0..16_u8 {
            for z in 0..16_u8 {
                let biome = self.biome_id(x, 15, z).unwrap_or_default();
                for y in 0..16_u8 {
                    let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
                    updates.push((linear, biome));
                }
            }
        }
        storage.apply_runtime_updates(&updates);
        Self(storage)
    }
}

/// A dense vertical biome column decoded from LevelChunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBiomeColumn {
    pub(crate) base_sub_chunk_y: i32,
    pub(crate) storages: Box<[Arc<BiomeStorage>]>,
    pub(crate) bytes_consumed: usize,
}

impl DecodedBiomeColumn {
    /// Decodes `storage_count` network biome slots the way vanilla does.
    ///
    /// A `0xfe`/`0xff` header or end of input leaves a slot empty, and an
    /// invalid width empties it and consumes the rest of the payload. Empty
    /// slots below the last storage take the fallback biome; slots above it
    /// repeat that storage's top layer.
    pub fn decode(
        base_sub_chunk_y: i32,
        storage_count: usize,
        payload: &[u8],
        ids: &dyn BiomeIds,
    ) -> Self {
        let mut reader = Reader::new(payload);
        let mut slots: Vec<Option<Arc<BiomeStorage>>> = Vec::with_capacity(storage_count);
        for _ in 0..storage_count {
            if reader.is_at_end() {
                slots.push(None);
                continue;
            }
            let header = reader.read_u8();
            let entry = |reader: &mut Reader<'_>| ids.resolve(reader.read_var_i32() as u16);
            let storage = match header >> 1 {
                0x7f => None,
                0 => Some(PalettedStorage::uniform(entry(&mut reader))),
                bits if PACKED_BITS.contains(&bits) => {
                    Some(read_packed_storage(&mut reader, bits, entry))
                }
                _ => {
                    reader.skip_to_end();
                    None
                }
            };
            slots.push(storage.map(|storage| Arc::new(BiomeStorage(storage))));
        }
        let fallback = Arc::new(BiomeStorage::uniform(ids.default_biome()));
        let above = slots
            .iter()
            .rev()
            .find_map(Option::as_ref)
            .map(|top| Arc::new(top.extruded_top_layer()));
        let count = slots
            .iter()
            .rposition(Option::is_some)
            .map_or(0, |last| last + 1);
        let storages = slots
            .into_iter()
            .enumerate()
            .map(|(index, slot)| match (slot, &above) {
                (Some(storage), _) => storage,
                (None, Some(above)) if index >= count => Arc::clone(above),
                (None, _) => Arc::clone(&fallback),
            })
            .collect();
        Self {
            base_sub_chunk_y,
            storages,
            bytes_consumed: reader.position(),
        }
    }

    /// First absolute sub-chunk Y represented by the column.
    #[must_use]
    pub fn base_sub_chunk_y(&self) -> i32 {
        self.base_sub_chunk_y
    }

    /// Number of vertical biome storages in this column.
    #[must_use]
    pub fn len(&self) -> usize {
        self.storages.len()
    }

    /// Returns true when the column contains no biome storages.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.storages.is_empty()
    }

    /// Bytes occupied by the decoded biome-storage prefix.
    #[must_use]
    pub fn bytes_consumed(&self) -> usize {
        self.bytes_consumed
    }

    /// Returns the packed storage for one absolute sub-chunk Y.
    #[must_use]
    pub fn storage(&self, sub_chunk_y: i32) -> Option<Arc<BiomeStorage>> {
        let offset = sub_chunk_y.checked_sub(self.base_sub_chunk_y)?;
        let offset = usize::try_from(offset).ok()?;
        self.storages.get(offset).cloned()
    }
}
