use std::collections::{HashMap, HashSet};

/// Number of block positions in a 16×16×16 sub-chunk.
pub const BLOCKS_PER_SUB_CHUNK: usize = SUB_CHUNK_SIDE * SUB_CHUNK_SIDE * SUB_CHUNK_SIDE;
/// Block positions along each sub-chunk axis.
pub const SUB_CHUNK_SIDE: usize = 16;

pub(crate) const SUPPORTED_BITS: [u8; 9] = [0, 1, 2, 3, 4, 5, 6, 8, 16];

/// Storage widths that carry packed index words.
pub(crate) const PACKED_BITS: [u8; 8] = [1, 2, 3, 4, 5, 6, 8, 16];

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
#[derive(Debug, Clone)]
pub struct PalettedStorage {
    bits_per_index: u8,
    words: Box<[u32]>,
    palette: Palette,
    usage: Option<PaletteUsage>,
}

/// Derived counts are owned by each snapshot and never participate in storage equality.
#[derive(Debug, Clone)]
struct PaletteUsage {
    counts: Box<[u16]>,
    canonical: bool,
}

impl PartialEq for PalettedStorage {
    /// Compares the exact packed representation independently of retained bookkeeping.
    fn eq(&self, other: &Self) -> bool {
        self.bits_per_index == other.bits_per_index
            && self.words == other.words
            && self.palette == other.palette
    }
}
impl Eq for PalettedStorage {}

