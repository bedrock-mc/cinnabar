//! The section padded with its neighbours' touching slabs, as voxel words, class bits and
//! one class mask per padded column.
//!
//! Padded coordinates run `0..PAD`; section voxel `v` sits at `v + 1`. A column runs along
//! X and is indexed `y * PAD + z`; bit `x` of its masks is padded voxel `x`. Edge and corner
//! cells of the padding are never read by faces and stay empty.

use crate::{
    geometry::{Face, SECTION_SIDE, SECTION_VOXELS, voxel_index},
    mesh::{
        FALLBACK_LIGHT,
        class::{AIR, CLASS_BITS, MeshClasses, SEE_THROUGH, SOLID, WATER, WET},
        input::MeshInput,
    },
    storage::{PACKED_WORD_BITS, PackedVoxels},
    voxel::Voxel,
};

/// Padded voxels along each axis.
pub(super) const PAD: usize = SECTION_SIDE + 2;
/// Padded columns.
pub(super) const PAD2: usize = PAD * PAD;
/// Padded voxels.
pub(super) const PAD3: usize = PAD2 * PAD;
/// Every bit of a padded column.
pub(super) const COLUMN_BITS: u64 = (1 << PAD) - 1;
/// Flag bytes read past the last one by the eight-at-a-time packing.
const FLAG_SLACK: usize = 8;

const _: () = assert!(PAD <= u64::BITS as usize);

/// The padded index of section voxel number `voxel` in packed order (X fastest, then Z,
/// then Y).
const fn interior_index(voxel: usize) -> usize {
    let (row, x) = (voxel / SECTION_SIDE, voxel % SECTION_SIDE);
    let (y, z) = (row / SECTION_SIDE, row % SECTION_SIDE);
    padded_index(x + 1, y + 1, z + 1)
}

/// Linear index of padded coordinates.
pub(super) const fn padded_index(x: usize, y: usize, z: usize) -> usize {
    (y * PAD + z) * PAD + x
}

/// Index offset of one padded step along `axis` (0 X, 1 Y, 2 Z).
pub(super) const fn axis_stride(axis: usize) -> usize {
    match axis {
        0 => 1,
        1 => PAD2,
        _ => PAD,
    }
}

/// Class masks of every padded column, one per class bit.
pub(super) struct Masks {
    pub(super) air: Box<[u64]>,
    pub(super) solid: Box<[u64]>,
    pub(super) water: Box<[u64]>,
    pub(super) see_through: Box<[u64]>,
    /// Waterlogged see-through voxels, a subset of `see_through`.
    pub(super) wet: Box<[u64]>,
}

/// Reusable padded section of one mesh job.
pub(super) struct Padded {
    pub(super) words: Box<[u32]>,
    pub(super) flags: Box<[u8]>,
    pub(super) masks: Masks,
    palette: Vec<(u32, u8)>,
}

impl Padded {
    /// Zeroed buffers sized for one padded section, allocated once per mesher.
    pub(super) fn new() -> Self {
        let column = || vec![0_u64; PAD2].into_boxed_slice();
        Self {
            words: vec![0; PAD3].into_boxed_slice(),
            flags: vec![0; PAD3 + FLAG_SLACK].into_boxed_slice(),
            masks: Masks {
                air: column(),
                solid: column(),
                water: column(),
                see_through: column(),
                wet: column(),
            },
            palette: Vec::new(),
        }
    }

    /// Bytes held between jobs.
    pub(super) fn heap_bytes(&self) -> usize {
        size_of_val(&*self.words)
            + size_of_val(&*self.flags)
            + CLASS_BITS as usize * PAD2 * size_of::<u64>()
            + self.palette.capacity() * size_of::<(u32, u8)>()
    }

    /// Copies `input`'s section and touching neighbour slabs in, then builds the masks.
    /// Unknown neighbours become air lit by [`FALLBACK_LIGHT`].
    pub(super) fn gather(&mut self, input: &MeshInput<'_>, classes: &MeshClasses) {
        self.clear_padding();
        self.gather_section(input.section().voxels(), classes);
        for face in Face::ALL {
            self.gather_slab(face, input.face(face).same.map(|s| s.voxels()), classes);
        }
        self.build_masks();
    }

