//! Component-boundary cost of delivering one terrain region to a polling guest per frame.
// The bench polls without per-frame checks, so the test expectations stay unused.
#[allow(dead_code)]
#[path = "../src/tests/terrain_guest.rs"]
mod terrain_guest;

use std::{
    hint::black_box,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mod_host::{
    GameplaySnapshot, GameplayVector3, ModGrants, ModHost, TerrainBlockState, TerrainCheck,
    TerrainChunkRevision, TerrainDescriptors, TerrainMetadata, TerrainPhysics, TerrainPhysicsFlags,
    TerrainQuery, TerrainRefresh, TerrainRegion, TerrainScheduler, TerrainShape,
    TerrainShapeBinding, TerrainShapeOverride, TerrainSource, TerrainStamp, TerrainSurfaceResponse,
};

#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

/// Collision and selection flag bits.
const BOTH: u8 = 3;
/// One benchmarked region, sized as the sim fixture samples it.
struct Shape {
    name: &'static str,
    min: [i32; 3],
    size: [i32; 3],
    layers: usize,
    states: usize,
    boxes: usize,
    overrides: usize,
}
const REGIONS: [Shape; 2] = [
    Shape {
        name: "256",
        min: [12, 62, 12],
        size: [8, 4, 8],
        layers: 2,
        states: 10,
        boxes: 9,
        overrides: 13,
    },
    Shape {
        name: "4096",
        min: [8, 56, 8],
        size: [16, 16, 16],
        layers: 2,
        states: 12,
        boxes: 23,
        overrides: 58,
    },
];
const BLOCKS: [(&str, &str, &[&str]); 4] = [
    (
        "minecraft:stone",
        "{}",
        &["minecraft:is_pickaxe_item_destructible", "stone"],
    ),
    (
        "minecraft:oak_stairs",
        r#"{"upside_down_bit":{"type":"byte","value":0},"weirdo_direction":{"type":"int","value":2}}"#,
        &["minecraft:is_axe_item_destructible", "wood", "stairs"],
    ),
    (
        "minecraft:water",
        r#"{"liquid_depth":{"type":"int","value":0}}"#,
        &["water"],
    ),
    (
        "minecraft:dirt",
        r#"{"dirt_type":{"type":"string","value":"normal"}}"#,
        &["minecraft:is_shovel_item_destructible", "dirt"],
    ),
];

/// Fixture registry metadata, cycling through representative blocks.
struct Metadata(Vec<(String, String, Vec<Arc<str>>)>);
impl Metadata {
    fn new(states: usize) -> Self {
        Self(
            (0..states)
                .map(|index| {
                    let (identifier, state, tags) = BLOCKS[index % BLOCKS.len()];
                    (
                        format!("{identifier}_{index}"),
                        state.to_owned(),
                        tags.iter().map(|tag| Arc::from(*tag)).collect(),
                    )
                })
                .collect(),
        )
    }
}
impl TerrainMetadata for Metadata {
    fn identifier(&self, runtime_id: u32) -> Option<&str> {
        Some(&self.0[runtime_id as usize].0)
    }
    fn canonical_state(&self, runtime_id: u32) -> Option<&str> {
        Some(&self.0[runtime_id as usize].1)
    }
    fn tags(&self, runtime_id: u32) -> &[Arc<str>] {
        &self.0[runtime_id as usize].2
    }
}

fn snapshot() -> GameplaySnapshot {
    GameplaySnapshot {
        session: 1,
        dimension: 0,
        eye: GameplayVector3 {
            x: 16.,
            y: 64.,
            z: 16.,
        },
        yaw: 0.,
        pitch: 0.,
        attack_held: false,
        frame_seconds: 0.016,
        players: vec![],
    }
}

/// Palette entries with cached metadata, as the client assembles them per changed region.
fn palette(
    descriptors: &mut TerrainDescriptors,
    metadata: &Metadata,
    states: usize,
) -> Vec<TerrainBlockState> {
    let fixed = TerrainShapeBinding::Fixed(TerrainShape::Known(1));
    (0..states as u32)
        .map(|runtime_id| TerrainBlockState {
            physics: Some(TerrainPhysics {
                friction: 0.6,
                horizontal_speed_factor: 1.,
                vertical_speed_factor: 1.,
                fluid_height: 0.,
                flags: TerrainPhysicsFlags::empty(),
                surface: TerrainSurfaceResponse::None,
            }),
            collision: fixed,
            selection: fixed,
            ..descriptors.describe(0, runtime_id, metadata).0.clone()
        })
        .collect()
}

/// A flat response with the sim fixture's sizes; the second layer is mostly absent.
fn flat_region(metadata: &Metadata, shape: &Shape) -> TerrainRegion {
    let Shape {
        layers,
        states,
        boxes,
        overrides,
        ..
    } = *shape;
    let cells = shape.size.iter().product::<i32>() as usize;
    let palette = palette(&mut TerrainDescriptors::default(), metadata, states);
    TerrainRegion {
        stamp: TerrainStamp {
            session: 1,
            dimension: 0,
            registry: 0,
            revision: 0,
        },
        chunks: (0..=1)
            .flat_map(|x| {
                (0..=1).map(move |z| TerrainChunkRevision {
                    x,
                    z,
                    revision: (x + z < 2).then_some(7),
                })
            })
            .collect(),
        layer_count: layers as u8,
        loaded: vec![u64::MAX; cells.div_ceil(64)],
        states: (0..cells * layers)
            .map(|slot| {
                if slot < cells {
                    (slot % states) as u16
                } else if slot.is_multiple_of(37) {
                    2
                } else {
                    u16::MAX
                }
            })
            .collect(),
        palette,
        shape_offsets: (0..=boxes as u32 + 1)
            .map(|index| index.saturating_sub(1))
            .collect(),
        shape_boxes: (0..boxes * 6)
            .map(|value| (value % 7) as f64 / 8.)
            .collect(),
        overrides: (0..overrides as u32)
            .map(|index| TerrainShapeOverride {
                slot: index * 31,
                collision: Some(TerrainShape::Known(index % boxes as u32)),
                selection: None,
            })
            .collect(),
    }
}

/// Supplies the terrain session that admission measures from.
fn observe(host: &mut ModHost) {
    let world = mod_host::WorldObservation {
        session: 1,
        dimension: 0,
        world: "fixture".into(),
        position: (16., 64., 16.),
        alive: true,
    };
    host.observe_presentation(Some(world), None, None, Vec::new())
        .unwrap();
}

/// A world that reports one check for every subscription and samples one prepared region.
struct Supply {
    check: TerrainCheck,
    region: TerrainRefresh,
}
impl TerrainSource for Supply {
    fn check(&mut self, _: &TerrainQuery, _: Option<&TerrainRegion>) -> TerrainCheck {
        self.check
    }
    fn sample(&mut self, _: &TerrainQuery) -> TerrainRefresh {
        self.region.clone()
    }
}

/// Times only the guest frame; sampling happens outside the measurement.
fn frames(host: &mut ModHost, supply: &mut Supply, budget: usize, iterations: u64) -> Duration {
    let mut total = Duration::ZERO;
    for _ in 0..iterations {
        TerrainScheduler::default().refresh(&mut [&mut *host], budget, supply);
        observe(host);
        let start = Instant::now();
        host.frame_with_gameplay(false, Some(snapshot())).unwrap();
        total += start.elapsed();
    }
    total
}

/// Reports allocation totals alongside the terrain lowering benchmark timings.
#[expect(
    clippy::print_stderr,
    reason = "the benchmark reports allocation measurements directly to its caller"
)]
fn lowering_benches(criterion: &mut Criterion) {
    let grants = ModGrants {
        terrain: true,
        ..Default::default()
    };
    let mut group = criterion.benchmark_group("terrain_lowering");
    for shape in &REGIONS {
        let Shape {
            name,
            min,
            size,
            states,
            ..
        } = *shape;
        let max = [0, 1, 2].map(|axis| min[axis] + size[axis]);
        let cells = size.iter().product::<i32>() as usize;
        let load = |remember: bool| {
            let guest = terrain_guest::terrain_guest(min, max, BOTH, remember, "");
            let mut host = ModHost::load_snapshot_with_grants(
                Path::new("terrain.wasm"),
                &guest,
                grants.clone(),
            )
            .unwrap();
            observe(&mut host);
            host.frame_with_gameplay(false, Some(snapshot())).unwrap();
            host
        };
        let metadata = Metadata::new(states);
        let region = flat_region(&metadata, shape);
        let stale = TerrainCheck::Stale { cells };
        let supply = |check| Supply {
            check,
            region: TerrainRefresh::Replace(Box::new(region.clone())),
        };
        let mut changed = load(false);
        TerrainScheduler::default().refresh(&mut [&mut changed], cells, &mut supply(stale));
        observe(&mut changed);
        let ((), allocations) = test_alloc::measure(|| {
            changed
                .frame_with_gameplay(false, Some(snapshot()))
                .unwrap()
        });
        eprintln!(
            "terrain_lowering/changed/{name}: {} allocations, {} bytes per delivered frame",
            allocations.calls, allocations.bytes
        );
        group.throughput(Throughput::Elements(cells as u64));
        group.bench_function(BenchmarkId::new("changed", name), |bencher| {
            bencher.iter_custom(|iterations| {
                frames(&mut changed, &mut supply(stale), cells, iterations)
            });
        });
        let mut unchanged = load(true);
        frames(&mut unchanged, &mut supply(stale), cells, 1);
        group.bench_function(BenchmarkId::new("unchanged", name), |bencher| {
            bencher.iter_custom(|iterations| {
                frames(
                    &mut unchanged,
                    &mut supply(TerrainCheck::Current),
                    cells,
                    iterations,
                )
            });
        });
        let mut pending = load(false);
        group.bench_function(BenchmarkId::new("pending", name), |bencher| {
            bencher
                .iter_custom(|iterations| frames(&mut pending, &mut supply(stale), 0, iterations));
        });
        let mut warm = TerrainDescriptors::default();
        palette(&mut warm, &metadata, states);
        group.bench_function(BenchmarkId::new("palette_warm", name), |bencher| {
            bencher.iter(|| palette(&mut warm, &metadata, black_box(states)));
        });
        group.bench_function(BenchmarkId::new("palette_cold", name), |bencher| {
            bencher.iter(|| {
                palette(
                    &mut TerrainDescriptors::default(),
                    &metadata,
                    black_box(states),
                )
            });
        });
    }
    group.finish();
}

criterion_group!(benches, lowering_benches);
criterion_main!(benches);
