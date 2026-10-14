//! Per-section costs of meshing distant terrain on synthetic fixture terrain (the dense
//! hills of `terrain_block` and the gentler `rolling_block`), sampled and merged as
//! ingestion stores it; live captures, not these, check frame budgets.

#[cfg(feature = "benchmark-mimalloc")]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

use std::{
    cell::RefCell,
    collections::HashMap,
    convert::Infallible,
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use lod::{
    fixtures::{biome_column, blocks, column_of, fixture_classifier, rolling_block, terrain_block},
    geometry::{Face, SectionKey, SectionPos},
    ingest::{SectionSource, merge_columns},
    mesh::{
        ExteriorCulling,
        class::MeshClasses,
        input::{FaceSlabs, MeshInput},
        mesher::Mesher,
    },
    sampler::{ColumnSnapshot, sample_column},
    section::{ChildMask, Section},
    storage::{DenseSection, PackScratch},
    voxel::{FULL_COVERAGE, Light, MaterialId, Voxel},
};
use world::ChunkKey;

/// Chunk columns per side of the fixture area.
const AREA_CHUNKS: i32 = 12;
/// Sub-chunk Ys of every fixture column: the terrain, its caves and the air above it.
const SUB_CHUNK_YS: std::ops::Range<i32> = 0..8;
/// Timed repetitions of each section for the percentile report.
const PERCENTILE_REPEATS: usize = 9;

/// Stored sections in memory, standing in for the store and cache mesh jobs read.
#[derive(Default)]
struct Memory(HashMap<SectionKey, Section>);

impl SectionSource for Memory {
    type Error = Infallible;

    /// The stored section at `pos`, cloned; never fails.
    fn load(&mut self, pos: SectionPos) -> Result<Option<Section>, Infallible> {
        Ok(self.0.get(&pos.key()).cloned())
    }
}

/// A fixture block function: the layer-0 id at an absolute block position and whether
/// layer 1 holds water.
type BlockFn = fn(i32, i32, i32) -> (u32, bool);

/// The fixture terrains: name and block function.
const TERRAINS: [(&str, BlockFn); 2] = [("fixture", terrain_block), ("rolling", rolling_block)];

/// Samples and merges `block`'s terrain over the fixture area column by column, as the
/// ingestion writer does.
fn fixture_store(block: BlockFn) -> Memory {
    let classes = fixture_classifier();
    let mut classifier = fixture_classifier();
    let biomes = biome_column(0, &[1, 1, 2, 2, 3, 3, 4, 4]);
    let mut store = Memory::default();
    for chunk_x in 0..AREA_CHUNKS {
        for chunk_z in 0..AREA_CHUNKS {
            let sub_chunks = column_of(block, chunk_x, chunk_z, SUB_CHUNK_YS);
            let snapshot = ColumnSnapshot {
                chunk: ChunkKey::new(0, chunk_x, chunk_z),
                sub_chunks: &sub_chunks,
                biomes: &biomes,
                default_biome: 0,
                light: None,
            };
            let sample = sample_column(&snapshot, &mut classifier).expect("fixture keys");
            let Ok(merged) = merge_columns([&sample], &mut store, &classes);
            for section in merged.sections {
                store.0.insert(section.pos().key(), section);
            }
        }
    }
    store
}

/// Stored sections at `level` whose same-level neighbours are all stored or above the
/// terrain, so they mesh as they would inside explored land.
fn interior(store: &Memory, level: u8) -> Vec<SectionPos> {
    let mut out: Vec<SectionPos> = store
        .0
        .keys()
        .map(|key| key.pos())
        .filter(|pos| pos.level() == level)
        .filter(|&pos| {
            [Face::NegX, Face::PosX, Face::NegZ, Face::PosZ]
                .into_iter()
                .all(|face| {
                    FaceSlabs::of(pos, face)
                        .same
                        .is_some_and(|n| store.0.contains_key(&n.key()))
                })
        })
        .collect();
    out.sort();
    out
}

/// The mesh input of `pos` with every stored section it reads; `pos` must be stored.
fn input<'a>(store: &'a Memory, pos: SectionPos) -> MeshInput<'a> {
    MeshInput::from_lookup(&store.0[&pos.key()], |at| store.0.get(&at.key()))
}