    /// Resets every padding cell to unlit air.
    fn clear_padding(&mut self) {
        let air = (Voxel::AIR.bits(), AIR);
        for y in 0..PAD {
            for z in 0..PAD {
                let row = padded_index(0, y, z);
                if y == 0 || y == PAD - 1 || z == 0 || z == PAD - 1 {
                    self.fill(row..row + PAD, air);
                } else {
                    self.fill(row..row + 1, air);
                    self.fill(row + PAD - 1..row + PAD, air);
                }
            }
        }
    }

    /// Writes `word` and its class `flag` together over the padded index `range`.
    fn fill(&mut self, range: std::ops::Range<usize>, (word, flag): (u32, u8)) {
        self.words[range.clone()].fill(word);
        self.flags[range].fill(flag);
    }

    /// Decodes the packed section straight into the padded interior.
    fn gather_section(&mut self, voxels: &PackedVoxels, classes: &MeshClasses) {
        self.palette.clear();
        self.palette.extend(voxels.palette().iter().map(|voxel| {
            let word = voxel.bits();
            (word, classes.of_word(word))
        }));
        let bits = usize::from(voxels.bits_per_index());
        if bits == 0 {
            let only = self.palette[0];
            for y in 0..SECTION_SIDE {
                for z in 0..SECTION_SIDE {
                    let row = padded_index(1, y + 1, z + 1);
                    self.fill(row..row + SECTION_SIDE, only);
                }
            }
            return;
        }
        match bits {
            1 => self.decode::<1>(voxels.words()),
            2 => self.decode::<2>(voxels.words()),
            3 => self.decode::<3>(voxels.words()),
            4 => self.decode::<4>(voxels.words()),
            5 => self.decode::<5>(voxels.words()),
            6 => self.decode::<6>(voxels.words()),
            7 => self.decode::<7>(voxels.words()),
            8 => self.decode::<8>(voxels.words()),
            _ => self.decode_wide(bits, voxels.words()),
        }
    }

    /// Decodes `BITS`-bit packed indices through the palette into the padded interior,
    /// with the index count per packed word known at compile time.
    fn decode<const BITS: usize>(&mut self, words: &[u64]) {
        let per_word = PACKED_WORD_BITS / BITS;
        let full = SECTION_VOXELS / per_word;
        let palette = &self.palette[..];
        let (out_words, out_flags) = (&mut self.words[..], &mut self.flags[..]);
        let mut put = |voxel: usize, index: u64| {
            let (value, flag) = palette[index as usize];
            let at = interior_index(voxel);
            out_words[at] = value;
            out_flags[at] = flag;
        };
        let mask = (1_u64 << BITS) - 1;
        for (number, &word) in words[..full].iter().enumerate() {
            let first = number * per_word;
            for slot in 0..per_word {
                put(first + slot, word >> (slot * BITS) & mask);
            }
        }
        if let Some(&word) = words.get(full) {
            for slot in 0..SECTION_VOXELS - full * per_word {
                put(full * per_word + slot, word >> (slot * BITS) & mask);
            }
        }
    }

    /// [`Self::decode`] for palettes wider than eight bits, which sections rarely need.
    fn decode_wide(&mut self, bits: usize, words: &[u64]) {
        let per_word = PACKED_WORD_BITS / bits;
        let mask = (1_u64 << bits) - 1;
        for voxel in 0..SECTION_VOXELS {
            let index = words[voxel / per_word] >> (voxel % per_word * bits) & mask;
            let (value, flag) = self.palette[index as usize];
            let at = interior_index(voxel);
            self.words[at] = value;
            self.flags[at] = flag;
        }
    }

