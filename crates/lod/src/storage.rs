//! Palette + bit-packed voxel storage and the dense scratch form jobs expand it into.

use std::collections::HashMap;

use foldhash::fast::FixedState;

use crate::{
    biome_grid::BiomeGrid,
    geometry::{SECTION_VOXELS, VoxelBox, voxel_index},
    voxel::Voxel,
};

/// Bits in one packed index word.
pub const PACKED_WORD_BITS: usize = u64::BITS as usize;
/// Widest index a section needs: one palette entry per voxel.
pub const MAX_BITS_PER_INDEX: u8 = SECTION_VOXELS.trailing_zeros() as u8;

/// Why stored voxel parts do not form a valid section.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StorageError {
    #[error("palette has {0} entries; expected 1..={SECTION_VOXELS}")]
    PaletteLength(usize),
    #[error("palette repeats voxel {0:#010x}")]
    DuplicatePalette(u32),
    #[error("{bits} bits per index does not match a palette of {palette_len}")]
    BitsPerIndex { bits: u8, palette_len: usize },
    #[error("{actual} packed words; expected {expected}")]
    WordCount { actual: usize, expected: usize },
    #[error("packed index {index} at voxel {voxel} exceeds the palette")]
    IndexOutOfRange { voxel: usize, index: u64 },
    #[error("packed word {0} sets padding bits")]
    Padding(usize),
    #[error("biome palette has {0} entries; expected 1..=256")]
    BiomePaletteLength(usize),
    #[error("biome cell {cell} indexes entry {index} of a {palette_len}-entry palette")]
    BiomeIndex {
        cell: usize,
        index: u8,
        palette_len: usize,
    },
    #[error("child mask {0:#06x} marks a child non-empty without knowing it")]
    ChildMask(u16),
    #[error("level-0 section given child mask {0:#06x}; level 0 has no children")]
    LevelZeroChildren(u16),
}

/// Index width for a palette of `len` entries: 0 for one entry, else `ceil(log2(len))`.
#[must_use]
pub const fn bits_for_palette_len(len: usize) -> u8 {
    if len <= 1 {
        0
    } else {
        (usize::BITS - (len - 1).leading_zeros()) as u8
    }
}

/// Packed words holding every voxel's index at `bits` per index. Indices never straddle
/// words; each word holds `64 / bits` indices from its least significant bit up.
#[must_use]
pub const fn packed_word_count(bits: u8) -> usize {
    if bits == 0 {
        0
    } else {
        SECTION_VOXELS.div_ceil(PACKED_WORD_BITS / bits as usize)
    }
}

/// A section's voxels as a palette in first-use order plus packed indices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackedVoxels {
    palette: Box<[Voxel]>,
    bits: u8,
    words: Box<[u64]>,
}

impl PackedVoxels {
    /// Every voxel equal to `voxel`; no index words.
    #[must_use]
    pub fn uniform(voxel: Voxel) -> Self {
        Self {
            palette: Box::new([voxel]),
            bits: 0,
            words: Box::new([]),
        }
    }

    /// Packs dense voxels into their canonical form, reusing `scratch` so steady-state
    /// packing allocates only the result.
    #[must_use]
    pub fn from_dense(voxels: &[Voxel; SECTION_VOXELS], scratch: &mut PackScratch) -> Self {
        let PackScratch {
            lookup,
            palette,
            indices,
        } = scratch;
        lookup.clear();
        palette.clear();
        let mut previous = None;
        for (slot, &voxel) in indices.iter_mut().zip(voxels.iter()) {
            *slot = match previous {
                Some((last, index)) if last == voxel => index,
                _ => {
                    let index = *lookup.entry(voxel).or_insert_with(|| {
                        palette.push(voxel);
                        (palette.len() - 1) as u16
                    });
                    previous = Some((voxel, index));
                    index
                }
            };
        }
        let bits = bits_for_palette_len(palette.len());
        let mut words = vec![0_u64; packed_word_count(bits)].into_boxed_slice();
        if bits > 0 {
            let per_word = PACKED_WORD_BITS / usize::from(bits);
            for (word, chunk) in words.iter_mut().zip(indices.chunks(per_word)) {
                *word = chunk.iter().enumerate().fold(0, |word, (slot, &index)| {
                    word | u64::from(index) << (slot * usize::from(bits))
                });
            }
        }
        Self {
            palette: palette.as_slice().into(),
            bits,
            words,
        }
    }

