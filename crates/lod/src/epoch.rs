//! World epochs: telling different worlds behind one server identity apart by their terrain.
//!
//! A store keeps a 16-bit signature of the solid surface (top Y and material, water
//! ignored) of every block column it has seen, grouped by 64×64-block [`RegionSignature`].
//! A fresh region is compared with the stored one on [`SAMPLED_COLUMNS`] columns drawn at
//! random from the columns both have recorded, as in the epoch measurements. A region
//! abstains when either side's samples share one signature, so flat lobbies and void
//! arenas never vote. An [`EpochVote`] then merges the session into a stored epoch only on
//! a clear lead of matching regions and splits it off only on a much clearer lead of
//! mismatching ones, staying a candidate in between.

use std::num::NonZeroU16;

use world::SUB_CHUNK_SIDE;

use crate::{
    classify::{BlockClass, MaterialClasses, voxel_class},
    geometry::SECTION_SIDE,
    sampler::ColumnSample,
    voxel::MaterialId,
};

/// Blocks along each side of a chunk column.
pub const CHUNK_SIDE: usize = SUB_CHUNK_SIDE;
/// Block columns of one chunk.
pub const CHUNK_COLUMNS: usize = CHUNK_SIDE * CHUNK_SIDE;
/// Chunks along each side of a signature region: 64 blocks, the size the epoch
/// measurements used.
pub const REGION_SIDE_CHUNKS: i32 = 4;
const REGION_CHUNKS: usize = (REGION_SIDE_CHUNKS * REGION_SIDE_CHUNKS) as usize;
/// Block columns of one region.
pub const REGION_COLUMNS: usize = REGION_CHUNKS * CHUNK_COLUMNS;
/// Fewest columns both sides must have recorded for a region to vote; the measurements
/// only scored regions with this much overlap.
pub const MIN_OVERLAPPING_COLUMNS: usize = 1024;
const _: () = assert!(MIN_OVERLAPPING_COLUMNS <= REGION_COLUMNS);
/// Columns drawn per comparison, with replacement; 32 at [`MIN_AGREEMENT_PERCENT`] made no
/// wrong decision in 20,000 trials each way on real terrain.
pub const SAMPLED_COLUMNS: usize = 32;
/// Least share of drawn columns with equal signatures for a region to vote "match".
pub const MIN_AGREEMENT_PERCENT: usize = 50;
/// Weight of each opposing vote in the merge and split margins.
pub const OPPOSING_VOTE_WEIGHT: i64 = 2;
/// Merge once `matching - 2 × mismatching` exceeds this.
pub const MERGE_MARGIN: i64 = 10;
/// Split once `mismatching - 2 × matching` exceeds this.
pub const SPLIT_MARGIN: i64 = 100;

/// The solid surface of one block column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SurfaceColumn {
    pub top_y: i32,
    pub material: MaterialId,
}

impl SurfaceColumn {
    /// The persisted 16-bit signature of this surface. Unequal surfaces collide with
    /// probability about 2⁻¹⁶, which raises a different world's agreement by a negligible
    /// amount next to the 50% threshold. The mix is part of the store format.
    #[must_use]
    pub const fn signature(self) -> ColumnSignature {
        let mut value = (self.top_y as u32).wrapping_mul(0x9e37_79b9) ^ self.material.raw();
        value ^= value >> 16;
        value = value.wrapping_mul(0x7feb_352d);
        value ^= value >> 15;
        value = value.wrapping_mul(0x846c_a68b);
        value ^= value >> 16;
        let folded = (value ^ value >> 16) as u16;
        match NonZeroU16::new(folded) {
            Some(bits) => ColumnSignature(bits),
            None => ColumnSignature(NonZeroU16::MIN),
        }
    }
}

/// A column's persisted surface signature; see [`SurfaceColumn::signature`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ColumnSignature(NonZeroU16);

impl ColumnSignature {
    /// The stored form, never zero.
    #[must_use]
    pub const fn bits(self) -> u16 {
        self.0.get()
    }

    /// Reads a stored form; `None` for zero, which no signature uses.
    #[must_use]
    pub const fn from_bits(bits: u16) -> Option<Self> {
        match NonZeroU16::new(bits) {
            Some(bits) => Some(Self(bits)),
            None => None,
        }
    }
}

/// Surface signatures of a chunk's block columns, X fastest; `None` where a column has
/// no non-water block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkSurface {
    columns: [Option<ColumnSignature>; CHUNK_COLUMNS],
}

impl Default for ChunkSurface {
    fn default() -> Self {
        Self {
            columns: [None; CHUNK_COLUMNS],
        }
    }
}

