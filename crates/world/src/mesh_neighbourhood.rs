use std::{collections::BTreeMap, sync::Arc};

use crate::SubChunk;

const WIDTH: usize = 3;
const SUB_CHUNK_SIDE: i32 = crate::SUB_CHUNK_SIDE as i32;
const ENTRY_COUNT: usize = WIDTH * WIDTH * WIDTH;
mod offsets;
use offsets::ADJACENT_OFFSETS;
pub(crate) use offsets::LIQUID_SAMPLE_OFFSETS;

/// One palette-native block sample from a bounded meshing snapshot.
///
/// Missing adjacent sub-chunks and absent storage layers are deliberately
/// represented as open space. Callers never need to invent a runtime ID for
/// an unavailable boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeshSample {
    Block(u32),
    Open,
}

/// Asset-derived cross-subchunk dependencies for one mesh generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MeshDependencyMask {
    pub diagonal_ao: bool,
    pub liquid: bool,
    /// Seasonal leaves read shelter in their full vertical chunk column.
    pub seasonal_foliage: bool,
}

impl MeshDependencyMask {
    #[must_use]
    pub const fn new(diagonal_ao: bool, liquid: bool) -> Self {
        Self {
            diagonal_ao,
            liquid,
            seasonal_foliage: false,
        }
    }

    #[must_use]
    pub const fn needs_diagonal_samples(self) -> bool {
        self.diagonal_ao || self.liquid
    }

    #[must_use]
    pub const fn with_seasonal_foliage(mut self, enabled: bool) -> Self {
        self.seasonal_foliage = enabled;
        self
    }
}