    /// Validates stored parts: distinct palette, minimal width, exact word count,
    /// in-range indices and zero padding. Palette order is not constrained.
    pub fn from_parts(
        palette: Vec<Voxel>,
        bits: u8,
        words: Vec<u64>,
    ) -> Result<Self, StorageError> {
        if palette.is_empty() || palette.len() > SECTION_VOXELS {
            return Err(StorageError::PaletteLength(palette.len()));
        }
        let mut seen = std::collections::HashSet::with_capacity(palette.len());
        if let Some(duplicate) = palette.iter().find(|voxel| !seen.insert(**voxel)) {
            return Err(StorageError::DuplicatePalette(duplicate.bits()));
        }
        if bits != bits_for_palette_len(palette.len()) {
            return Err(StorageError::BitsPerIndex {
                bits,
                palette_len: palette.len(),
            });
        }
        let expected = packed_word_count(bits);
        if words.len() != expected {
            return Err(StorageError::WordCount {
                actual: words.len(),
                expected,
            });
        }
        let packed = Self {
            palette: palette.into_boxed_slice(),
            bits,
            words: words.into_boxed_slice(),
        };
        packed.validate_indices()?;
        Ok(packed)
    }

    /// Checks that every index is inside the palette and padding bits are zero.
    fn validate_indices(&self) -> Result<(), StorageError> {
        if self.bits == 0 {
            return Ok(());
        }
        let bits = usize::from(self.bits);
        let per_word = PACKED_WORD_BITS / bits;
        let mask = (1_u64 << bits) - 1;
        for (word_index, &word) in self.words.iter().enumerate() {
            let first = word_index * per_word;
            let used = per_word.min(SECTION_VOXELS - first);
            let used_bits = used * bits;
            if used_bits < PACKED_WORD_BITS && word >> used_bits != 0 {
                return Err(StorageError::Padding(word_index));
            }
            for slot in 0..used {
                let index = (word >> (slot * bits)) & mask;
                if index as usize >= self.palette.len() {
                    return Err(StorageError::IndexOutOfRange {
                        voxel: first + slot,
                        index,
                    });
                }
            }
        }
        Ok(())
    }

    /// Distinct voxels in first-use order for packed sections.
    #[must_use]
    pub fn palette(&self) -> &[Voxel] {
        &self.palette
    }

    /// Index width; 0 when every voxel is the single palette entry.
    #[must_use]
    pub fn bits_per_index(&self) -> u8 {
        self.bits
    }

    /// Packed index words; see [`packed_word_count`] for the layout.
    #[must_use]
    pub fn words(&self) -> &[u64] {
        &self.words
    }

    /// The only voxel of a uniform section.
    #[must_use]
    pub fn uniform_voxel(&self) -> Option<Voxel> {
        (self.bits == 0).then(|| self.palette[0])
    }

    /// The voxel at a linear [`voxel_index`].
    #[must_use]
    pub fn get(&self, index: usize) -> Voxel {
        if self.bits == 0 {
            return self.palette[0];
        }
        let bits = usize::from(self.bits);
        let per_word = PACKED_WORD_BITS / bits;
        let word = self.words[index / per_word];
        let palette_index = (word >> (index % per_word * bits)) & ((1 << bits) - 1);
        self.palette[palette_index as usize]
    }

    /// Writes every voxel into `out`.
    pub fn expand_into(&self, out: &mut [Voxel; SECTION_VOXELS]) {
        if self.bits == 0 {
            out.fill(self.palette[0]);
            return;
        }
        let bits = usize::from(self.bits);
        let per_word = PACKED_WORD_BITS / bits;
        let mask = (1_u64 << bits) - 1;
        for (chunk, &word) in out.chunks_mut(per_word).zip(self.words.iter()) {
            for (slot, voxel) in chunk.iter_mut().enumerate() {
                *voxel = self.palette[((word >> (slot * bits)) & mask) as usize];
            }
        }
    }

    /// True when no voxel holds a material.
    #[must_use]
    pub fn is_all_air(&self) -> bool {
        self.palette.iter().all(|voxel| voxel.is_air())
    }

    /// Heap bytes held by the palette and index words.
    #[must_use]
    pub fn heap_bytes(&self) -> usize {
        size_of_val(&*self.palette) + size_of_val(&*self.words)
    }
}