impl ChunkSurface {
    /// The surfaces of a sampled column: the highest voxel that is neither air nor water.
    /// Leaves and see-through blocks count; water is skipped, so the seabed stands for
    /// the sea as in the epoch measurements.
    #[must_use]
    pub fn from_sample(sample: &ColumnSample, classes: &impl MaterialClasses) -> Self {
        let mut tops: [Option<SurfaceColumn>; CHUNK_COLUMNS] = [None; CHUNK_COLUMNS];
        for sub in &sample.sub_chunks {
            let base_y =
                sub.section().y() * SECTION_SIDE as i32 + (sub.octant().y() * CHUNK_SIDE) as i32;
            for z in 0..CHUNK_SIDE {
                for x in 0..CHUNK_SIDE {
                    let slot = &mut tops[column(x, z)];
                    for y in (0..CHUNK_SIDE).rev() {
                        let top_y = base_y + y as i32;
                        if slot.is_some_and(|column| column.top_y >= top_y) {
                            break;
                        }
                        let voxel = sub.voxel(x, y, z);
                        let class = voxel_class(classes, voxel);
                        if class != BlockClass::Air && class != BlockClass::Water {
                            *slot = Some(SurfaceColumn {
                                top_y,
                                material: voxel.material(),
                            });
                            break;
                        }
                    }
                }
            }
        }
        Self {
            columns: tops.map(|top| top.map(SurfaceColumn::signature)),
        }
    }

    /// The signature of block column `(x, z)`; panics outside `0..16`.
    #[must_use]
    pub fn get(&self, x: usize, z: usize) -> Option<ColumnSignature> {
        self.columns[column(x, z)]
    }

    /// Sets block column `(x, z)`; panics outside `0..16`.
    pub fn set(&mut self, x: usize, z: usize, signature: Option<ColumnSignature>) {
        self.columns[column(x, z)] = signature;
    }
}

/// Index of block column `(x, z)` in [`ChunkSurface`] order; panics outside `0..16`.
fn column(x: usize, z: usize) -> usize {
    assert!(x < CHUNK_SIDE && z < CHUNK_SIDE, "column ({x}, {z})");
    z * CHUNK_SIDE + x
}

/// A signature region's position in units of [`REGION_SIDE_CHUNKS`] chunks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RegionPos {
    pub x: i32,
    pub z: i32,
}

impl RegionPos {
    /// The region holding chunk `(chunk_x, chunk_z)`.
    #[must_use]
    pub const fn of_chunk(chunk_x: i32, chunk_z: i32) -> Self {
        Self {
            x: chunk_x.div_euclid(REGION_SIDE_CHUNKS),
            z: chunk_z.div_euclid(REGION_SIDE_CHUNKS),
        }
    }

    /// Index of a chunk of this region, or `None` for chunks outside it.
    fn chunk_slot(self, chunk_x: i32, chunk_z: i32) -> Option<usize> {
        (Self::of_chunk(chunk_x, chunk_z) == self).then(|| {
            let local_x = chunk_x.rem_euclid(REGION_SIDE_CHUNKS);
            let local_z = chunk_z.rem_euclid(REGION_SIDE_CHUNKS);
            (local_z * REGION_SIDE_CHUNKS + local_x) as usize
        })
    }
}

/// Bytes of one recorded chunk in an encoded [`RegionSignature`].
pub const ENCODED_CHUNK_BYTES: usize = CHUNK_COLUMNS * 2;
/// Longest encoded [`RegionSignature`]: the chunk mask and every chunk.
pub const MAX_ENCODED_SIGNATURE_BYTES: usize = 2 + REGION_CHUNKS * ENCODED_CHUNK_BYTES;
const _: () = assert!(REGION_CHUNKS <= u16::BITS as usize);

/// Why an encoded [`RegionSignature`] was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SignatureError {
    #[error("encoded signature has {0} bytes, less than its chunk mask")]
    Truncated(usize),
    #[error("encoded signature has {actual} bytes; its chunk mask needs {expected}")]
    Length { expected: usize, actual: usize },
}

/// The recorded column signatures of one region's chunks: 2 bytes per column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionSignature {
    pos: RegionPos,
    chunks: Box<[Option<ChunkSurface>; REGION_CHUNKS]>,
}

impl RegionSignature {
    /// A region with no recorded chunks.
    #[must_use]
    pub fn new(pos: RegionPos) -> Self {
        Self {
            pos,
            chunks: Box::new(std::array::from_fn(|_| None)),
        }
    }

    /// The region these chunks belong to.
    #[must_use]
    pub fn pos(&self) -> RegionPos {
        self.pos
    }

    /// The recorded surface of a chunk; `None` when unrecorded or outside the region.
    #[must_use]
    pub fn chunk(&self, chunk_x: i32, chunk_z: i32) -> Option<&ChunkSurface> {
        let slot = self.pos.chunk_slot(chunk_x, chunk_z)?;
        self.chunks[slot].as_ref()
    }

