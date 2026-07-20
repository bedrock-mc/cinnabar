use std::sync::Arc;

use world::{BLOCKS_PER_SUB_CHUNK, BiomeStorage};

const HEADER_WORDS: usize = 1;
const BITS_MASK: u32 = 0xff;
const PALETTE_LEN_SHIFT: u32 = 8;
const PALETTE_LEN_MASK: u32 = 0x1fff;
const DESCRIPTOR_MAGIC: u32 = 0x4249_4f31;
const DESCRIPTOR_WORDS: usize = 2 + BIOME_NEIGHBOUR_SLOT_COUNT;
const NO_UNIFORM_TINT: u32 = u32::MAX;
const FALLBACK_WORDS: [u32; 13] = [
    DESCRIPTOR_MAGIC,
    0,
    0,
    0,
    0,
    0,
    DESCRIPTOR_WORDS as u32,
    0,
    0,
    0,
    0,
    1 << PALETTE_LEN_SHIFT,
    0,
];

/// Immutable identity for the biome tint table referenced by packed records.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct ChunkBiomeTintIdentity {
    stream: u64,
    revision: u64,
}

impl ChunkBiomeTintIdentity {
    #[must_use]
    pub const fn new(stream: u64, revision: u64) -> Self {
        Self { stream, revision }
    }

    #[must_use]
    pub const fn stream(self) -> u64 {
        self.stream
    }

    #[must_use]
    pub const fn revision(self) -> u64 {
        self.revision
    }
}

/// Number of horizontal biome storages retained by one render record.
pub const BIOME_NEIGHBOUR_SLOT_COUNT: usize = 9;

/// Provisional horizontal tint-sampling radius used by the bounded renderer.
///
/// The data sources pin exact Bedrock biome identities and colours but do not
/// publish the native renderer kernel. Keep this explicit until the native
/// abrupt-boundary acceptance witness adjudicates it.
pub const BIOME_BLEND_RADIUS: i32 = 1;

/// Number of samples in the fixed radius-one horizontal tint kernel.
pub const BIOME_BLEND_SAMPLE_COUNT: usize = BIOME_NEIGHBOUR_SLOT_COUNT;

/// Exact denominator for the equal-weight provisional kernel.
pub const BIOME_BLEND_WEIGHT_DENOMINATOR: u16 = BIOME_BLEND_SAMPLE_COUNT as u16;

/// One allocation-free palette lookup contributing to a biome tint blend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiomeBlendSample {
    pub offset: [i8; 2],
    pub tint_index: u32,
    pub weight_numerator: u8,
}

/// Absolute encoded-word ceiling for one self-contained halo record.
pub const MAX_PACKED_BIOME_RECORD_WORDS: usize =
    DESCRIPTOR_WORDS + BIOME_NEIGHBOUR_SLOT_COUNT * (1 + 2_048 + BLOCKS_PER_SUB_CHUNK);

/// Maps a horizontal subchunk offset in `-1..=1` to the stable descriptor slot.
#[must_use]
pub const fn biome_neighbour_index(dx: i8, dz: i8) -> Option<usize> {
    if dx < -1 || dx > 1 || dz < -1 || dz > 1 {
        return None;
    }
    Some(((dz + 1) as usize) * 3 + (dx + 1) as usize)
}

const CENTER_SLOT: usize = 4;

/// Palette-native biome data prepared for one GPU sub-chunk record.
///
/// A fixed 3x3 horizontal descriptor precedes deduplicated Bedrock packed
/// storages. Only palette entries are remapped from wire biome IDs to dense
/// tint indices; no 4,096-entry biome or colour array is materialized.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PackedBiomeRecord {
    words: Arc<[u32]>,
}

impl PackedBiomeRecord {
    /// Builds a center-only record. Missing neighbours clamp to its nearest edge.
    #[must_use]
    pub fn from_storage(
        storage: &BiomeStorage,
        resolve_tint_index: impl FnMut(u32) -> u32,
    ) -> Self {
        let mut halo = std::array::from_fn(|_| None);
        halo[CENTER_SLOT] = Some(Arc::new(storage.clone()));
        Self::from_neighbourhood(&halo, resolve_tint_index)
    }