/// Reusable buffers for [`PackedVoxels::from_dense`]: the palette lookup, palette and one
/// index per voxel. Holds no state between calls.
#[derive(Debug, Clone)]
pub struct PackScratch {
    lookup: HashMap<Voxel, u16, FixedState>,
    palette: Vec<Voxel>,
    indices: Box<[u16; SECTION_VOXELS]>,
}

impl Default for PackScratch {
    fn default() -> Self {
        Self {
            lookup: HashMap::with_hasher(FixedState::default()),
            palette: Vec::new(),
            indices: vec![0; SECTION_VOXELS]
                .into_boxed_slice()
                .try_into()
                .expect("one index per voxel"),
        }
    }
}

/// A section expanded to one word per voxel, plus its biome grid; the working form of
/// sampler and mip jobs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenseSection {
    voxels: Box<[Voxel; SECTION_VOXELS]>,
    biomes: BiomeGrid,
}

impl DenseSection {
    /// Every voxel set to `voxel` and every biome cell to `biome`.
    #[must_use]
    pub fn filled(voxel: Voxel, biome: u32) -> Self {
        let voxels = vec![voxel; SECTION_VOXELS]
            .into_boxed_slice()
            .try_into()
            .expect("the vector has exactly one section of voxels");
        Self {
            voxels,
            biomes: BiomeGrid::uniform(biome),
        }
    }

    /// Expands packed voxels and copies the biome grid, reusing this allocation.
    pub fn load(&mut self, voxels: &PackedVoxels, biomes: &BiomeGrid) {
        voxels.expand_into(&mut self.voxels);
        self.biomes.clone_from(biomes);
    }

    /// Every voxel in [`voxel_index`] order.
    #[must_use]
    pub fn voxels(&self) -> &[Voxel; SECTION_VOXELS] {
        &self.voxels
    }

    /// Every voxel in [`voxel_index`] order, for bulk writes.
    pub fn voxels_mut(&mut self) -> &mut [Voxel; SECTION_VOXELS] {
        &mut self.voxels
    }

    /// The surface biome of each voxel column.
    #[must_use]
    pub fn biomes(&self) -> &BiomeGrid {
        &self.biomes
    }

    /// The biome grid, for writes.
    pub fn biomes_mut(&mut self) -> &mut BiomeGrid {
        &mut self.biomes
    }

    /// The voxel at section-local coordinates; panics outside `0..32`.
    #[must_use]
    pub fn get(&self, x: usize, y: usize, z: usize) -> Voxel {
        self.voxels[voxel_index(x, y, z)]
    }

    /// Replaces one voxel; panics outside `0..32`.
    pub fn set(&mut self, x: usize, y: usize, z: usize, voxel: Voxel) {
        self.voxels[voxel_index(x, y, z)] = voxel;
    }

    /// Sets every voxel inside `region`.
    pub fn fill_box(&mut self, region: VoxelBox, voxel: Voxel) {
        let [x0, y0, z0] = region.min();
        let [x1, y1, z1] = region.max();
        for y in y0..y1 {
            for z in z0..z1 {
                self.voxels[voxel_index(x0, y, z)..voxel_index(x1, y, z)].fill(voxel);
            }
        }
    }

    /// True when no voxel holds a material.
    #[must_use]
    pub fn is_all_air(&self) -> bool {
        self.voxels.iter().all(|voxel| voxel.is_air())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        geometry::Octant,
        voxel::{Light, MaterialId},
    };

    fn distinct(count: usize) -> Vec<Voxel> {
        (0..count)
            .map(|index| {
                let material = MaterialId::new(1 + (index / 16) as u32).expect("material");
                Voxel::new(material, Light::new((index % 16) as u8, 0), 7, false)
            })
            .collect()
    }

    fn dense_from(palette: &[Voxel]) -> Box<[Voxel; SECTION_VOXELS]> {
        let mut dense = Box::new([Voxel::AIR; SECTION_VOXELS]);
        for (index, voxel) in dense.iter_mut().enumerate() {
            *voxel = palette[(index * 7 + index / 3) % palette.len()];
        }
        dense
    }