    /// Records a chunk's surface, replacing an older one. Returns false, recording
    /// nothing, for chunks outside the region.
    pub fn record(&mut self, chunk_x: i32, chunk_z: i32, surface: ChunkSurface) -> bool {
        let Some(slot) = self.pos.chunk_slot(chunk_x, chunk_z) else {
            return false;
        };
        self.chunks[slot] = Some(surface);
        true
    }

    /// Recorded chunks, as absolute chunk coordinates, in slot order.
    pub fn recorded_chunks(&self) -> impl Iterator<Item = (i32, i32, &ChunkSurface)> + '_ {
        self.chunks.iter().enumerate().filter_map(|(slot, chunk)| {
            let (local_x, local_z) = (
                slot as i32 % REGION_SIDE_CHUNKS,
                slot as i32 / REGION_SIDE_CHUNKS,
            );
            chunk.as_ref().map(|chunk| {
                (
                    self.pos.x * REGION_SIDE_CHUNKS + local_x,
                    self.pos.z * REGION_SIDE_CHUNKS + local_z,
                    chunk,
                )
            })
        })
    }

    /// True when no chunk is recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.iter().all(Option::is_none)
    }

    /// Records every chunk `newer` has recorded over this region's own. Panics when the
    /// signatures cover different regions.
    pub fn merge_from(&mut self, newer: &Self) {
        assert_eq!(self.pos, newer.pos, "signatures of different regions");
        for (own, newer) in self.chunks.iter_mut().zip(newer.chunks.iter()) {
            if newer.is_some() {
                own.clone_from(newer);
            }
        }
    }

    /// Appends the persisted form, part of the store format: a little-endian `u16` mask
    /// of recorded chunk slots, then each recorded chunk's column signatures as
    /// little-endian `u16`s in [`ChunkSurface`] order, zero where a column has none.
    pub fn encode_into(&self, out: &mut Vec<u8>) {
        let mask = self
            .chunks
            .iter()
            .enumerate()
            .filter(|(_, chunk)| chunk.is_some())
            .fold(0u16, |mask, (slot, _)| mask | 1 << slot);
        out.extend_from_slice(&mask.to_le_bytes());
        for chunk in self.chunks.iter().flatten() {
            for column in &chunk.columns {
                out.extend_from_slice(&column.map_or(0, ColumnSignature::bits).to_le_bytes());
            }
        }
    }

    /// Reads [`Self::encode_into`] output for the region at `pos`, rejecting any length
    /// other than the one its chunk mask implies.
    pub fn decode(pos: RegionPos, bytes: &[u8]) -> Result<Self, SignatureError> {
        let Some((mask, columns)) = bytes.split_first_chunk::<2>() else {
            return Err(SignatureError::Truncated(bytes.len()));
        };
        let mask = u16::from_le_bytes(*mask);
        let expected = 2 + mask.count_ones() as usize * ENCODED_CHUNK_BYTES;
        if bytes.len() != expected {
            return Err(SignatureError::Length {
                expected,
                actual: bytes.len(),
            });
        }
        let mut signature = Self::new(pos);
        let mut encoded = columns.as_chunks::<ENCODED_CHUNK_BYTES>().0.iter();
        for (slot, chunk) in signature.chunks.iter_mut().enumerate() {
            if mask & 1 << slot == 0 {
                continue;
            }
            let bytes = encoded.next().expect("the length check covers every chunk");
            let mut surface = ChunkSurface::default();
            for (column, bits) in surface.columns.iter_mut().zip(bytes.as_chunks::<2>().0) {
                *column = ColumnSignature::from_bits(u16::from_le_bytes(*bits));
            }
            *chunk = Some(surface);
        }
        Ok(signature)
    }

    /// The signature of every block column, chunk slot major; `None` where unrecorded.
    fn columns(&self) -> impl Iterator<Item = Option<ColumnSignature>> + '_ {
        self.chunks.iter().flat_map(|chunk| {
            (0..CHUNK_COLUMNS).map(move |index| chunk.as_ref().and_then(|c| c.columns[index]))
        })
    }
}

/// One region's comparison with its stored signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionComparison {
    /// Fewer than [`MIN_OVERLAPPING_COLUMNS`] columns are recorded on both sides; abstains.
    TooFewColumns,
    /// Either side's drawn columns share one signature, as on flat or void maps; abstains.
    Uniform,
    /// At least [`MIN_AGREEMENT_PERCENT`] of the [`SAMPLED_COLUMNS`] drawn columns agree.
    Match { matching: u32 },
    /// Fewer drawn columns agree.
    Mismatch { matching: u32 },
}