    /// Builds a self-contained 3x3 horizontal record from immutable snapshots.
    ///
    /// Identical packed payloads are emitted once and referenced by relative
    /// descriptor offsets. The center slot is required; an absent center yields
    /// the bounded fallback record.
    #[must_use]
    pub fn from_neighbourhood(
        storages: &[Option<Arc<BiomeStorage>>; BIOME_NEIGHBOUR_SLOT_COUNT],
        mut resolve_tint_index: impl FnMut(u32) -> u32,
    ) -> Self {
        if storages[CENTER_SLOT].is_none() {
            return Self::fallback();
        }

        let mut payloads = Vec::<Vec<u32>>::new();
        let mut slots = [None; BIOME_NEIGHBOUR_SLOT_COUNT];
        let mut uniform_tint = None;
        let mut all_uniform = true;
        for (slot, storage) in storages.iter().enumerate() {
            let Some(storage) = storage else {
                continue;
            };
            let payload = packed_storage_words(storage, &mut resolve_tint_index);
            let payload_uniform = packed_payload_uniform_tint(&payload);
            match (uniform_tint, payload_uniform) {
                (None, Some(value)) => uniform_tint = Some(value),
                (Some(current), Some(value)) if current == value => {}
                _ => all_uniform = false,
            }
            let payload_index = payloads
                .iter()
                .position(|existing| existing == &payload)
                .unwrap_or_else(|| {
                    payloads.push(payload);
                    payloads.len() - 1
                });
            slots[slot] = Some(payload_index);
        }

        let mut payload_offsets = Vec::with_capacity(payloads.len());
        let mut next_offset = DESCRIPTOR_WORDS;
        for payload in &payloads {
            payload_offsets.push(
                u32::try_from(next_offset).expect("bounded biome records fit relative u32 offsets"),
            );
            next_offset = next_offset
                .checked_add(payload.len())
                .expect("bounded biome record word count cannot overflow usize");
        }

        let mut words = Vec::with_capacity(next_offset);
        words.push(DESCRIPTOR_MAGIC);
        words.push(if all_uniform {
            uniform_tint.unwrap_or(0)
        } else {
            NO_UNIFORM_TINT
        });
        words.extend(slots.map(|slot| slot.map_or(0, |index| payload_offsets[index])));
        for payload in payloads {
            words.extend(payload);
        }
        debug_assert!(words.len() <= MAX_PACKED_BIOME_RECORD_WORDS);
        Self {
            words: words.into(),
        }
    }

    /// Uniform fallback record referencing tint-table entry zero.
    #[must_use]
    pub fn fallback() -> Self {
        Self {
            words: Arc::from(FALLBACK_WORDS),
        }
    }

    #[must_use]
    pub fn is_fallback(&self) -> bool {
        self.words.as_ref() == FALLBACK_WORDS
    }

    /// Exact storage-buffer words: descriptor then deduplicated packed records.
    #[must_use]
    pub fn words(&self) -> &[u32] {
        &self.words
    }

    #[must_use]
    pub fn bits_per_index(&self) -> u8 {
        (self.center_payload()[0] & BITS_MASK) as u8
    }

    #[must_use]
    pub fn palette_len(&self) -> usize {
        ((self.center_payload()[0] >> PALETTE_LEN_SHIFT) & PALETTE_LEN_MASK) as usize
    }

    #[must_use]
    pub fn byte_len(&self) -> u64 {
        self.words.len() as u64 * std::mem::size_of::<u32>() as u64
    }

    /// Returns the common dense tint index for a uniform 3x3 neighbourhood.
    #[must_use]
    pub fn uniform_tint_index(&self) -> Option<u32> {
        (self.words[1] != NO_UNIFORM_TINT).then_some(self.words[1])
    }

    /// CPU mirror of the shader's packed lookup at a center-local coordinate.
    /// Coordinates may extend into one horizontal neighbour. Missing slots
    /// clamp to the nearest center edge rather than introducing fallback seams.
    #[must_use]
    pub fn tint_index_at(&self, coordinate: [i32; 3]) -> Option<u32> {
        if !(0..16).contains(&coordinate[1]) {
            return None;
        }
        let dx = coordinate[0].div_euclid(16);
        let dz = coordinate[2].div_euclid(16);
        let slot = biome_neighbour_index(i8::try_from(dx).ok()?, i8::try_from(dz).ok()?)?;
        let relative = self.words[2 + slot];
        let (payload, x, z) = if relative == 0 {
            (
                self.center_payload(),
                coordinate[0].clamp(0, 15) as u8,
                coordinate[2].clamp(0, 15) as u8,
            )
        } else {
            (
                self.payload_at(relative)?,
                coordinate[0].rem_euclid(16) as u8,
                coordinate[2].rem_euclid(16) as u8,
            )
        };
        packed_payload_tint_index(payload, x, coordinate[1] as u8, z)
    }

