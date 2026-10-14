//! Sub-chunk, biome and classifier fixtures built through the public `world` decoders.

use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    sync::Arc,
};

use world::{
    BLOCKS_PER_SUB_CHUNK, DecodedBiomeColumn, PACKED_BITS, RawBiomeIds, RawBlockIds,
    SUB_CHUNK_SIDE, SubChunk,
};

use crate::{
    classify::{BlockClass, SampledBlock, TableClassifier},
    voxel::MaterialId,
};

/// Builds a version-8 network sub-chunk whose layer `n` holds `block(n, x, y, z)` runtime ids.
#[must_use]
pub fn sub_chunk(layers: usize, block: impl Fn(usize, usize, usize, usize) -> u32) -> SubChunk {
    let mut bytes = vec![8, layers as u8];
    for layer in 0..layers {
        let mut ids = vec![0_u32; BLOCKS_PER_SUB_CHUNK];
        for x in 0..SUB_CHUNK_SIDE {
            for z in 0..SUB_CHUNK_SIDE {
                for y in 0..SUB_CHUNK_SIDE {
                    ids[(x << 8) | (z << 4) | y] = block(layer, x, y, z);
                }
            }
        }
        encode_storage(&ids, &mut bytes);
    }
    SubChunk::decode(&bytes, &RawBlockIds { air: 0 })
}

/// A one-layer sub-chunk filled with `runtime_id`.
#[must_use]
pub fn uniform_sub_chunk(runtime_id: u32) -> SubChunk {
    sub_chunk(1, |_, _, _, _| runtime_id)
}

/// A biome column whose storages from `base_sub_chunk_y` up are each uniform.
#[must_use]
pub fn biome_column(base_sub_chunk_y: i32, biomes: &[u32]) -> DecodedBiomeColumn {
    let mut payload = Vec::new();
    for &biome in biomes {
        payload.push(0);
        write_var_i32(biome as i32, &mut payload);
    }
    DecodedBiomeColumn::decode(
        base_sub_chunk_y,
        biomes.len(),
        &payload,
        &RawBiomeIds { default_biome: 0 },
    )
}

/// Runtime ids of [`fixture_classifier`]; each id is also its material id.
pub mod blocks {
    pub const AIR: u32 = 0;
    pub const STONE: u32 = 1;
    pub const DIRT: u32 = 2;
    pub const GRASS: u32 = 3;
    pub const WATER: u32 = 4;
    pub const LEAVES: u32 = 5;
    pub const GLASS: u32 = 6;
    pub const SAND: u32 = 7;
}

/// Classifies [`blocks`] ids: stone, dirt, grass and sand are opaque, water is water,
/// leaves are leaves and glass is see-through.
#[must_use]
pub fn fixture_classifier() -> TableClassifier {
    let mut classifier = TableClassifier::default();
    for (id, class) in [
        (blocks::STONE, BlockClass::Opaque),
        (blocks::DIRT, BlockClass::Opaque),
        (blocks::GRASS, BlockClass::Opaque),
        (blocks::WATER, BlockClass::Water),
        (blocks::LEAVES, BlockClass::Leaves),
        (blocks::GLASS, BlockClass::SeeThrough),
        (blocks::SAND, BlockClass::Opaque),
    ] {
        let material = MaterialId::new(id).expect("fixture ids fit the material field");
        classifier.insert(id, SampledBlock { material, class });
    }
    classifier
}

/// Sea level of [`terrain_block`].
pub const TERRAIN_SEA_LEVEL: i32 = 46;

/// Deterministic terrain in absolute block coordinates: hills around Y 40–52 with grass,
/// dirt, stone, sand under water, small caves, leaf canopies and some glass, waterlogged
/// below sea level. Returns the layer-0 id and whether layer 1 holds water.
#[must_use]
pub fn terrain_block(x: i32, y: i32, z: i32) -> (u32, bool) {
    let height = 40 + (x.rem_euclid(23) * 3 + z.rem_euclid(17) * 2) % 13;
    let tree = (x * 7 + z * 11).rem_euclid(29) == 0;
    let glass = (x * 5 + z * 3).rem_euclid(41) == 0;
    let block = if y > height {
        if glass && y == height + 1 {
            return (blocks::GLASS, y <= TERRAIN_SEA_LEVEL);
        } else if tree && y <= height + 4 && y > TERRAIN_SEA_LEVEL {
            blocks::LEAVES
        } else if y <= TERRAIN_SEA_LEVEL {
            blocks::WATER
        } else {
            blocks::AIR
        }
    } else if y == height {
        if height < TERRAIN_SEA_LEVEL {
            blocks::SAND
        } else {
            blocks::GRASS
        }
    } else if y > height - 4 {
        blocks::DIRT
    } else if y < height - 6 && (x * 3 + y * 5 + z * 7).rem_euclid(31) < 3 {
        blocks::AIR
    } else {
        blocks::STONE
    };
    (block, false)
}