    /// Writes the plane of `neighbour` touching `face` into the padding across it.
    fn gather_slab(&mut self, face: Face, neighbour: Option<&PackedVoxels>, classes: &MeshClasses) {
        let Some(voxels) = neighbour.filter(|voxels| voxels.uniform_voxel().is_none()) else {
            let only = neighbour.map_or(Voxel::air(FALLBACK_LIGHT), |voxels| {
                voxels.uniform_voxel().expect("uniform")
            });
            let entry = (only.bits(), classes.of_word(only.bits()));
            self.fill_slab(face, |_| entry);
            return;
        };
        self.palette.clear();
        self.palette.extend(voxels.palette().iter().map(|voxel| {
            let word = voxel.bits();
            (word, classes.of_word(word))
        }));
        let words = voxels.words();
        match usize::from(voxels.bits_per_index()) {
            1 => self.slab::<1>(face, words),
            2 => self.slab::<2>(face, words),
            3 => self.slab::<3>(face, words),
            4 => self.slab::<4>(face, words),
            5 => self.slab::<5>(face, words),
            6 => self.slab::<6>(face, words),
            7 => self.slab::<7>(face, words),
            8 => self.slab::<8>(face, words),
            bits => {
                let per_word = PACKED_WORD_BITS / bits;
                let mask = (1_u64 << bits) - 1;
                let palette = std::mem::take(&mut self.palette);
                self.fill_slab(face, |index| {
                    palette[(words[index / per_word] >> (index % per_word * bits) & mask) as usize]
                });
                self.palette = palette;
            }
        }
    }

    /// Writes the plane touching `face` of a neighbour packed at `BITS` bits per index,
    /// through the palette table this mesh just built, into the padding across `face`.
    fn slab<const BITS: usize>(&mut self, face: Face, words: &[u64]) {
        let per_word = PACKED_WORD_BITS / BITS;
        let mask = (1_u64 << BITS) - 1;
        let palette = std::mem::take(&mut self.palette);
        self.fill_slab(face, |index| {
            palette[(words[index / per_word] >> (index % per_word * BITS) & mask) as usize]
        });
        self.palette = palette;
    }

    /// Writes `entry(index)`, the word and class of the neighbour voxel at linear
    /// [`voxel_index`] `index` on the plane touching `face`, into the padding across it.
    fn fill_slab(&mut self, face: Face, entry: impl Fn(usize) -> (u32, u8)) {
        let shell = if face.is_positive() { PAD - 1 } else { 0 };
        let touching = face.opposite();
        for v in 0..SECTION_SIDE {
            for u in 0..SECTION_SIDE {
                let [x, y, z] = face.voxel_at([u, v]);
                let mut at = [x + 1, y + 1, z + 1];
                at[face.axis()] = shell;
                let [nx, ny, nz] = touching.voxel_at([u, v]);
                let (word, flag) = entry(voxel_index(nx, ny, nz));
                let index = padded_index(at[0], at[1], at[2]);
                self.words[index] = word;
                self.flags[index] = flag;
            }
        }
    }

    /// Packs each padded column's class bytes into one mask per class, eight bytes at a
    /// time: a multiply gathers bit `c` of eight bytes into one byte.
    fn build_masks(&mut self) {
        const ONES: u64 = 0x0101_0101_0101_0101;
        const GATHER: u64 = 0x0102_0408_1020_4080;
        let Masks {
            air,
            solid,
            water,
            see_through,
            wet,
        } = &mut self.masks;
        for column in 0..PAD2 {
            let row = column * PAD;
            let mut bits = [0_u64; CLASS_BITS as usize];
            for chunk in 0..PAD.div_ceil(8) {
                let start = row + 8 * chunk;
                let bytes: [u8; 8] = self.flags[start..start + 8]
                    .try_into()
                    .expect("eight flag bytes");
                let word = u64::from_le_bytes(bytes);
                for (class, out) in bits.iter_mut().enumerate() {
                    let lanes = (word >> class) & ONES;
                    *out |= (lanes.wrapping_mul(GATHER) >> 56) << (8 * chunk);
                }
            }
            air[column] = bits[AIR.trailing_zeros() as usize] & COLUMN_BITS;
            solid[column] = bits[SOLID.trailing_zeros() as usize] & COLUMN_BITS;
            water[column] = bits[WATER.trailing_zeros() as usize] & COLUMN_BITS;
            see_through[column] = bits[SEE_THROUGH.trailing_zeros() as usize] & COLUMN_BITS;
            wet[column] = bits[WET.trailing_zeros() as usize] & COLUMN_BITS;
        }
    }
}
