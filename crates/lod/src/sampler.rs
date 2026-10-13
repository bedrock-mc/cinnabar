//! Level-0 sampling of one chunk column into the quadrant it covers in each L0 section.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use world::{
    BLOCKS_PER_SUB_CHUNK, BiomeStorage, Chunk, ChunkKey, DecodedBiomeColumn, LightChannel,
    LightStoreSnapshot, PalettedStorage, SUB_CHUNK_SIDE, SubChunk, SubChunkKey, SubChunkLight,
};

use crate::{
    classify::{BlockClass, BlockClassifier, SampledBlock},
    geometry::{KeyError, MAX_SECTION_Y, MIN_SECTION_Y, Octant, SectionPos, VoxelBox},
    storage::DenseSection,
    voxel::{FULL_COVERAGE, Light, Voxel},
};

const SIDE: usize = SUB_CHUNK_SIDE;
const COLUMNS: usize = SIDE * SIDE;
/// Lowest sub-chunk Y whose level-0 section has a key.
const MIN_SUB_CHUNK_Y: i32 = MIN_SECTION_Y * 2;
/// Highest sub-chunk Y whose level-0 section has a key.
const MAX_SUB_CHUNK_Y: i32 = MAX_SECTION_Y * 2 + 1;

/// Packed biome storage per absolute sub-chunk Y.
pub trait BiomeSource {
    /// The biome storage of sub-chunk `sub_chunk_y`; `None` when the column has none there,
    /// and the sampler then uses [`ColumnSnapshot::default_biome`].
    fn biome_storage(&self, sub_chunk_y: i32) -> Option<Arc<BiomeStorage>>;
}

impl BiomeSource for Chunk {
    fn biome_storage(&self, sub_chunk_y: i32) -> Option<Arc<BiomeStorage>> {
        Chunk::biome_storage(self, sub_chunk_y)
    }
}

impl BiomeSource for DecodedBiomeColumn {
    fn biome_storage(&self, sub_chunk_y: i32) -> Option<Arc<BiomeStorage>> {
        self.storage(sub_chunk_y)
    }
}

/// Immutable inputs for sampling one column, usually taken from a `world::Chunk` and a
/// `LightStore` snapshot on a background lane.
#[derive(Clone, Copy)]
pub struct ColumnSnapshot<'a> {
    pub chunk: ChunkKey,
    /// Sub-chunks by absolute Y; gaps between stored sub-chunks are air.
    pub sub_chunks: &'a BTreeMap<i32, Arc<SubChunk>>,
    pub biomes: &'a dyn BiomeSource,
    /// Biome used where the column has no biome storage.
    pub default_biome: u32,
    /// Solved light; sub-chunks without an entry use the heightmap fallback.
    pub light: Option<&'a LightStoreSnapshot>,
}

/// Counters for one sampled column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SampleStats {
    /// Sub-chunks written, including air gaps between stored ones and the air that
    /// completes the lowest and highest level-0 sections.
    pub sub_chunks: u32,
    /// Stored sub-chunks skipped because their section Y has no key.
    pub out_of_range_sub_chunks: u32,
    /// Sub-chunks lit by the fallback because no solved light was available.
    pub fallback_lit_sub_chunks: u32,
    /// Layer-0 blocks whose packed index was missing or past the palette, sampled as air,
    /// plus layer-1 blocks with such an index, sampled without water.
    pub malformed_indices: u32,
    /// Opaque blocks rewritten to [`Voxel::HIDDEN`].
    pub hidden_voxels: u32,
}

/// One sampled sub-chunk: the 16³ octant it fills in its level-0 section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SampledSubChunk {
    section: SectionPos,
    octant: Octant,
    voxels: Box<[Voxel; BLOCKS_PER_SUB_CHUNK]>,
    surface_biomes: Arc<[u32; COLUMNS]>,
}

impl SampledSubChunk {
    /// The level-0 section this sub-chunk lies in.
    #[must_use]
    pub fn section(&self) -> SectionPos {
        self.section
    }

    /// The octant of [`Self::section`] this sub-chunk fills.
    #[must_use]
    pub fn octant(&self) -> Octant {
        self.octant
    }

    /// The section voxels this sub-chunk writes; the dirty box for the mip chain.
    #[must_use]
    pub fn region(&self) -> VoxelBox {
        self.octant.parent_region()
    }