/// Gentle terrain in absolute block coordinates, about as dense in faces as explored
/// surface terrain: slopes of one block in four around Y 38–56, grass over dirt and stone,
/// sand under water, rare one-block caves and some leaf canopies. Returns the layer-0 id and
/// whether layer 1 holds water, as [`terrain_block`].
#[must_use]
pub fn rolling_block(x: i32, y: i32, z: i32) -> (u32, bool) {
    // A triangle wave of `period` blocks rising one block in four.
    let wave = |value: i32, period: i32| (value.rem_euclid(period) - period / 2).abs() / 4;
    let height = 38 + wave(x, 48) + wave(z + 7, 40);
    let tree = (x * 7 + z * 11).rem_euclid(53) == 0;
    let block = if y > height {
        if tree && y <= height + 4 && y > TERRAIN_SEA_LEVEL {
            blocks::LEAVES
        } else if y <= TERRAIN_SEA_LEVEL {
            blocks::WATER
        } else {
            blocks::AIR
        }
    } else if y == height {
        if height < TERRAIN_SEA_LEVEL {
            blocks::SAND
        } else {
            blocks::GRASS
        }
    } else if y > height - 4 {
        blocks::DIRT
    } else if y < height - 6 && (x * 3 + y * 5 + z * 7).rem_euclid(331) == 0 {
        blocks::AIR
    } else {
        blocks::STONE
    };
    (block, false)
}

/// The [`terrain_block`] sub-chunks of one chunk column for sub-chunk Ys `sub_chunk_ys`.
#[must_use]
pub fn terrain_column(
    chunk_x: i32,
    chunk_z: i32,
    sub_chunk_ys: Range<i32>,
) -> BTreeMap<i32, Arc<SubChunk>> {
    column_of(terrain_block, chunk_x, chunk_z, sub_chunk_ys)
}

/// The sub-chunks of one chunk column for sub-chunk Ys `sub_chunk_ys`, from `block` in
/// absolute block coordinates ([`terrain_block`], [`rolling_block`]).
#[must_use]
pub fn column_of(
    block: fn(i32, i32, i32) -> (u32, bool),
    chunk_x: i32,
    chunk_z: i32,
    sub_chunk_ys: Range<i32>,
) -> BTreeMap<i32, Arc<SubChunk>> {
    let side = SUB_CHUNK_SIDE as i32;
    sub_chunk_ys
        .map(|sub_y| {
            let sub_chunk = sub_chunk(2, |layer, x, y, z| {
                let (block, waterlogged) = block(
                    chunk_x * side + x as i32,
                    sub_y * side + y as i32,
                    chunk_z * side + z as i32,
                );
                match layer {
                    0 => block,
                    _ if waterlogged => blocks::WATER,
                    _ => blocks::AIR,
                }
            });
            (sub_y, Arc::new(sub_chunk))
        })
        .collect()
}

/// Appends one network block storage holding `ids` in Bedrock order.
fn encode_storage(ids: &[u32], out: &mut Vec<u8>) {
    let mut palette = Vec::new();
    let mut lookup = HashMap::new();
    let indices: Vec<usize> = ids
        .iter()
        .map(|&id| {
            *lookup.entry(id).or_insert_with(|| {
                palette.push(id);
                palette.len() - 1
            })
        })
        .collect();
    if palette.len() == 1 {
        out.push(1);
        write_var_i32(palette[0] as i32, out);
        return;
    }
    let bits = PACKED_BITS
        .into_iter()
        .find(|&bits| palette.len() <= 1 << bits)
        .expect("16 bits index every sub-chunk block");
    out.push(bits << 1 | 1);
    let per_word = 32 / usize::from(bits);
    for chunk in indices.chunks(per_word) {
        let word = chunk
            .iter()
            .enumerate()
            .fold(0_u32, |word, (slot, &index)| {
                word | (index as u32) << (slot * usize::from(bits))
            });
        out.extend_from_slice(&word.to_le_bytes());
    }
    write_var_i32(palette.len() as i32, out);
    for id in palette {
        write_var_i32(id as i32, out);
    }
}

/// Appends a zigzag var-int.
fn write_var_i32(value: i32, out: &mut Vec<u8>) {
    let mut value = ((value << 1) ^ (value >> 31)) as u32;
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}
