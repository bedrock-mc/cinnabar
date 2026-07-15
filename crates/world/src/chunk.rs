use std::{collections::BTreeMap, sync::Arc};

use crate::{
    BiomeStorage, BlockEntityKey, BlockEntityNbt, DecodedBiomeColumn, SubChunk,
    mesh_neighbourhood::LIQUID_SAMPLE_OFFSETS,
};

/// Key for one horizontal chunk column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkKey {
    pub dimension: i32,
    pub x: i32,
    pub z: i32,
}

impl ChunkKey {
    #[must_use]
    pub const fn new(dimension: i32, x: i32, z: i32) -> Self {
        Self { dimension, x, z }
    }
}

/// Key for one vertical sub-chunk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SubChunkKey {
    pub dimension: i32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl SubChunkKey {
    #[must_use]
    pub const fn new(dimension: i32, x: i32, y: i32, z: i32) -> Self {
        Self { dimension, x, y, z }
    }

    #[must_use]
    pub const fn from_chunk(chunk: ChunkKey, y: i32) -> Self {
        Self::new(chunk.dimension, chunk.x, y, chunk.z)
    }

    #[must_use]
    pub const fn chunk(self) -> ChunkKey {
        ChunkKey::new(self.dimension, self.x, self.z)
    }

    /// Meshes invalidated when this sub-chunk's block data changes.
    ///
    /// Face culling crosses sub-chunk boundaries, so the changed key and all
    /// six face-adjacent keys must be considered for remeshing. Coordinates at
    /// the `i32` edge omit the overflowing neighbour.
    pub fn mesh_dependents(self) -> impl Iterator<Item = Self> {
        let x_minus = self
            .x
            .checked_sub(1)
            .map(|x| Self::new(self.dimension, x, self.y, self.z));
        let x_plus = self
            .x
            .checked_add(1)
            .map(|x| Self::new(self.dimension, x, self.y, self.z));
        let y_minus = self
            .y
            .checked_sub(1)
            .map(|y| Self::new(self.dimension, self.x, y, self.z));
        let y_plus = self
            .y
            .checked_add(1)
            .map(|y| Self::new(self.dimension, self.x, y, self.z));
        let z_minus = self
            .z
            .checked_sub(1)
            .map(|z| Self::new(self.dimension, self.x, self.y, z));
        let z_plus = self
            .z
            .checked_add(1)
            .map(|z| Self::new(self.dimension, self.x, self.y, z));
        [
            Some(self),
            x_minus,
            x_plus,
            y_minus,
            y_plus,
            z_minus,
            z_plus,
        ]
        .into_iter()
        .flatten()
    }

    /// Every bounded sub-chunk whose AO or liquid geometry can sample this
    /// sub-chunk, including face, edge, and corner offsets.
    pub fn mesh_neighbourhood_dependents(self) -> impl Iterator<Item = Self> {
        (-1_i32..=1).flat_map(move |dx| {
            (-1_i32..=1).flat_map(move |dy| {
                (-1_i32..=1).filter_map(move |dz| {
                    Some(Self::new(
                        self.dimension,
                        self.x.checked_add(dx)?,
                        self.y.checked_add(dy)?,
                        self.z.checked_add(dz)?,
                    ))
                })
            })
        })
    }

    /// Every mesh whose bounded liquid sample set can reference this source.
    ///
    /// This is the checked inverse of `MeshNeighbourhood::liquid_sample_offsets`:
    /// two horizontal 3x3 target layers plus the inverse lower-center/cardinal
    /// flow samples. It does
    /// not widen ordinary face-only cube invalidation.
    pub fn liquid_mesh_dependents(self) -> impl Iterator<Item = Self> {
        LIQUID_SAMPLE_OFFSETS
            .into_iter()
            .filter_map(move |[dx, dy, dz]| {
                Some(Self::new(
                    self.dimension,
                    self.x.checked_sub(i32::from(dx))?,
                    self.y.checked_sub(i32::from(dy))?,
                    self.z.checked_sub(i32::from(dz))?,
                ))
            })
    }
}

/// Sparse block data for one chunk column.
#[derive(Debug, Default)]
pub struct Chunk {
    pub(crate) sub_chunks: BTreeMap<i32, Arc<SubChunk>>,
    pub(crate) biomes: Option<DecodedBiomeColumn>,
    pub(crate) block_entities: BTreeMap<BlockEntityKey, Arc<BlockEntityNbt>>,
    pub(crate) block_entity_bytes: usize,
}

impl Chunk {
    #[must_use]
    pub fn sub_chunk(&self, y: i32) -> Option<Arc<SubChunk>> {
        self.sub_chunks.get(&y).cloned()
    }

    pub fn sub_chunks(&self) -> impl ExactSizeIterator<Item = (i32, Arc<SubChunk>)> + '_ {
        self.sub_chunks
            .iter()
            .map(|(&y, sub_chunk)| (y, Arc::clone(sub_chunk)))
    }

    /// Returns the packed biome storage for one absolute sub-chunk Y.
    #[must_use]
    pub fn biome_storage(&self, y: i32) -> Option<Arc<BiomeStorage>> {
        self.biomes.as_ref()?.storage(y)
    }

    #[must_use]
    pub fn block_entity(&self, key: BlockEntityKey) -> Option<Arc<BlockEntityNbt>> {
        self.block_entities.get(&key).cloned()
    }

    pub fn block_entities(
        &self,
    ) -> impl ExactSizeIterator<Item = (BlockEntityKey, Arc<BlockEntityNbt>)> + '_ {
        self.block_entities
            .iter()
            .map(|(&key, value)| (key, Arc::clone(value)))
    }
}