    /// Voxel at sub-chunk-local coordinates; panics outside `0..16`.
    #[must_use]
    pub fn voxel(&self, x: usize, y: usize, z: usize) -> Voxel {
        self.voxels[local_index(x, y, z)]
    }

    /// Writes this octant's voxels and its columns' surface biomes into `section`.
    pub fn write_into(&self, section: &mut DenseSection) {
        let [x0, y0, z0] = self.region().min();
        for y in 0..SIDE {
            for z in 0..SIDE {
                for x in 0..SIDE {
                    section.set(x0 + x, y0 + y, z0 + z, self.voxel(x, y, z));
                }
            }
        }
        for z in 0..SIDE {
            for x in 0..SIDE {
                section
                    .biomes_mut()
                    .set(x0 + x, z0 + z, self.surface_biomes[z * SIDE + x]);
            }
        }
    }
}

/// A sampled column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnSample {
    pub sub_chunks: Vec<SampledSubChunk>,
    pub stats: SampleStats,
}

/// Samples a column into level-0 voxels.
///
/// Covers every sub-chunk from the lowest to the highest stored one whose section Y has a
/// key, widened to whole level-0 sections: Bedrock treats unsent sub-chunks as air, so gaps
/// and the sibling octants of the lowest and highest sections are sampled as air with their
/// solved or fallback light. Sections outside that range are not touched; a store must drop
/// what an older, taller copy of the column left there.
///
/// Each distinct palette value is classified once. Layer 1 water marks a block as
/// waterlogged. Opaque blocks whose six neighbours inside this column are opaque become
/// [`Voxel::HIDDEN`]; neighbours in other columns count as not opaque. Sub-chunks without
/// solved light get sky light 15 above the column's highest opaque block, 0 at or below it,
/// and block light 0. Each column's surface biome is taken at its highest non-air block.
/// Fails only when the column's X or Z has no section key.
pub fn sample_column(
    snapshot: &ColumnSnapshot<'_>,
    classifier: &mut impl BlockClassifier,
) -> Result<ColumnSample, KeyError> {
    SectionPos::of_sub_chunk(snapshot.chunk.x, 0, snapshot.chunk.z)?;
    let in_range = MIN_SUB_CHUNK_Y..=MAX_SUB_CHUNK_Y;
    let mut stats = SampleStats {
        out_of_range_sub_chunks: snapshot
            .sub_chunks
            .keys()
            .filter(|y| !in_range.contains(y))
            .count() as u32,
        ..SampleStats::default()
    };
    let mut stored = snapshot.sub_chunks.range(in_range).map(|(&y, _)| y);
    let Some(first) = stored.next() else {
        return Ok(ColumnSample {
            sub_chunks: Vec::new(),
            stats,
        });
    };
    let last = stored.next_back().unwrap_or(first);
    // Whole level-0 sections; the key range starts on an even and ends on an odd Y.
    let low = first.div_euclid(2) * 2;
    let high = last.div_euclid(2) * 2 + 1;

    let mut palettes = PaletteCache::new(classifier);
    let layers: Vec<Option<Box<[Cell]>>> = (low..=high)
        .map(|y| {
            snapshot
                .sub_chunks
                .get(&y)
                .map(|sub_chunk| palettes.cells(sub_chunk, &mut stats.malformed_indices))
        })
        .collect();
    let column = ColumnFacts::new(low, &layers, snapshot);

    let mut sub_chunks = Vec::with_capacity(layers.len());
    for (offset, layer) in layers.iter().enumerate() {
        let y = low + offset as i32;
        let (section, octant) = SectionPos::of_sub_chunk(snapshot.chunk.x, y, snapshot.chunk.z)?;
        let light = snapshot
            .light
            .and_then(|light| light.light(SubChunkKey::from_chunk(snapshot.chunk, y)));
        stats.fallback_lit_sub_chunks += u32::from(light.is_none());
        let mut voxels = Box::new([Voxel::AIR; BLOCKS_PER_SUB_CHUNK]);
        for ly in 0..SIDE {
            for lz in 0..SIDE {
                for lx in 0..SIDE {
                    let linear = bedrock_index(lx, ly, lz);
                    let cell = layer.as_ref().map_or(Cell::AIR, |cells| cells[linear]);
                    let block_y = y * SIDE as i32 + ly as i32;
                    let voxel = if cell.block.class == BlockClass::Opaque
                        && is_enclosed(&layers, offset, lx, ly, lz)
                    {
                        stats.hidden_voxels += 1;
                        Voxel::HIDDEN
                    } else {
                        let fallback = column.fallback_light(lx, lz, block_y);
                        let light =
                            light.map_or(fallback, |light| solved_light(light, linear, fallback));
                        Voxel::new(cell.block.material, light, FULL_COVERAGE, cell.water)
                    };
                    voxels[local_index(lx, ly, lz)] = voxel;
                }
            }
        }
        stats.sub_chunks += 1;
        sub_chunks.push(SampledSubChunk {
            section,
            octant,
            voxels,
            surface_biomes: Arc::clone(&column.surface_biomes),
        });
    }
    Ok(ColumnSample { sub_chunks, stats })
}

