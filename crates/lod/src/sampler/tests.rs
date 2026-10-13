use std::{cell::Cell as Counter, collections::BTreeMap, sync::Arc};

use world::{ChunkKey, LightStore, SubChunkKey, SubChunkLight};

use super::*;
use crate::{
    fixtures::{biome_column, blocks, fixture_classifier, sub_chunk, uniform_sub_chunk},
    voxel::MaterialId,
};

const KEY: ChunkKey = ChunkKey::new(0, 0, 0);

fn material(id: u32) -> MaterialId {
    MaterialId::new(id).expect("fixture id")
}

fn sample(
    chunk: ChunkKey,
    sub_chunks: &BTreeMap<i32, Arc<SubChunk>>,
    light: Option<&LightStoreSnapshot>,
) -> ColumnSample {
    let biomes = biome_column(-4, &[]);
    let snapshot = ColumnSnapshot {
        chunk,
        sub_chunks,
        biomes: &biomes,
        default_biome: 1,
        light,
    };
    sample_column(&snapshot, &mut fixture_classifier()).expect("column has a key")
}

fn column(entries: impl IntoIterator<Item = (i32, SubChunk)>) -> BTreeMap<i32, Arc<SubChunk>> {
    entries
        .into_iter()
        .map(|(y, sub_chunk)| (y, Arc::new(sub_chunk)))
        .collect()
}

#[test]
fn enclosed_opaque_voxels_become_hidden_inside_the_column_only() {
    let stone = column((0..3).map(|y| (y, uniform_sub_chunk(blocks::STONE))));
    let sample = sample(KEY, &stone, None);
    // Column edges border unloaded columns and the bottom and top border no data.
    assert_eq!(sample.stats.hidden_voxels, 14 * 14 * 46);
    let middle = &sample.sub_chunks[1];
    assert_eq!(
        middle.voxel(5, 0, 5),
        Voxel::HIDDEN,
        "neighbours across sub-chunks count"
    );
    assert!(!middle.voxel(0, 5, 5).is_hidden());
    assert!(!middle.voxel(5, 5, 15).is_hidden());
    assert!(!sample.sub_chunks[0].voxel(5, 0, 5).is_hidden());
    assert!(!sample.sub_chunks[2].voxel(5, 15, 5).is_hidden());
}

#[test]
fn leaves_and_water_never_hide_their_neighbours() {
    let sub = sub_chunk(1, |_, x, _, _| {
        if x == 7 {
            blocks::STONE
        } else {
            blocks::LEAVES
        }
    });
    let sample = sample(KEY, &column([(0, sub)]), None);
    assert_eq!(sample.stats.hidden_voxels, 0);
    assert_eq!(
        sample.sub_chunks[0].voxel(7, 5, 5).material(),
        material(blocks::STONE)
    );
}

#[test]
fn sub_chunks_map_to_level_zero_octants() {
    let sub_chunks = column([
        (-1, uniform_sub_chunk(blocks::STONE)),
        (2, uniform_sub_chunk(blocks::DIRT)),
    ]);
    let sample = sample(ChunkKey::new(0, 3, -1), &sub_chunks, None);
    let placed: Vec<_> = sample
        .sub_chunks
        .iter()
        .map(|sub| (sub.section(), sub.octant()))
        .collect();
    let pos = |y| SectionPos::new(0, 1, y, -1).expect("pos");
    let octant = |upper| Octant::from_halves(true, upper, true);
    assert_eq!(
        placed,
        [
            (pos(-1), octant(false)),
            (pos(-1), octant(true)),
            (pos(0), octant(false)),
            (pos(0), octant(true)),
            (pos(1), octant(false)),
            (pos(1), octant(true)),
        ]
    );
    assert_eq!(
        sample.stats.sub_chunks, 6,
        "gaps and the rest of the end sections are air"
    );
    assert!(sample.sub_chunks[2].voxel(3, 3, 3).is_air());
    assert_eq!(
        sample.sub_chunks[1].voxel(0, 3, 3).material(),
        material(blocks::STONE)
    );
}

#[test]
fn the_top_section_is_completed_with_sky_lit_air() {
    // Nine sub-chunks: the highest stored one fills only the lower octant of its section.
    let sub_chunks = column((-4..=4).map(|y| {
        let block = if y < 4 { blocks::STONE } else { blocks::AIR };
        (y, uniform_sub_chunk(block))
    }));
    let unlit = sample(KEY, &sub_chunks, None);
    let top = unlit.sub_chunks.last().expect("sampled sub-chunks");
    assert_eq!(unlit.stats.sub_chunks, 10);
    assert_eq!(top.section(), SectionPos::new(0, 0, 2, 0).expect("pos"));
    assert_eq!(top.octant(), Octant::from_halves(false, true, false));
    for (x, y, z) in [(0, 0, 0), (7, 8, 9), (15, 15, 15)] {
        assert_eq!(top.voxel(x, y, z), Voxel::air(Light::FULL_SKY));
    }

    let mut store = LightStore::default();
    store.insert_resident(
        SubChunkKey::from_chunk(KEY, 5),
        SubChunkLight::uniform(2, 11, 1).expect("light in range"),
    );
    let snapshot = store.snapshot();
    let lit = sample(KEY, &sub_chunks, Some(&snapshot));
    let top = lit.sub_chunks.last().expect("sampled sub-chunks");
    assert_eq!(
        top.voxel(4, 4, 4).light(),
        Light::new(11, 2),
        "solved light reaches the completing octant"
    );
}

