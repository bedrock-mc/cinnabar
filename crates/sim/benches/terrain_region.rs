//! Host-side terrain region construction over mixed terrain.
#[path = "terrain_region/fixture.rs"]
mod fixture;

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sim::{CollisionRegistry, PaletteWorld, TerrainGeometry, TerrainSampler};
use world::ChunkStore;

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// Minimum and size of each query; all cross four columns, one absent, and two section rows.
const QUERIES: [(&str, [i32; 3], [u8; 3]); 2] = [
    ("256", [12, 62, 12], [8, 4, 8]),
    ("4096", [8, 56, 8], [16, 16, 16]),
];
const BOTH: TerrainGeometry = TerrainGeometry {
    collision: true,
    selection: true,
};
const MAX_BOXES: usize = 4096;

/// Owned metadata copied once per palette state, as the response palette does.
type Metadata = (u32, Option<String>, Option<String>, Vec<String>);

/// Samples one region and copies its palette metadata.
fn flat(
    sampler: &mut TerrainSampler,
    store: &ChunkStore,
    registry: &CollisionRegistry,
    (min, size): ([i32; 3], [u8; 3]),
    geometry: TerrainGeometry,
) -> (sim::TerrainSample, Vec<Metadata>) {
    let world = PaletteWorld::new(store, registry, 0, Default::default());
    let sample = sampler
        .sample(&world, min, size, geometry, MAX_BOXES)
        .unwrap();
    let metadata = sample
        .palette
        .iter()
        .map(|state| {
            let (identifier, canonical, tags) = fixture::metadata(state.runtime_id);
            (
                state.runtime_id,
                Some(identifier.to_owned()),
                Some(canonical.to_owned()),
                tags.iter().map(|tag| (*tag).to_owned()).collect(),
            )
        })
        .collect();
    (sample, metadata)
}

/// Reports allocation totals alongside the terrain region sampling timings.
#[expect(
    clippy::print_stderr,
    reason = "the benchmark reports allocation measurements directly to its caller"
)]
fn region_benches(criterion: &mut Criterion) {
    let store = fixture::store();
    let registry = fixture::registry();
    let mut group = criterion.benchmark_group("terrain_region");
    for (name, min, size) in QUERIES {
        let cells = size.iter().map(|&side| u64::from(side)).product::<u64>();
        let query = (min, size);
        let mut warm = TerrainSampler::default();
        flat(&mut warm, &store, &registry, query, BOTH);
        for (case, geometry) in [
            ("flat", BOTH),
            ("flat_no_geometry", TerrainGeometry::default()),
        ] {
            let ((sample, _), allocations) =
                test_alloc::measure(|| flat(&mut warm, &store, &registry, query, geometry));
            assert_eq!(
                sample.states.len() as u64,
                cells * u64::from(sample.layer_count)
            );
            eprintln!(
                "terrain_region/{case}/{name}: {} allocations, {} bytes, {} states, {} boxes, {} overrides",
                allocations.calls,
                allocations.bytes,
                sample.palette.len(),
                sample.shape_boxes.len() / 6,
                sample.overrides.len()
            );
            group.throughput(Throughput::Elements(cells));
            group.bench_with_input(BenchmarkId::new(case, name), &query, |bencher, &query| {
                bencher.iter(|| flat(&mut warm, &store, &registry, black_box(query), geometry));
            });
        }
        group.bench_with_input(
            BenchmarkId::new("flat_cold", name),
            &query,
            |bencher, &query| {
                bencher.iter(|| {
                    flat(
                        &mut TerrainSampler::default(),
                        &store,
                        &registry,
                        black_box(query),
                        BOTH,
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, region_benches);
criterion_main!(benches);