/// Sub-chunk-local voxel order of [`SampledSubChunk`]: X fastest, then Z, then Y.
const fn local_index(x: usize, y: usize, z: usize) -> usize {
    (y * SIDE + z) * SIDE + x
}

/// Bedrock's packed order, shared by block palettes and light nibbles.
const fn bedrock_index(x: usize, y: usize, z: usize) -> usize {
    (x << 8) | (z << 4) | y
}

/// The solved light of the block at Bedrock index `linear`; a channel without a sample
/// there takes that channel of `fallback`, the heightmap light.
fn solved_light(light: &SubChunkLight, linear: usize, fallback: Light) -> Light {
    let read = |channel| light.channel(channel).get(linear);
    Light::new(
        read(LightChannel::Sky).unwrap_or(fallback.sky()),
        read(LightChannel::Block).unwrap_or(fallback.block()),
    )
}

/// One classified block plus its water flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    block: SampledBlock,
    water: bool,
}

impl Cell {
    const AIR: Self = Self {
        block: SampledBlock::AIR,
        water: false,
    };
}

/// True when all six neighbours inside the column's data are opaque.
fn is_enclosed(
    layers: &[Option<Box<[Cell]>>],
    offset: usize,
    x: usize,
    y: usize,
    z: usize,
) -> bool {
    let opaque_at = |layer: Option<&Option<Box<[Cell]>>>, x: usize, y: usize, z: usize| {
        layer
            .and_then(Option::as_ref)
            .is_some_and(|cells| cells[bedrock_index(x, y, z)].block.class == BlockClass::Opaque)
    };
    let here = layers.get(offset);
    let below = if y > 0 {
        opaque_at(here, x, y - 1, z)
    } else {
        offset
            .checked_sub(1)
            .is_some_and(|below| opaque_at(layers.get(below), x, SIDE - 1, z))
    };
    let above = if y + 1 < SIDE {
        opaque_at(here, x, y + 1, z)
    } else {
        opaque_at(layers.get(offset + 1), x, 0, z)
    };
    below
        && above
        && x > 0
        && opaque_at(here, x - 1, y, z)
        && x + 1 < SIDE
        && opaque_at(here, x + 1, y, z)
        && z > 0
        && opaque_at(here, x, y, z - 1)
        && z + 1 < SIDE
        && opaque_at(here, x, y, z + 1)
}

/// Classifies palette values once per column and expands storages into cells.
struct PaletteCache<'c, C> {
    classifier: &'c mut C,
    classified: HashMap<u32, SampledBlock>,
    indices: Box<[u32; BLOCKS_PER_SUB_CHUNK]>,
}

impl<'c, C: BlockClassifier> PaletteCache<'c, C> {
    /// An empty cache over `classifier`.
    fn new(classifier: &'c mut C) -> Self {
        Self {
            classifier,
            classified: HashMap::new(),
            indices: Box::new([0; BLOCKS_PER_SUB_CHUNK]),
        }
    }

    /// The sampled block of each palette entry, classifying values not seen before.
    fn map_palette(&mut self, storage: &PalettedStorage) -> Vec<SampledBlock> {
        storage
            .palette()
            .values()
            .iter()
            .map(|&value| {
                *self
                    .classified
                    .entry(value)
                    .or_insert_with(|| self.classifier.classify(value).normalized())
            })
            .collect()
    }