impl PalettedStorage {
    /// Retains the supplied representation; mutation bookkeeping is initialized on demand.
    pub(crate) fn new(bits_per_index: u8, words: Vec<u32>, palette: Vec<u32>) -> Self {
        Self {
            bits_per_index,
            words: words.into_boxed_slice(),
            palette: Palette::new(palette),
            usage: None,
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

    /// Writes every block's palette index into `out`, indexed `x << 8 | z << 4 | y`;
    /// [`u32::MAX`] marks a block whose packed word is missing from a malformed storage.
    pub fn palette_indices(&self, out: &mut [u32; BLOCKS_PER_SUB_CHUNK]) {
        if self.bits_per_index == 0 {
            out.fill(0);
            return;
        }
        let bits = usize::from(self.bits_per_index);
        let per_word = 32 / bits;
        let mask = (1_u32 << self.bits_per_index) - 1;
        for (word_index, chunk) in out.chunks_mut(per_word).enumerate() {
            let Some(&word) = self.words.get(word_index) else {
                for slot in &mut out[word_index * per_word..] {
                    *slot = u32::MAX;
                }
                return;
            };
            for (slot, value) in chunk.iter_mut().enumerate() {
                *value = (word >> (slot * bits)) & mask;
            }
        }
    }

    /// Writes the palette indices of column `(x, z)` in Y order. [`u16::MAX`] marks a block whose
    /// packed word is missing; callers still check every index against the palette length.
    ///
    /// # Panics
    ///
    /// Panics when `x` or `z` is not below [`SUB_CHUNK_SIDE`].
    pub fn column_palette_indices(&self, x: u8, z: u8, out: &mut [u16; SUB_CHUNK_SIDE]) {
        assert!(usize::from(x) < SUB_CHUNK_SIDE && usize::from(z) < SUB_CHUNK_SIDE);
        if self.bits_per_index == 0 {
            out.fill(0);
            return;
        }
        let bits = u32::from(self.bits_per_index);
        let per_word = (32 / bits) as usize;
        let mask = (1_u32 << bits) - 1;
        let first = (usize::from(x) << 8) | (usize::from(z) << 4);
        let (mut word, mut slot) = (first / per_word, first % per_word);
        for value in out {
            *value = self.words.get(word).map_or(u16::MAX, |packed| {
                ((packed >> (slot as u32 * bits)) & mask) as u16
            });
            slot += 1;
            if slot == per_word {
                slot = 0;
                word += 1;
            }
        }
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

    pub(crate) fn zero_indices_at_or_above(&mut self, palette_len: usize) {
        if self.bits_per_index == 0 || palette_len >= 1_usize << self.bits_per_index {
            return;
        }
        self.usage = None;
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            if self
                .palette_index(linear)
                .is_some_and(|index| index >= palette_len)
            {
                write_palette_index(&mut self.words, self.bits_per_index, linear, 0);
            }
        }
    }

    pub(crate) fn uniform(runtime_id: u32) -> Self {
        Self::new(0, Vec::new(), vec![runtime_id])
    }

    /// Patches one word when all existing indices remain canonical, otherwise rebuilds.
    /// Both paths produce the exact representation of [`Self::apply_runtime_updates`].
    pub(crate) fn apply_runtime_update(&mut self, linear: usize, runtime_id: u32) -> bool {
        let Some(updated_index) = self
            .palette_index(linear)
            .filter(|&index| index < self.palette.len())
        else {
            return self.apply_runtime_updates(&[(linear, runtime_id)]);
        };
        if self.palette.values[updated_index] == runtime_id {
            return false;
        }
        if self.bits_per_index == 0 {
            return self.apply_runtime_updates(&[(linear, runtime_id)]);
        }
        if self.usage.is_none() {
            self.usage = Some(self.count_usage());
        }
        let usage = self.usage.as_mut().expect("initialized palette usage");
        if usage.canonical && usage.counts[updated_index] > 1 {
            let destination = self.palette.values.iter().position(|&id| id == runtime_id);
            let destination = destination.or_else(|| {
                let index = self.palette.len();
                if bits_for_palette_len(index + 1) != self.bits_per_index {
                    return None;
                }
                let mut values = self.palette.values.to_vec();
                values.push(runtime_id);
                self.palette.values = values.into_boxed_slice();
                let mut counts = usage.counts.to_vec();
                counts.push(0);
                usage.counts = counts.into_boxed_slice();
                Some(index)
            });
            if let Some(destination) = destination {
                write_palette_index(&mut self.words, self.bits_per_index, linear, destination);
                usage.counts[updated_index] -= 1;
                usage.counts[destination] += 1;
                return true;
            }
        }
        self.apply_runtime_updates(&[(linear, runtime_id)])
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
        let mut counts = vec![0_u16; values.len()];
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            let current = self
                .palette_index(linear)
                .and_then(|index| self.palette.get(index))
                .expect("valid packed storage has a runtime value");
            let value = final_updates.get(&linear).copied().unwrap_or(current);
            let index = value_indices[&value];
            counts[index] += 1;
            write_palette_index(&mut words, bits_per_index, linear, index);
        }
        self.bits_per_index = bits_per_index;
        self.words = words.into_boxed_slice();
        self.palette.values = values.into_boxed_slice();
        self.usage = Some(PaletteUsage {
            canonical: counts.iter().all(|&count| count != 0),
            counts: counts.into_boxed_slice(),
        });
        true
    }

    /// Counts valid cells once and checks whether a word patch could preserve canonical output.
    fn count_usage(&self) -> PaletteUsage {
        let mut counts = vec![0_u16; self.palette.len()];
        let mut valid = true;
        for linear in 0..BLOCKS_PER_SUB_CHUNK {
            match self
                .palette_index(linear)
                .and_then(|index| counts.get_mut(index))
            {
                Some(count) => *count += 1,
                None => valid = false,
            }
        }
        let canonical = valid
            && (1..=BLOCKS_PER_SUB_CHUNK).contains(&self.palette.len())
            && counts.iter().all(|&count| count != 0)
            && self
                .palette
                .values
                .iter()
                .copied()
                .collect::<HashSet<_>>()
                .len()
                == self.palette.len()
            && bits_for_palette_len(self.palette.len()) == self.bits_per_index
            && self.has_zero_padding();
        PaletteUsage {
            counts: counts.into_boxed_slice(),
            canonical,
        }
    }

    /// Requires the exact word count and zero bits outside the valid packed cell slots.
    fn has_zero_padding(&self) -> bool {
        if self.words.len() != word_count(self.bits_per_index) {
            return false;
        }
        if self.bits_per_index == 0 {
            return true;
        }
        let bits = usize::from(self.bits_per_index);
        let per_word = 32 / bits;
        self.words.iter().enumerate().all(|(index, &word)| {
            let slots = (BLOCKS_PER_SUB_CHUNK - index * per_word).min(per_word);
            let used_bits = slots * bits;
            used_bits == 32 || word >> used_bits == 0
        })
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

#[cfg(test)]
mod tests;
