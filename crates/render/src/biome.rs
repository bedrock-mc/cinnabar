use std::sync::Arc;

use world::{BLOCKS_PER_SUB_CHUNK, BiomeStorage};

const HEADER_WORDS: usize = 1;
const BITS_MASK: u32 = 0xff;
const PALETTE_LEN_SHIFT: u32 = 8;
const PALETTE_LEN_MASK: u32 = 0x1fff;

/// Palette-native biome data prepared for one GPU sub-chunk record.
///
/// The original Bedrock packed index words are copied verbatim. Only the
/// small palette is remapped from wire biome IDs to tint-table indices, so no
/// 4,096-entry biome array is ever materialized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedBiomeRecord {
    words: Arc<[u32]>,
}

impl PackedBiomeRecord {
    /// Builds a GPU record while resolving only the source palette entries.
    #[must_use]
    pub fn from_storage(
        storage: &BiomeStorage,
        mut resolve_tint_index: impl FnMut(u32) -> u32,
    ) -> Self {
        let palette_len = storage.palette().len();
        debug_assert!((1..=BLOCKS_PER_SUB_CHUNK).contains(&palette_len));
        let header = u32::from(storage.bits_per_index())
            | (u32::try_from(palette_len).expect("biome palettes are bounded")
                << PALETTE_LEN_SHIFT);
        let mut words = Vec::with_capacity(
            HEADER_WORDS + storage.packed_words().len() + storage.palette().len(),
        );
        words.push(header);
        words.extend_from_slice(storage.packed_words());
        words.extend(
            storage
                .palette()
                .values()
                .iter()
                .copied()
                .map(&mut resolve_tint_index),
        );
        Self {
            words: words.into(),
        }
    }

    /// Uniform fallback record referencing tint-table entry zero.
    #[must_use]
    pub fn fallback() -> Self {
        Self {
            words: Arc::from([1 << PALETTE_LEN_SHIFT, 0]),
        }
    }

    /// Exact storage-buffer words: header, packed indices, remapped palette.
    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    #[must_use]
    pub fn bits_per_index(&self) -> u8 {
        (self.words[0] & BITS_MASK) as u8
    }

    #[must_use]
    pub fn palette_len(&self) -> usize {
        ((self.words[0] >> PALETTE_LEN_SHIFT) & PALETTE_LEN_MASK) as usize
    }

    #[must_use]
    pub fn byte_len(&self) -> u64 {
        self.words.len() as u64 * std::mem::size_of::<u32>() as u64
    }

    /// CPU mirror of the shader's native packed lookup, used by validation
    /// and deterministic tests without expanding the storage.
    #[must_use]
    pub fn tint_index(&self, x: u8, y: u8, z: u8) -> Option<u32> {
        if x >= 16 || y >= 16 || z >= 16 {
            return None;
        }
        let bits = usize::from(self.bits_per_index());
        let packed_word_count = if bits == 0 {
            0
        } else {
            let values_per_word = 32 / bits;
            BLOCKS_PER_SUB_CHUNK.div_ceil(values_per_word)
        };
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        let palette_index = if bits == 0 {
            0
        } else {
            let values_per_word = 32 / bits;
            let word = *self.words.get(HEADER_WORDS + linear / values_per_word)?;
            let shift = (linear % values_per_word) * bits;
            let mask = (1_u32 << bits) - 1;
            ((word >> shift) & mask) as usize
        };
        if palette_index >= self.palette_len() {
            return None;
        }
        self.words
            .get(HEADER_WORDS + packed_word_count + palette_index)
            .copied()
    }
}

impl Default for PackedBiomeRecord {
    fn default() -> Self {
        Self::fallback()
    }
}