    /// Expands a sub-chunk's block layer and waterlogging layer into cells, counting
    /// malformed indices of either layer in `malformed`.
    fn cells(&mut self, sub_chunk: &SubChunk, malformed: &mut u32) -> Box<[Cell]> {
        let mut cells = vec![Cell::AIR; BLOCKS_PER_SUB_CHUNK].into_boxed_slice();
        let storages = sub_chunk.storages();
        if let Some(blocks) = storages.first() {
            let palette = self.map_palette(blocks);
            blocks.palette_indices(&mut self.indices);
            *malformed += apply_block_layer(&mut cells, &palette, &self.indices[..]);
        }
        if let Some(liquid) = storages.get(1) {
            let palette = self.map_palette(liquid);
            liquid.palette_indices(&mut self.indices);
            *malformed += apply_liquid_layer(&mut cells, &palette, &self.indices[..]);
        }
        cells
    }
}

/// Sets each cell's block from layer-0 `indices` into `palette`; water blocks are also
/// waterlogged. Returns the indices that miss the palette, whose cells stay air.
fn apply_block_layer(cells: &mut [Cell], palette: &[SampledBlock], indices: &[u32]) -> u32 {
    let mut malformed = 0;
    for (cell, &index) in cells.iter_mut().zip(indices) {
        match palette.get(index as usize) {
            Some(&block) => {
                cell.block = block;
                cell.water = block.class == BlockClass::Water;
            }
            None => malformed += 1,
        }
    }
    malformed
}

/// Marks cells waterlogged where layer-1 `indices` name water, and turns air cells into
/// that water. Returns the indices that miss the palette, whose cells stay unchanged.
fn apply_liquid_layer(cells: &mut [Cell], palette: &[SampledBlock], indices: &[u32]) -> u32 {
    let mut malformed = 0;
    for (cell, &index) in cells.iter_mut().zip(indices) {
        let Some(&block) = palette.get(index as usize) else {
            malformed += 1;
            continue;
        };
        if block.class == BlockClass::Water {
            cell.water = true;
            if cell.block.class == BlockClass::Air {
                cell.block = block;
            }
        }
    }
    malformed
}

/// Per-column facts shared by every sub-chunk: light fallback heights and surface biomes.
struct ColumnFacts {
    /// Absolute Y of the highest opaque block per column, if any.
    top_opaque: [Option<i32>; COLUMNS],
    surface_biomes: Arc<[u32; COLUMNS]>,
}

impl ColumnFacts {
    /// Scans `layers`, the sampled sub-chunks from Y `low` up, from the top down.
    fn new(low: i32, layers: &[Option<Box<[Cell]>>], snapshot: &ColumnSnapshot<'_>) -> Self {
        let mut top_opaque = [None; COLUMNS];
        let mut surface = [None::<(i32, usize)>; COLUMNS];
        for (offset, cells) in layers.iter().enumerate().rev() {
            let Some(cells) = cells else { continue };
            for z in 0..SIDE {
                for x in 0..SIDE {
                    let column = z * SIDE + x;
                    if top_opaque[column].is_some() {
                        continue;
                    }
                    for y in (0..SIDE).rev() {
                        let class = cells[bedrock_index(x, y, z)].block.class;
                        let block_y = (low + offset as i32) * SIDE as i32 + y as i32;
                        if class != BlockClass::Air && surface[column].is_none() {
                            surface[column] = Some((low + offset as i32, y));
                        }
                        if class == BlockClass::Opaque {
                            top_opaque[column].get_or_insert(block_y);
                            break;
                        }
                    }
                }
            }
        }
        let mut biomes = [snapshot.default_biome; COLUMNS];
        for (column, biome) in biomes.iter_mut().enumerate() {
            let (sub_y, y) = surface[column].unwrap_or((low, 0));
            let (x, z) = (column % SIDE, column / SIDE);
            if let Some(id) = snapshot
                .biomes
                .biome_storage(sub_y)
                .and_then(|storage| storage.biome_id(x as u8, y as u8, z as u8))
            {
                *biome = id;
            }
        }
        Self {
            top_opaque,
            surface_biomes: Arc::new(biomes),
        }
    }

    /// Fallback light of a block: full sky above the column's highest opaque block, dark
    /// at or below it, block light 0.
    fn fallback_light(&self, x: usize, z: usize, block_y: i32) -> Light {
        match self.top_opaque[z * SIDE + x] {
            Some(top) if block_y <= top => Light::DARK,
            _ => Light::FULL_SKY,
        }
    }
}

#[cfg(test)]
mod tests;