/// Center sub-chunk plus at most one adjacent sub-chunk in every direction.
///
/// References preserve the world's palette-packed representation. Coordinates
/// accepted by [`Self::sample`] span exactly `-16..=31` on each axis; anything
/// beyond that bounded 3x3x3 snapshot is explicit open space.
#[derive(Debug, Clone)]
pub struct MeshNeighbourhood<'a> {
    block_origin: [i32; 3],
    sub_chunks: [Option<&'a SubChunk>; ENTRY_COUNT],
    column_above: Vec<(i32, &'a SubChunk)>,
    shared_column: Option<(i32, &'a BTreeMap<i32, Arc<SubChunk>>)>,
}

impl<'a> MeshNeighbourhood<'a> {
    pub const ADJACENT_SUB_CHUNK_COUNT: usize = ADJACENT_OFFSETS.len();
    pub const LIQUID_SAMPLE_SUB_CHUNK_COUNT: usize = LIQUID_SAMPLE_OFFSETS.len();

    #[must_use]
    pub fn new(center: &'a SubChunk) -> Self {
        let mut sub_chunks = [None; ENTRY_COUNT];
        sub_chunks[index([0, 0, 0]).expect("center offset is bounded")] = Some(center);
        Self {
            block_origin: [0; 3],
            sub_chunks,
            column_above: Vec::new(),
            shared_column: None,
        }
    }

    /// World-space origin of the center sub-chunk, for native positional art.
    #[must_use]
    pub fn with_block_origin(mut self, origin: [i32; 3]) -> Self {
        self.block_origin = origin;
        self
    }

    #[must_use]
    pub const fn block_origin(&self) -> [i32; 3] {
        self.block_origin
    }

    /// Inserts one of the 26 adjacent sub-chunks. Returns false for an
    /// out-of-bounds offset or an attempt to replace the center.
    pub fn insert(&mut self, offset: [i8; 3], sub_chunk: &'a SubChunk) -> bool {
        if offset == [0, 0, 0] {
            return false;
        }
        let Some(index) = index(offset) else {
            return false;
        };
        self.sub_chunks[index] = Some(sub_chunk);
        true
    }

    #[must_use]
    pub fn sub_chunk(&self, offset: [i8; 3]) -> Option<&'a SubChunk> {
        self.sub_chunks.get(index(offset)?).copied().flatten()
    }

    /// Adds palette-packed seasonal shelter context beyond the ordinary AO
    /// halo. Offsets zero/one already belong to the center/upper halo.
    pub fn insert_column_above(&mut self, offset_y: i32, sub_chunk: &'a SubChunk) -> bool {
        if offset_y < 2
            || offset_y.checked_mul(SUB_CHUNK_SIDE).is_none()
            || self
                .column_above
                .iter()
                .any(|(existing, _)| *existing == offset_y)
        {
            return false;
        }
        self.column_above.push((offset_y, sub_chunk));
        true
    }

    /// Borrows an immutable column index; explicitly inserted upper sections take precedence.
    pub fn with_shared_column(
        mut self,
        center_y: i32,
        column: &'a BTreeMap<i32, Arc<SubChunk>>,
    ) -> Self {
        self.shared_column = Some((center_y, column));
        self
    }

    /// Center, upper neighbor, and explicitly captured higher sub-chunks.
    pub fn seasonal_column(&self) -> impl Iterator<Item = (i32, &'a SubChunk)> + '_ {
        [
            self.sub_chunk([0, 0, 0]).map(|chunk| (0, chunk)),
            self.sub_chunk([0, 1, 0]).map(|chunk| (1, chunk)),
        ]
        .into_iter()
        .flatten()
        .chain(self.column_above.iter().copied())
        .chain(
            self.shared_column
                .into_iter()
                .flat_map(move |(center, column)| {
                    column
                        .range(center.saturating_add(2)..)
                        .filter_map(move |(&y, section)| {
                            let offset = y.checked_sub(center)?;
                            (offset >= 2
                                && offset.checked_mul(SUB_CHUNK_SIDE).is_some()
                                && !self
                                    .column_above
                                    .iter()
                                    .any(|(explicit, _)| *explicit == offset))
                            .then_some((offset, section.as_ref()))
                        })
                }),
        )
    }

    /// Canonical offsets for all 26 adjacent sub-chunks used by diagonal AO.
    pub fn adjacent_offsets() -> impl ExactSizeIterator<Item = [i8; 3]> {
        ADJACENT_OFFSETS.into_iter()
    }

    /// Exact sub-chunk offsets needed by vanilla-like liquid surface meshing.
    ///
    /// The set is a horizontal 3x3 at the current and upper Y levels, plus
    /// the lower center and four lower cardinals used by bottom faces and
    /// downward flow. It is fixed, deduplicated, and deliberately smaller
    /// than the general 3x3x3 AO neighbourhood.
    pub fn liquid_sample_offsets() -> impl ExactSizeIterator<Item = [i8; 3]> {
        LIQUID_SAMPLE_OFFSETS.into_iter()
    }

    /// Returns every liquid sample slot, retaining missing neighbours as None.
    pub fn liquid_sub_chunks(
        &self,
    ) -> impl ExactSizeIterator<Item = ([i8; 3], Option<&'a SubChunk>)> + '_ {
        Self::liquid_sample_offsets().map(|offset| (offset, self.sub_chunk(offset)))
    }

    /// Reads one storage layer without flattening any block array.
    #[must_use]
    pub fn sample(&self, layer: usize, coordinate: [i32; 3]) -> MeshSample {
        let Some((offset, local)) = split_coordinate(coordinate) else {
            return MeshSample::Open;
        };
        self.sub_chunk(offset)
            .and_then(|sub_chunk| sub_chunk.runtime_id(layer, local[0], local[1], local[2]))
            .map_or(MeshSample::Open, MeshSample::Block)
    }

    /// Reads one storage layer only when it belongs to the liquid sample set.
    #[must_use]
    pub fn liquid_sample(&self, layer: usize, coordinate: [i32; 3]) -> MeshSample {
        let Some((offset, local)) = split_coordinate(coordinate) else {
            return MeshSample::Open;
        };
        if !is_liquid_sample_offset(offset) {
            return MeshSample::Open;
        }
        self.sub_chunk(offset)
            .and_then(|sub_chunk| sub_chunk.runtime_id(layer, local[0], local[1], local[2]))
            .map_or(MeshSample::Open, MeshSample::Block)
    }

    /// Returns the referenced packed sub-chunk and local block coordinate.
    #[must_use]
    pub fn block_source(&self, coordinate: [i32; 3]) -> Option<(&'a SubChunk, [u8; 3])> {
        let (offset, local) = split_coordinate(coordinate)?;
        Some((self.sub_chunk(offset)?, local))
    }

    /// Returns a palette-native source only within the liquid sample set.
    #[must_use]
    pub fn liquid_block_source(&self, coordinate: [i32; 3]) -> Option<(&'a SubChunk, [u8; 3])> {
        let (offset, local) = split_coordinate(coordinate)?;
        is_liquid_sample_offset(offset).then_some((self.sub_chunk(offset)?, local))
    }
}

const fn is_liquid_sample_offset([x, y, z]: [i8; 3]) -> bool {
    ((y == 0 || y == 1) && x >= -1 && x <= 1 && z >= -1 && z <= 1)
        || (y == -1 && ((x == 0 && z == 0) || (x.abs() + z.abs() == 1)))
}

fn index([x, y, z]: [i8; 3]) -> Option<usize> {
    if !(-1..=1).contains(&x) || !(-1..=1).contains(&y) || !(-1..=1).contains(&z) {
        return None;
    }
    Some(
        (usize::from((x + 1) as u8) * WIDTH + usize::from((y + 1) as u8)) * WIDTH
            + usize::from((z + 1) as u8),
    )
}

fn split_coordinate(coordinate: [i32; 3]) -> Option<([i8; 3], [u8; 3])> {
    let mut offset = [0_i8; 3];
    let mut local = [0_u8; 3];
    for axis in 0..3 {
        let sub_chunk_offset = coordinate[axis].div_euclid(SUB_CHUNK_SIDE);
        if !(-1..=1).contains(&sub_chunk_offset) {
            return None;
        }
        offset[axis] = sub_chunk_offset as i8;
        local[axis] = coordinate[axis].rem_euclid(SUB_CHUNK_SIDE) as u8;
    }
    Some((offset, local))
}
