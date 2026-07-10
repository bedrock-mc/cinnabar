use std::collections::HashMap;

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

    /// Changes one packed entry, growing only through legal Bedrock widths.
    /// Coordinates have already been validated by the store boundary.
    pub(crate) fn set_runtime_id(&mut self, x: u8, y: u8, z: u8, runtime_id: u32) -> bool {
        let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
        let current_index = self
            .palette_index(linear)
            .expect("validated local coordinates have a packed index");
        if self.palette.get(current_index) == Some(runtime_id) {
            return false;
        }

        let palette_index = if let Some(index) = self
            .palette
            .values
            .iter()
            .position(|&value| value == runtime_id)
        {
            index
        } else {
            if self.palette.len() == BLOCKS_PER_SUB_CHUNK {
                let current_is_unique = (0..BLOCKS_PER_SUB_CHUNK).all(|other| {
                    other == linear || self.palette_index(other) != Some(current_index)
                });
                if current_is_unique {
                    let mut values = self.palette.values.to_vec();
                    values[current_index] = runtime_id;
                    self.palette.values = values.into_boxed_slice();
                    return true;
                }
                // A repeated current index in a full-sized palette guarantees
                // that at least one palette slot is unused. Compact it away
                // before appending the replacement value.
                self.compact();
            }

            let index = self.palette.len();
            debug_assert!(index < BLOCKS_PER_SUB_CHUNK);
            let required_bits = bits_for_palette_len(index + 1);
            if required_bits != self.bits_per_index {
                self.repack(required_bits, None);
            }
            let mut values = self.palette.values.to_vec();
            values.push(runtime_id);
            self.palette.values = values.into_boxed_slice();
            index
        };
        write_palette_index(&mut self.words, self.bits_per_index, linear, palette_index);
        true
    }

    /// Removes unused and duplicate palette entries and selects the smallest
    /// legal width. The temporary maps are palette-sized, never block-sized.
    pub(crate) fn compact(&mut self) {
        let mut used = vec![false; self.palette.len()];
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            let index = self
                .palette_index(linear)
                .expect("valid storage has every packed index");
            used[index] = true;
        }

        let mut values = Vec::with_capacity(self.palette.len());
        let mut value_indices = HashMap::with_capacity(self.palette.len());
        let mut remap = vec![0; self.palette.len()];
        for (old_index, (&value, is_used)) in
            self.palette.values.iter().zip(used.into_iter()).enumerate()
        {
            if !is_used {
                continue;
            }
            let next_index = values.len();
            let new_index = *value_indices.entry(value).or_insert_with(|| {
                values.push(value);
                next_index
            });
            remap[old_index] = new_index;
        }

        debug_assert!(!values.is_empty());
        let bits = bits_for_palette_len(values.len());
        let identity = values.len() == self.palette.len()
            && remap.iter().enumerate().all(|(old, &new)| old == new);
        if bits != self.bits_per_index || !identity {
            self.repack(bits, Some(&remap));
        }
        self.palette.values = values.into_boxed_slice();
    }

    pub(crate) fn contains_only(&self, runtime_id: u32) -> bool {
        (0..BLOCKS_PER_SUB_CHUNK).all(|linear| {
            self.palette_index(linear)
                .and_then(|index| self.palette.get(index))
                == Some(runtime_id)
        })
    }

    fn repack(&mut self, bits_per_index: u8, remap: Option<&[usize]>) {
        let mut words = vec![0; word_count(bits_per_index)];
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            let old_index = self
                .palette_index(linear)
                .expect("valid storage has every packed index");
            let index = remap.map_or(old_index, |indices| indices[old_index]);
            write_palette_index(&mut words, bits_per_index, linear, index);
        }
        self.bits_per_index = bits_per_index;
        self.words = words.into_boxed_slice();
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