/// A section of alternating stone and air voxels in every direction, the most faces a
/// section can have, with its six neighbours.
fn checkerboard() -> Memory {
    let stone = Voxel::new(
        MaterialId::new(blocks::STONE).expect("id"),
        Light::DARK,
        FULL_COVERAGE,
        false,
    );
    let mut dense = DenseSection::filled(Voxel::AIR, 0);
    for y in 0..32 {
        for z in 0..32 {
            for x in 0..32 {
                if (x + y + z) % 2 == 0 {
                    dense.set(x, y, z, stone);
                } else {
                    dense.set(x, y, z, Voxel::air(Light::FULL_SKY));
                }
            }
        }
    }
    let mut scratch = PackScratch::default();
    let mut store = Memory::default();
    let centre = SectionPos::new(0, 0, 0, 0).expect("pos");
    for pos in Face::ALL
        .into_iter()
        .filter_map(|face| centre.neighbour(face))
        .chain([centre])
    {
        let section =
            Section::from_dense(pos, &dense, ChildMask::UNKNOWN, &mut scratch).expect("level 0");
        store.0.insert(pos.key(), section);
    }
    store
}

/// Times meshing every interior fixture section of each level, with and without exterior
/// culling, and the checkerboard worst case.
fn meshing(c: &mut Criterion) {
    let store = fixture_store(terrain_block);
    let classes = MeshClasses::new(&fixture_classifier(), blocks::SAND + 1);
    let mut mesher = Mesher::new();
    let mut group = c.benchmark_group("mesh");
    for level in 0..=4 {
        let sections = interior(&store, level);
        if sections.is_empty() {
            continue;
        }
        group.throughput(Throughput::Elements(sections.len() as u64));
        for (name, culling) in [
            ("exterior_culled", ExteriorCulling::SectionLocal),
            ("all_faces", ExteriorCulling::Off),
        ] {
            group.bench_function(format!("l{level}_fixture_sections_{name}"), |b| {
                b.iter(|| {
                    for &pos in &sections {
                        black_box(mesher.mesh(&input(&store, pos), &classes, culling));
                    }
                });
            });
        }
    }
    let worst = checkerboard();
    let centre = SectionPos::new(0, 0, 0, 0).expect("pos");
    group.throughput(Throughput::Elements(1));
    group.bench_function("l0_checkerboard_worst_case", |b| {
        b.iter(|| black_box(mesher.mesh(&input(&worst, centre), &classes, ExteriorCulling::Off)));
    });
    group.finish();
}

/// Prints p50 and p99 over each terrain's level-0 sections of each section's median time.
fn percentiles(c: &mut Criterion) {
    let classes = MeshClasses::new(&fixture_classifier(), blocks::SAND + 1);
    let mut mesher = Mesher::new();
    for (terrain, block) in TERRAINS {
        let store = fixture_store(block);
        let sections = interior(&store, 0);
        for (name, culling) in [
            ("exterior_culled", ExteriorCulling::SectionLocal),
            ("all_faces", ExteriorCulling::Off),
        ] {
            let medians = RefCell::new(Vec::new());
            let mut quads = 0;
            c.bench_function(&format!("mesh/l0_{terrain}_percentiles_{name}"), |b| {
                b.iter_custom(|iterations| {
                    let mut total = Duration::ZERO;
                    let mut found = Vec::with_capacity(sections.len());
                    for _ in 0..iterations {
                        found.clear();
                        quads = 0;
                        for &pos in &sections {
                            let input = input(&store, pos);
                            let mut times = [Duration::ZERO; PERCENTILE_REPEATS];
                            for time in &mut times {
                                let start = Instant::now();
                                let mesh = black_box(mesher.mesh(&input, &classes, culling));
                                *time = start.elapsed();
                                quads += mesh.quads().len();
                            }
                            times.sort_unstable();
                            found.push(times[PERCENTILE_REPEATS / 2]);
                            total += times.iter().sum::<Duration>() / PERCENTILE_REPEATS as u32;
                        }
                    }
                    *medians.borrow_mut() = found.clone();
                    total
                });
            });
            let mut medians = medians.into_inner();
            medians.sort_unstable();
            if let (Some(p50), Some(p99)) = (
                medians.get(medians.len() / 2),
                medians.get((medians.len() * 99).div_ceil(100).saturating_sub(1)),
            ) {
                eprintln!(
                    "mesh/l0_{terrain} {name}: {} sections, p50 {p50:?}, p99 {p99:?}, {} quads per section",
                    medians.len(),
                    quads / (medians.len() * PERCENTILE_REPEATS).max(1)
                );
            }
        }
    }
}

criterion_group!(benches, meshing, percentiles);
criterion_main!(benches);
