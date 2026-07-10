use std::collections::{HashMap, HashSet};

/// Number of block positions in a 16×16×16 sub-chunk.
pub const BLOCKS_PER_SUB_CHUNK: usize = 16 * 16 * 16;

pub(crate) const SUPPORTED_BITS: [u8; 9] = [0, 1, 2, 3, 4, 5, 6, 8, 16];

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

    pub(crate) fn uniform(runtime_id: u32) -> Self {
        Self::new(0, Vec::new(), vec![runtime_id])
    }

    /// Applies a whole layer's final mutations with one palette-map build and
    /// one packed-word allocation, regardless of duplicate coordinates.
    pub(crate) fn apply_runtime_updates(&mut self, updates: &[(usize, u32)]) -> bool {
        let mut final_updates = HashMap::with_capacity(updates.len());
        for &(linear, runtime_id) in updates {
            final_updates.insert(linear, runtime_id);
        }

        let mut changed = false;
        let mut used_values = HashSet::with_capacity(self.palette.len().min(BLOCKS_PER_SUB_CHUNK));
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            let current = self
                .palette_index(linear)
                .and_then(|index| self.palette.get(index))
                .expect("valid packed storage has a runtime value");
            let value = final_updates.get(&linear).copied().unwrap_or(current);
            changed |= value != current;
            used_values.insert(value);
        }
        if !changed {
            return false;
        }

        let mut values = Vec::with_capacity(used_values.len());
        let mut value_indices = HashMap::with_capacity(used_values.len());
        for &value in self.palette.values.iter() {
            if used_values.contains(&value) && !value_indices.contains_key(&value) {
                let index = values.len();
                values.push(value);
                value_indices.insert(value, index);
            }
        }
        for &(linear, value) in updates {
            if final_updates.get(&linear) == Some(&value) && !value_indices.contains_key(&value) {
                let index = values.len();
                values.push(value);
                value_indices.insert(value, index);
            }
        }

        let bits_per_index = bits_for_palette_len(values.len());
        let mut words = vec![0; word_count(bits_per_index)];
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            let current = self
                .palette_index(linear)
                .and_then(|index| self.palette.get(index))
                .expect("valid packed storage has a runtime value");
            let value = final_updates.get(&linear).copied().unwrap_or(current);
            let index = value_indices[&value];
            write_palette_index(&mut words, bits_per_index, linear, index);
        }
        self.bits_per_index = bits_per_index;
        self.words = words.into_boxed_slice();
        self.palette.values = values.into_boxed_slice();
        true
    }

    pub(crate) fn contains_only(&self, runtime_id: u32) -> bool {
        (0..BLOCKS_PER_SUB_CHUNK).all(|linear| {
            self.palette_index(linear)
                .and_then(|index| self.palette.get(index))
                == Some(runtime_id)
        })
    }
}

pub(crate) fn word_count(bits_per_index: u8) -> usize {
    if bits_per_index == 0 {
        return 0;
    }
    let values_per_word = 32 / usize::from(bits_per_index);
    BLOCKS_PER_SUB_CHUNK.div_ceil(values_per_word)
}

fn bits_for_palette_len(palette_len: usize) -> u8 {
    debug_assert!((1..=BLOCKS_PER_SUB_CHUNK).contains(&palette_len));
    SUPPORTED_BITS
        .into_iter()
        .find(|&bits| bits == 16 || palette_len <= 1_usize << bits)
        .expect("16-bit palettes cover every sub-chunk block")
}

fn write_palette_index(words: &mut [u32], bits_per_index: u8, linear: usize, index: usize) {
    if bits_per_index == 0 {
        debug_assert_eq!(index, 0);
        return;
    }
    let bits = usize::from(bits_per_index);
    let values_per_word = 32 / bits;
    let word = &mut words[linear / values_per_word];
    let shift = (linear % values_per_word) * bits;
    let mask = ((1_u32 << bits_per_index) - 1) << shift;
    *word = (*word & !mask) | (((index as u32) << shift) & mask);
}