/// Compares two signatures of the same region on [`SAMPLED_COLUMNS`] columns drawn with
/// replacement from those both have recorded. `seed` picks the draw; callers vary it per
/// comparison so repeated checks of one region do not reuse one draw. Panics when the
/// signatures cover different regions.
#[must_use]
pub fn compare_region(
    stored: &RegionSignature,
    fresh: &RegionSignature,
    seed: u64,
) -> RegionComparison {
    assert_eq!(stored.pos, fresh.pos, "signatures of different regions");
    let overlap: Vec<(ColumnSignature, ColumnSignature)> = stored
        .columns()
        .zip(fresh.columns())
        .filter_map(|(stored, fresh)| Some((stored?, fresh?)))
        .collect();
    if overlap.len() < MIN_OVERLAPPING_COLUMNS {
        return RegionComparison::TooFewColumns;
    }
    let mut draw = SplitMix64(seed);
    let pairs: [(ColumnSignature, ColumnSignature); SAMPLED_COLUMNS] =
        std::array::from_fn(|_| overlap[draw.below(overlap.len())]);
    let uniform = |side: fn(&(ColumnSignature, ColumnSignature)) -> ColumnSignature| {
        pairs.iter().all(|pair| side(pair) == side(&pairs[0]))
    };
    if uniform(|pair| pair.0) || uniform(|pair| pair.1) {
        return RegionComparison::Uniform;
    }
    agreement(
        pairs
            .iter()
            .filter(|(stored, fresh)| stored == fresh)
            .count(),
    )
}

/// The verdict for `matching` equal columns out of [`SAMPLED_COLUMNS`].
fn agreement(matching: usize) -> RegionComparison {
    let votes = matching as u32;
    if matching * 100 >= MIN_AGREEMENT_PERCENT * SAMPLED_COLUMNS {
        RegionComparison::Match { matching: votes }
    } else {
        RegionComparison::Mismatch { matching: votes }
    }
}

/// SplitMix64: a small, stable generator for reproducible column draws.
struct SplitMix64(u64);

impl SplitMix64 {
    /// A value in `0..bound`; `bound` must be non-zero. The modulo bias is below 2⁻⁴⁸
    /// for region-sized bounds.
    fn below(&mut self, bound: usize) -> usize {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ value >> 30).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ value >> 27).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^= value >> 31;
        (value % bound as u64) as usize
    }
}

/// Where a session stands against one stored epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EpochDecision {
    /// Not enough evidence yet; keep the session's data separate.
    Candidate,
    /// The session is the stored epoch's world.
    Merge,
    /// The session is a different world.
    Split,
}

/// Votes of one session against one stored epoch. The first decision is final.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpochVote {
    matching: u32,
    mismatching: u32,
    decision: EpochDecision,
}

impl Default for EpochVote {
    fn default() -> Self {
        Self {
            matching: 0,
            mismatching: 0,
            decision: EpochDecision::Candidate,
        }
    }
}

impl EpochVote {
    /// A session with no votes.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Counts one region's comparison; abstentions change nothing.
    pub fn record(&mut self, comparison: RegionComparison) -> EpochDecision {
        match comparison {
            RegionComparison::Match { .. } => self.add(1, 0),
            RegionComparison::Mismatch { .. } => self.add(0, 1),
            RegionComparison::TooFewColumns | RegionComparison::Uniform => self.decision,
        }
    }

    /// Adds caller-counted matching and mismatching region votes and returns the decision.
    pub fn add(&mut self, matching: u32, mismatching: u32) -> EpochDecision {
        self.matching = self.matching.saturating_add(matching);
        self.mismatching = self.mismatching.saturating_add(mismatching);
        if self.decision == EpochDecision::Candidate {
            self.decision = decide(self.matching, self.mismatching);
        }
        self.decision
    }

    /// The current decision; final once it is not a candidate.
    #[must_use]
    pub fn decision(&self) -> EpochDecision {
        self.decision
    }

    /// Matching region votes so far.
    #[must_use]
    pub fn matching(&self) -> u32 {
        self.matching
    }

    /// Mismatching region votes so far.
    #[must_use]
    pub fn mismatching(&self) -> u32 {
        self.mismatching
    }
}

/// The asymmetric rule: merging needs a small clear lead, splitting a large one.
#[must_use]
pub fn decide(matching: u32, mismatching: u32) -> EpochDecision {
    let (matching, mismatching) = (i64::from(matching), i64::from(mismatching));
    if matching - OPPOSING_VOTE_WEIGHT * mismatching > MERGE_MARGIN {
        EpochDecision::Merge
    } else if mismatching - OPPOSING_VOTE_WEIGHT * matching > SPLIT_MARGIN {
        EpochDecision::Split
    } else {
        EpochDecision::Candidate
    }
}

#[cfg(test)]
mod tests;