    /// Exact fixed-size kernel samples in stable Z-major order.
    #[must_use]
    pub fn blend_samples(
        &self,
        coordinate: [i32; 3],
    ) -> Option<[BiomeBlendSample; BIOME_BLEND_SAMPLE_COUNT]> {
        let mut samples = [BiomeBlendSample {
            offset: [0, 0],
            tint_index: 0,
            weight_numerator: 1,
        }; BIOME_BLEND_SAMPLE_COUNT];
        for dz in -BIOME_BLEND_RADIUS..=BIOME_BLEND_RADIUS {
            for dx in -BIOME_BLEND_RADIUS..=BIOME_BLEND_RADIUS {
                let dx = i8::try_from(dx).ok()?;
                let dz = i8::try_from(dz).ok()?;
                let slot = biome_neighbour_index(dx, dz)?;
                samples[slot] = BiomeBlendSample {
                    offset: [dx, dz],
                    tint_index: self.tint_index_at([
                        coordinate[0] + i32::from(dx),
                        coordinate[1],
                        coordinate[2] + i32::from(dz),
                    ])?,
                    weight_numerator: 1,
                };
            }
        }
        Some(samples)
    }

    /// Nine radius-1 box-kernel tint indices in stable Z-major order.
    #[must_use]
    pub fn blend_tint_indices(&self, coordinate: [i32; 3]) -> Option<[u32; 9]> {
        Some(
            self.blend_samples(coordinate)?
                .map(|sample| sample.tint_index),
        )
    }

    /// Center-local lookup retained for existing validation callers.
    #[must_use]
    pub fn tint_index(&self, x: u8, y: u8, z: u8) -> Option<u32> {
        if x >= 16 || y >= 16 || z >= 16 {
            return None;
        }
        self.tint_index_at([i32::from(x), i32::from(y), i32::from(z)])
    }

    fn center_payload(&self) -> &[u32] {
        self.payload_at(self.words[2 + CENTER_SLOT])
            .expect("PackedBiomeRecord always contains a validated center payload")
    }

    fn payload_at(&self, relative: u32) -> Option<&[u32]> {
        let start = usize::try_from(relative).ok()?;
        let header = *self.words.get(start)?;
        let bits = usize::try_from(header & BITS_MASK).ok()?;
        let palette_len = usize::try_from((header >> PALETTE_LEN_SHIFT) & PALETTE_LEN_MASK).ok()?;
        let word_count = packed_word_count(bits)?;
        let end = start.checked_add(HEADER_WORDS + word_count + palette_len)?;
        self.words.get(start..end)
    }
}

fn packed_storage_words(
    storage: &BiomeStorage,
    resolve_tint_index: &mut impl FnMut(u32) -> u32,
) -> Vec<u32> {
    let palette_len = storage.palette().len();
    debug_assert!((1..=BLOCKS_PER_SUB_CHUNK).contains(&palette_len));
    let header = u32::from(storage.bits_per_index())
        | (u32::try_from(palette_len).expect("biome palettes are bounded") << PALETTE_LEN_SHIFT);
    let mut words =
        Vec::with_capacity(HEADER_WORDS + storage.packed_words().len() + storage.palette().len());
    words.push(header);
    words.extend_from_slice(storage.packed_words());
    words.extend(
        storage
            .palette()
            .values()
            .iter()
            .copied()
            .map(resolve_tint_index),
    );
    words
}

fn packed_word_count(bits: usize) -> Option<usize> {
    if bits == 0 {
        return Some(0);
    }
    let values_per_word = 32_usize.checked_div(bits)?;
    Some(BLOCKS_PER_SUB_CHUNK.div_ceil(values_per_word))
}

fn packed_payload_uniform_tint(payload: &[u32]) -> Option<u32> {
    let header = *payload.first()?;
    let bits = usize::try_from(header & BITS_MASK).ok()?;
    let palette_len = usize::try_from((header >> PALETTE_LEN_SHIFT) & PALETTE_LEN_MASK).ok()?;
    let palette_start = HEADER_WORDS.checked_add(packed_word_count(bits)?)?;
    let palette = payload.get(palette_start..palette_start.checked_add(palette_len)?)?;
    let first = *palette.first()?;
    palette.iter().all(|&entry| entry == first).then_some(first)
}

fn packed_payload_tint_index(payload: &[u32], x: u8, y: u8, z: u8) -> Option<u32> {
    let header = *payload.first()?;
    let bits = usize::try_from(header & BITS_MASK).ok()?;
    let palette_len = usize::try_from((header >> PALETTE_LEN_SHIFT) & PALETTE_LEN_MASK).ok()?;
    let packed_word_count = packed_word_count(bits)?;
    let linear = (usize::from(x) << 8) | (usize::from(z) << 4) | usize::from(y);
    let palette_index = if bits == 0 {
        0
    } else {
        let values_per_word = 32 / bits;
        let word = *payload.get(HEADER_WORDS + linear / values_per_word)?;
        let shift = (linear % values_per_word) * bits;
        let mask = (1_u32 << bits) - 1;
        ((word >> shift) & mask) as usize
    };
    if palette_index >= palette_len {
        return None;
    }
    payload
        .get(HEADER_WORDS + packed_word_count + palette_index)
        .copied()
}

impl Default for PackedBiomeRecord {
    fn default() -> Self {
        Self::fallback()
    }
}
