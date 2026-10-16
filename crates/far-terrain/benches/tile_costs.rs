//! CPU cost of selecting far-terrain tiles and building their meshes.
#![expect(
    clippy::print_stderr,
    reason = "the benchmark reports quad and tile counts on stderr"
)]

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use far_terrain::{
    COLUMN_SIDE, ColumnPos, ColumnSummary, FarTiles, NearMask, SummaryStore, SurfaceCell, TileGrid,
    TileKey, TileView, Versioned,
    config::{LEVEL_COUNT, TILE_REBUILD_INTERVAL},
    mesh_tile, select_tiles,
};

/// Rolling hills with lakes, so tops rarely merge and walls are common.
fn hills(pos: ColumnPos) -> ColumnSummary {
    let mut summary = ColumnSummary::default();
    for (index, cell) in summary.cells_mut().iter_mut().enumerate() {
        let x = (pos.x * 16 + (index % 16) as i32) as f32;
        let z = (pos.z * 16 + (index / 16) as i32) as f32;
        let height = 64.0 + 12.0 * (x / 37.0).sin() * (z / 53.0).cos() + 4.0 * (x / 7.0).sin();
        let water = height < 60.0;
        *cell = SurfaceCell {
            height: if water { 60 } else { height as i16 },
            rgb: [(x as u32 % 255) as u8, 140, (z as u32 % 255) as u8],
            water_depth: if water { (60.0 - height) as u8 + 1 } else { 0 },
        };
    }
    summary
}

fn columns(key: TileKey) -> Vec<(ColumnPos, ColumnSummary)> {
    key.columns().map(|pos| (pos, hills(pos))).collect()
}

/// Holds the measured update until its workers finish outside Criterion's timed section.
struct UpdatingTile {
    tiles: FarTiles,
    store: SummaryStore,
    column: ColumnPos,
    now: Instant,
}

impl Drop for UpdatingTile {
    fn drop(&mut self) {
        test_time::eventually("benchmark tile finishes", || {
            self.tiles.update(&mut self.store, self.now);
            self.tiles.ready(self.column)
        });
    }
}

/// Isolates a fully cached coarsest tile after the rest of its ring has settled empty.
fn updating_tile() -> UpdatingTile {
    let width = TileKey {
        level: LEVEL_COUNT - 1,
        x: 0,
        z: 0,
    }
    .width_blocks();
    let view = TileView {
        centre: [8.0, 8.0],
        near_blocks: 0.0,
        skip_blocks: 4.0 * width as f32,
        far_blocks: 5.0 * width as f32,
    };
    let keys = select_tiles(view);
    assert!(keys.iter().all(|key| key.level == LEVEL_COUNT - 1));
    let key = keys[0];
    let now = Instant::now();
    let mut tiles = FarTiles::default();
    tiles.set_view(view);
    let mut store = SummaryStore::new(usize::MAX);
    // Each readiness probe needs a decoded summary, even when its terrain is empty.
    for key in &keys {
        store.admit(key.columns().next().unwrap(), &ColumnSummary::default());
    }
    while !keys
        .iter()
        .all(|key| tiles.ready(key.columns().next().unwrap()))
    {
        tiles.update(&mut store, now);
    }
    let [x, z] = key.origin().map(|block| block / COLUMN_SIDE as i32);
    let side = key.width_blocks() / COLUMN_SIDE as i32;
    let encoded = hills(ColumnPos::new(x, z)).encode();
    for cz in z - 1..=z + side {
        for cx in x - 1..=x + side {
            store.admit_versioned(
                ColumnPos::new(cx, cz),
                Versioned {
                    version: 1,
                    summary: encoded.clone(),
                },
            );
        }
    }
    let column = ColumnPos::new(x + side / 2, z + side / 2);
    tiles.mark_changed(&[column]);
    UpdatingTile {
        tiles,
        store,
        column,
        now: now + TILE_REBUILD_INTERVAL,
    }
}

/// Measures snapshot copies and the first foreground slice separately from worker meshing.
fn update_costs(c: &mut Criterion) {
    let encoded = hills(ColumnPos::new(0, 0)).encode();
    let side = (TileKey {
        level: LEVEL_COUNT - 1,
        x: 0,
        z: 0,
    }
    .width_blocks()
        / COLUMN_SIDE as i32
        + 2) as usize;
    let copies: Vec<_> = (0..side * side).map(|_| encoded.clone()).collect();
    let shared = copies
        .iter()
        .filter(|copy| std::ptr::eq(copy.bytes.as_ref(), encoded.bytes.as_ref()))
        .count();
    eprintln!(
        "snapshot columns: {}; shared payloads: {shared}; copied payload bytes: {}",
        copies.len(),
        (copies.len() - shared) * encoded.bytes.len()
    );
    let mut group = c.benchmark_group("far_terrain");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(200));
    group.bench_function("clone_level_4_snapshot", |b| {
        b.iter(|| {
            (0..side * side)
                .map(|_| black_box(&encoded).clone())
                .collect::<Vec<_>>()
        });
    });
    group.bench_function("update_full_level_4_cache", |b| {
        b.iter_batched(
            updating_tile,
            |mut sample| {
                black_box(sample.tiles.update(&mut sample.store, sample.now));
                sample
            },
            BatchSize::PerIteration,
        );
    });
}

fn tile_costs(c: &mut Criterion) {
    for level in [0, 3] {
        let key = TileKey { level, x: 3, z: -2 };
        let columns = columns(key);
        c.bench_function(&format!("far_terrain/grid_level_{level}"), |b| {
            b.iter(|| TileGrid::build(black_box(key), black_box(&columns)));
        });
        let grid = TileGrid::build(key, &columns);
        c.bench_function(&format!("far_terrain/mesh_level_{level}"), |b| {
            b.iter(|| mesh_tile(black_box(&grid)));
        });
        eprintln!("level {level} quads: {}", mesh_tile(&grid).quads.len());
    }
    let view = TileView {
        centre: [8.0, 8.0],
        near_blocks: 15.0 * 16.0,
        skip_blocks: 15.0 * 16.0,
        far_blocks: 64.0 * 16.0,
    };
    eprintln!("selected tiles: {}", select_tiles(view).len());
    c.bench_function("far_terrain/select_tiles", |b| {
        b.iter(|| select_tiles(black_box(view)))
    });
    let disc: Vec<ColumnPos> = (-12..=12)
        .flat_map(|x| (-12..=12).map(move |z| ColumnPos::new(x, z)))
        .filter(|column| column.x * column.x + column.z * column.z <= 144)
        .collect();
    c.bench_function("far_terrain/near_mask_radius_12", |b| {
        b.iter(|| NearMask::from_columns(black_box([0, 0]), 12, disc.iter().copied()));
    });
}

criterion_group!(benches, tile_costs, update_costs);
criterion_main!(benches);