#[test]
fn missing_light_uses_the_column_heightmap() {
    let sub = sub_chunk(1, |_, x, y, _| match (x, y) {
        (_, 0..=9) => blocks::STONE,
        (0, 12) => blocks::DIRT,
        (_, 11) if x == 0 => blocks::GLASS,
        _ => blocks::AIR,
    });
    let sample = sample(
        KEY,
        &column([(0, sub), (1, uniform_sub_chunk(blocks::AIR))]),
        None,
    );
    assert_eq!(sample.stats.fallback_lit_sub_chunks, 2);
    let lower = &sample.sub_chunks[0];
    assert_eq!(
        lower.voxel(3, 10, 3).light(),
        Light::FULL_SKY,
        "above the top opaque block"
    );
    assert_eq!(
        lower.voxel(3, 9, 3).light(),
        Light::DARK,
        "the top opaque block itself"
    );
    assert_eq!(
        lower.voxel(0, 11, 0).light(),
        Light::DARK,
        "below an overhang"
    );
    assert_eq!(lower.voxel(0, 13, 0).light(), Light::FULL_SKY);
    assert_eq!(sample.sub_chunks[1].voxel(8, 8, 8).light(), Light::FULL_SKY);
}

#[test]
fn solved_light_wins_over_the_fallback() {
    let mut store = LightStore::default();
    store.insert_resident(
        SubChunkKey::from_chunk(KEY, 1),
        SubChunkLight::uniform(3, 7, 1).expect("light in range"),
    );
    let snapshot = store.snapshot();
    let sub_chunks = column([
        (0, uniform_sub_chunk(blocks::STONE)),
        (1, uniform_sub_chunk(blocks::AIR)),
    ]);
    let sample = sample(KEY, &sub_chunks, Some(&snapshot));
    assert_eq!(sample.stats.fallback_lit_sub_chunks, 1);
    assert_eq!(
        sample.sub_chunks[1].voxel(4, 4, 4).light(),
        Light::new(7, 3)
    );
}

#[test]
fn channels_without_a_sample_take_the_heightmap_light() {
    let solved = SubChunkLight::uniform(6, 9, 1).expect("light in range");
    let fallback = Light::new(15, 2);
    assert_eq!(
        solved_light(&solved, bedrock_index(15, 15, 15), fallback),
        Light::new(9, 6)
    );
    assert_eq!(
        solved_light(&solved, BLOCKS_PER_SUB_CHUNK, fallback),
        fallback,
        "a sample past the channel falls back per channel, not to darkness"
    );
}

#[test]
fn second_layer_water_marks_waterlogging() {
    let sub = sub_chunk(2, |layer, x, _, _| match (layer, x) {
        (0, 0) => blocks::GLASS,
        (0, 1) => blocks::AIR,
        (0, 2) => blocks::WATER,
        (0, _) => blocks::AIR,
        (_, 0 | 1) => blocks::WATER,
        _ => blocks::AIR,
    });
    let sample = sample(KEY, &column([(0, sub)]), None);
    let voxels = &sample.sub_chunks[0];
    let logged = voxels.voxel(0, 4, 4);
    assert_eq!(logged.material(), material(blocks::GLASS));
    assert!(logged.has_water());
    let liquid_only = voxels.voxel(1, 4, 4);
    assert_eq!(liquid_only.material(), material(blocks::WATER));
    assert!(liquid_only.has_water());
    assert!(voxels.voxel(2, 4, 4).has_water());
    assert!(!voxels.voxel(3, 4, 4).has_water());
}

