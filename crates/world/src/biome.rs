use std::sync::Arc;

use crate::{
    DecodeError, Palette, PalettedStorage,
    sub_chunk::{Reader, decode_storage_with_header},
};

/// One packed 16x16x16 Bedrock biome storage.
///
/// Biome IDs remain palette-native and are looked up directly from packed
/// indices; the client never expands them into a 4,096-entry array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BiomeStorage(PalettedStorage);

impl BiomeStorage {
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
}

/// A fully validated dense vertical biome column decoded from LevelChunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedBiomeColumn {
    pub(crate) base_sub_chunk_y: i32,
    pub(crate) storages: Box<[Arc<BiomeStorage>]>,
    pub(crate) bytes_consumed: usize,
}

impl DecodedBiomeColumn {
    /// Decodes exactly `storage_count` network biome storages.
    ///
    /// The `0xff` copy-previous marker reuses the preceding storage's `Arc`,
    /// retaining the compact representation emitted by vanilla servers.
    pub fn decode(
        base_sub_chunk_y: i32,
        storage_count: usize,
        payload: &[u8],
    ) -> Result<Self, DecodeError> {
        let mut reader = Reader::new(payload);
        let mut storages: Vec<Arc<BiomeStorage>> = Vec::with_capacity(storage_count);
        for index in 0..storage_count {
            let header = reader.read_u8("biome palette header")?;
            if header == 0xff {
                let previous = storages
                    .last()
                    .cloned()
                    .ok_or(DecodeError::BiomeCopyWithoutPrevious { index })?;
                storages.push(previous);
                continue;
            }
            let storage = decode_storage_with_header(&mut reader, header)?;
            storages.push(Arc::new(BiomeStorage(storage)));
        }
        Ok(Self {
            base_sub_chunk_y,
            storages: storages.into_boxed_slice(),
            bytes_consumed: reader.position(),
        })
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