    #[test]
    fn dense_round_trips_through_every_width() {
        // One scratch for every width: packing must not depend on earlier calls.
        let mut scratch = PackScratch::default();
        for len in [1, 2, 3, 17, 300, 4096, SECTION_VOXELS] {
            let palette = distinct(len);
            let dense = if len == SECTION_VOXELS {
                Box::new(std::array::from_fn(|index| palette[index]))
            } else {
                dense_from(&palette)
            };
            let packed = PackedVoxels::from_dense(&dense, &mut scratch);
            assert_eq!(
                packed.bits_per_index(),
                bits_for_palette_len(packed.palette().len())
            );
            assert_eq!(
                packed.words().len(),
                packed_word_count(packed.bits_per_index())
            );
            let mut expanded = Box::new([Voxel::AIR; SECTION_VOXELS]);
            packed.expand_into(&mut expanded);
            assert_eq!(expanded, dense, "palette of {len}");
            assert_eq!(packed.get(12_345), dense[12_345]);
            let rebuilt = PackedVoxels::from_parts(
                packed.palette().to_vec(),
                packed.bits_per_index(),
                packed.words().to_vec(),
            );
            assert_eq!(rebuilt.as_ref(), Ok(&packed));
        }
        assert_eq!(bits_for_palette_len(SECTION_VOXELS), MAX_BITS_PER_INDEX);
    }

    #[test]
    fn uniform_sections_store_no_index_words() {
        let voxel = distinct(1)[0];
        let packed = PackedVoxels::from_dense(
            &Box::new([voxel; SECTION_VOXELS]),
            &mut PackScratch::default(),
        );
        assert_eq!(packed, PackedVoxels::uniform(voxel));
        assert_eq!(packed.bits_per_index(), 0);
        assert!(packed.words().is_empty());
        assert_eq!(packed.uniform_voxel(), Some(voxel));
        assert_eq!(packed.heap_bytes(), size_of::<Voxel>());
        assert!(!packed.is_all_air());
        assert!(PackedVoxels::uniform(Voxel::air(Light::FULL_SKY)).is_all_air());
    }

    #[test]
    fn malformed_parts_are_rejected() {
        let palette = distinct(3);
        let words = || vec![0_u64; packed_word_count(2)];
        assert_eq!(
            PackedVoxels::from_parts(Vec::new(), 0, Vec::new()),
            Err(StorageError::PaletteLength(0))
        );
        assert_eq!(
            PackedVoxels::from_parts(
                vec![palette[0], palette[0]],
                1,
                vec![0; packed_word_count(1)]
            ),
            Err(StorageError::DuplicatePalette(palette[0].bits()))
        );
        assert_eq!(
            PackedVoxels::from_parts(palette.clone(), 3, words()),
            Err(StorageError::BitsPerIndex {
                bits: 3,
                palette_len: 3
            })
        );
        assert_eq!(
            PackedVoxels::from_parts(palette.clone(), 2, vec![0; 3]),
            Err(StorageError::WordCount {
                actual: 3,
                expected: packed_word_count(2)
            })
        );
        let mut out_of_range = words();
        out_of_range[1] = 3 << 2;
        assert_eq!(
            PackedVoxels::from_parts(palette.clone(), 2, out_of_range),
            Err(StorageError::IndexOutOfRange {
                voxel: 33,
                index: 3
            })
        );
        // 15-bit indices leave four padding bits per word.
        let wide = distinct(SECTION_VOXELS / 2 + 1);
        let mut padded = vec![0_u64; packed_word_count(15)];
        padded[0] = 1 << 62;
        assert_eq!(
            PackedVoxels::from_parts(wide, 15, padded),
            Err(StorageError::Padding(0))
        );
    }

    #[test]
    fn dense_sections_fill_boxes_and_load_packed_parts() {
        let stone = distinct(2)[1];
        let mut dense = DenseSection::filled(Voxel::AIR, 4);
        dense.fill_box(Octant::ALL[7].parent_region(), stone);
        assert_eq!(dense.get(16, 16, 16), stone);
        assert_eq!(dense.get(31, 31, 31), stone);
        assert_eq!(dense.get(15, 31, 31), Voxel::AIR);
        assert!(!dense.is_all_air());
        let packed = PackedVoxels::from_dense(dense.voxels(), &mut PackScratch::default());
        let mut loaded = DenseSection::filled(stone, 0);
        loaded.load(&packed, dense.biomes());
        assert_eq!(loaded, dense);
    }
}
