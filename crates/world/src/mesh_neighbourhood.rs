use crate::SubChunk;

const WIDTH: usize = 3;
const SUB_CHUNK_SIDE: i32 = 16;
const ENTRY_COUNT: usize = WIDTH * WIDTH * WIDTH;
const ADJACENT_OFFSETS: [[i8; 3]; 26] = [
    [-1, -1, -1],
    [-1, -1, 0],
    [-1, -1, 1],
    [-1, 0, -1],
    [-1, 0, 0],
    [-1, 0, 1],
    [-1, 1, -1],
    [-1, 1, 0],
    [-1, 1, 1],
    [0, -1, -1],
    [0, -1, 0],
    [0, -1, 1],
    [0, 0, -1],
    [0, 0, 1],
    [0, 1, -1],
    [0, 1, 0],
    [0, 1, 1],
    [1, -1, -1],
    [1, -1, 0],
    [1, -1, 1],
    [1, 0, -1],
    [1, 0, 0],
    [1, 0, 1],
    [1, 1, -1],
    [1, 1, 0],
    [1, 1, 1],
];
pub(crate) const LIQUID_SAMPLE_OFFSETS: [[i8; 3]; 19] = [
    [0, -1, 0],
    [-1, 0, -1],
    [-1, 0, 0],
    [-1, 0, 1],
    [0, 0, -1],
    [0, 0, 0],
    [0, 0, 1],
    [1, 0, -1],
    [1, 0, 0],
    [1, 0, 1],
    [-1, 1, -1],
    [-1, 1, 0],
    [-1, 1, 1],
    [0, 1, -1],
    [0, 1, 0],
    [0, 1, 1],
    [1, 1, -1],
    [1, 1, 0],
    [1, 1, 1],
];

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
}

impl MeshDependencyMask {
    #[must_use]
    pub const fn new(diagonal_ao: bool, liquid: bool) -> Self {
        Self {
            diagonal_ao,
            liquid,
        }
    }

    #[must_use]
    pub const fn needs_diagonal_samples(self) -> bool {
        self.diagonal_ao || self.liquid
    }
}

/// Center sub-chunk plus at most one adjacent sub-chunk in every direction.
///
/// References preserve the world's palette-packed representation. Coordinates
/// accepted by [`Self::sample`] span exactly `-16..=31` on each axis; anything
/// beyond that bounded 3x3x3 snapshot is explicit open space.
#[derive(Debug, Clone)]
pub struct MeshNeighbourhood<'a> {
    sub_chunks: [Option<&'a SubChunk>; ENTRY_COUNT],
}

impl<'a> MeshNeighbourhood<'a> {
    pub const ADJACENT_SUB_CHUNK_COUNT: usize = ADJACENT_OFFSETS.len();
    pub const LIQUID_SAMPLE_SUB_CHUNK_COUNT: usize = LIQUID_SAMPLE_OFFSETS.len();

    #[must_use]
    pub fn new(center: &'a SubChunk) -> Self {
        let mut sub_chunks = [None; ENTRY_COUNT];
        sub_chunks[index([0, 0, 0]).expect("center offset is bounded")] = Some(center);
        Self { sub_chunks }
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

    /// Canonical offsets for all 26 adjacent sub-chunks used by diagonal AO.
    pub fn adjacent_offsets() -> impl ExactSizeIterator<Item = [i8; 3]> {
        ADJACENT_OFFSETS.into_iter()
    }

    /// Exact sub-chunk offsets needed by vanilla-like liquid surface meshing.
    ///
    /// The set is a horizontal 3x3 at the current and upper Y levels, plus
    /// the lower center used by the bottom face. It is fixed, deduplicated,
    /// and deliberately smaller than the general 3x3x3 AO neighbourhood.
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
    ((y == 0 || y == 1) && x >= -1 && x <= 1 && z >= -1 && z <= 1) || (x == 0 && y == -1 && z == 0)
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