#[test]
fn each_palette_value_is_classified_once_per_column() {
    struct Counting {
        inner: crate::classify::TableClassifier,
        calls: Counter<u32>,
    }
    impl BlockClassifier for Counting {
        fn classify(&mut self, runtime_id: u32) -> SampledBlock {
            self.calls.set(self.calls.get() + 1);
            self.inner.classify(runtime_id)
        }
    }
    let mixed = |_: usize, x: usize, _: usize, _: usize| [blocks::STONE, blocks::DIRT, 999][x % 3];
    let sub_chunks = column((0..4).map(|y| (y, sub_chunk(1, mixed))));
    let biomes = biome_column(0, &[]);
    let mut classifier = Counting {
        inner: fixture_classifier(),
        calls: Counter::new(0),
    };
    let snapshot = ColumnSnapshot {
        chunk: KEY,
        sub_chunks: &sub_chunks,
        biomes: &biomes,
        default_biome: 0,
        light: None,
    };
    let sample = sample_column(&snapshot, &mut classifier).expect("column has a key");
    assert_eq!(classifier.calls.get(), 3);
    assert!(
        sample.sub_chunks[0].voxel(2, 0, 0).is_air(),
        "unknown ids sample as air"
    );
}

#[test]
fn surface_biomes_come_from_the_highest_non_air_block() {
    let lower = sub_chunk(1, |_, x, y, _| {
        if x < 8 || y < 4 {
            blocks::STONE
        } else {
            blocks::AIR
        }
    });
    let upper = sub_chunk(1, |_, x, y, _| {
        if x < 8 && y < 2 {
            blocks::GRASS
        } else {
            blocks::AIR
        }
    });
    let sub_chunks = column([(0, lower), (1, upper)]);
    let biomes = biome_column(0, &[10, 20]);
    let snapshot = ColumnSnapshot {
        chunk: ChunkKey::new(0, 1, 0),
        sub_chunks: &sub_chunks,
        biomes: &biomes,
        default_biome: 0,
        light: None,
    };
    let sample = sample_column(&snapshot, &mut fixture_classifier()).expect("column has a key");
    let mut section = DenseSection::filled(Voxel::AIR, 99);
    sample.sub_chunks[0].write_into(&mut section);
    assert_eq!(
        section.biomes().get(16 + 3, 5),
        20,
        "grass in the upper sub-chunk"
    );
    assert_eq!(
        section.biomes().get(16 + 12, 5),
        10,
        "stone in the lower sub-chunk"
    );
    assert_eq!(
        section.biomes().get(3, 5),
        99,
        "other quadrants keep their biome"
    );
    assert_eq!(
        section.get(16 + 3, 2, 0).material(),
        material(blocks::STONE)
    );
}

#[test]
fn out_of_range_sub_chunks_are_counted_and_skipped() {
    for odd in [600, -300] {
        let sub_chunks = column([
            (0, uniform_sub_chunk(blocks::STONE)),
            (odd, uniform_sub_chunk(blocks::STONE)),
        ]);
        let sampled = sample(KEY, &sub_chunks, None);
        assert_eq!(sampled.stats.out_of_range_sub_chunks, 1, "Y {odd}");
        assert_eq!(
            sampled.stats.sub_chunks, 2,
            "only sub-chunk 0 and its section sibling, Y {odd}"
        );
    }
    let only_odd = column([(600, uniform_sub_chunk(blocks::STONE))]);
    let sampled = sample(KEY, &only_odd, None);
    assert!(sampled.sub_chunks.is_empty());
    assert_eq!(sampled.stats.out_of_range_sub_chunks, 1);
}

#[test]
fn malformed_indices_are_counted_on_both_layers() {
    let stone = SampledBlock {
        material: material(blocks::STONE),
        class: BlockClass::Opaque,
    };
    let water = SampledBlock {
        material: material(blocks::WATER),
        class: BlockClass::Water,
    };
    let mut cells = vec![Cell::AIR; 4];
    assert_eq!(
        apply_block_layer(&mut cells, &[stone], &[0, 1, u32::MAX, 0]),
        2
    );
    assert_eq!(cells[0].block, stone);
    assert_eq!(cells[1], Cell::AIR, "a past-palette index samples as air");
    // Layer 1 storage with missing words reads `u32::MAX` for the blocks they held.
    assert_eq!(
        apply_liquid_layer(
            &mut cells,
            &[SampledBlock::AIR, water],
            &[1, 1, 7, u32::MAX]
        ),
        2
    );
    assert!(cells[0].water && cells[1].water);
    assert_eq!(cells[1].block, water);
    assert!(!cells[2].water && !cells[3].water);
    assert_eq!(cells[3].block, stone);
}

#[test]
fn columns_without_a_key_are_rejected() {
    let sub_chunks = column([(0, uniform_sub_chunk(blocks::STONE))]);
    let biomes = biome_column(0, &[]);
    let snapshot = ColumnSnapshot {
        chunk: ChunkKey::new(0, 1 << 25, 0),
        sub_chunks: &sub_chunks,
        biomes: &biomes,
        default_biome: 0,
        light: None,
    };
    assert!(matches!(
        sample_column(&snapshot, &mut fixture_classifier()),
        Err(KeyError::Horizontal(_))
    ));
}

#[test]
fn empty_columns_sample_nothing() {
    let sample = sample(KEY, &BTreeMap::new(), None);
    assert!(sample.sub_chunks.is_empty());
}
