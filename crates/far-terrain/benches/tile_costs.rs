//! CPU cost of selecting far-terrain tiles and building their meshes.
#![expect(
    clippy::print_stderr,
    reason = "the benchmark reports quad and tile counts on stderr"
)]

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use far_terrain::{
    ColumnPos, ColumnSummary, NearMask, SurfaceCell, TileGrid, TileKey, TileView, mesh_tile,
    select_tiles,
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

criterion_group!(benches, tile_costs);
criterion_main!(benches);
