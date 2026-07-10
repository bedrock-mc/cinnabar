/// Number of block positions in a 16×16×16 sub-chunk.
pub const BLOCKS_PER_SUB_CHUNK: usize = 16 * 16 * 16;

/// Runtime values referenced by a [`PalettedStorage`].
///
/// The values are deliberately kept as raw network `u32`s. Depending on the
/// StartGame flags, a server may send sequential runtime IDs or block-state
/// network hashes. Resolving those values belongs to the registry layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    values: Box<[u32]>,
}

impl Palette {
    pub(crate) fn new(values: Vec<u32>) -> Self {
        Self {
            values: values.into_boxed_slice(),
        }
    }

    /// Returns the number of entries in the palette.
    #[must_use]
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Returns true when the palette has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns all raw runtime values in palette order.
    #[must_use]
    pub fn values(&self) -> &[u32] {
        &self.values
    }

    pub(crate) fn get(&self, index: usize) -> Option<u32> {
        self.values.get(index).copied()
    }
}

/// Packed indices plus the palette they reference for one block layer.
///
/// This is the runtime representation used by the client. It never expands
/// into a 4,096-element per-block array.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PalettedStorage {
    bits_per_index: u8,
    words: Box<[u32]>,
    palette: Palette,
}

impl PalettedStorage {
    pub(crate) fn new(bits_per_index: u8, words: Vec<u32>, palette: Vec<u32>) -> Self {
        Self {
            bits_per_index,
            words: words.into_boxed_slice(),
            palette: Palette::new(palette),
        }
    }

    /// Number of bits occupied by each packed palette index.
    #[must_use]
    pub fn bits_per_index(&self) -> u8 {
        self.bits_per_index
    }

    /// Packed little-endian words in Dragonfly/Bedrock layout.
    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    /// Alias for [`Self::words`] that makes the representation explicit.
    #[must_use]
    pub fn packed_words(&self) -> &[u32] {
        self.words()
    }

    /// Palette referenced by the packed indices.
    #[must_use]
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Returns true when all 4,096 positions reference one palette value.
    #[must_use]
    pub fn is_uniform(&self) -> bool {
        self.bits_per_index == 0
    }

    /// Returns the single value held by a uniform storage.
    #[must_use]
    pub fn uniform_runtime_id(&self) -> Option<u32> {
        self.is_uniform().then(|| self.palette.get(0)).flatten()
    }

    /// Looks up a raw network runtime value without expanding the storage.
    #[must_use]
    pub fn runtime_id(&self, x: u8, y: u8, z: u8) -> Option<u32> {
        if x >= 16 || y >= 16 || z >= 16 {
            return None;
        }
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        self.palette.get(self.palette_index(linear)?)
    }

    pub(crate) fn palette_index(&self, linear: usize) -> Option<usize> {
        if linear >= BLOCKS_PER_SUB_CHUNK {
            return None;
        }
        if self.bits_per_index == 0 {
            return Some(0);
        }

        let bits = usize::from(self.bits_per_index);
        let values_per_word = 32 / bits;
        let word = *self.words.get(linear / values_per_word)?;
        let shift = (linear % values_per_word) * bits;
        let mask = (1_u32 << self.bits_per_index) - 1;
        Some(((word >> shift) & mask) as usize)
    }
}
