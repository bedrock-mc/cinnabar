use std::time::Instant;

use far_terrain::{
    ColumnPos, ColumnSummary, FACE_UP, FarTiles, NearMask, SummaryStore, SurfaceCell, TileGrid,
    TileKey, TileView, config::TILE_CELLS, mesh_tile, select_tiles,
};

fn flat(height: i16) -> ColumnSummary {
    let mut summary = ColumnSummary::default();
    summary.cells_mut().fill(SurfaceCell {
        height,
        rgb: [10, 200, 30],
        water_depth: 0,
    });
    summary
}

fn view() -> TileView {
    TileView {
        centre: [8.0, 8.0],
        near_blocks: 15.0 * 16.0,
        far_blocks: 64.0 * 16.0,
    }
}

fn contains(tile: TileKey, x: i32, z: i32) -> bool {
    let [ox, oz] = tile.origin();
    (ox..ox + tile.width_blocks()).contains(&x) && (oz..oz + tile.width_blocks()).contains(&z)
}

/// Every column of the far ring is covered exactly once, and only level 0 meets near terrain.
#[test]
fn selection_partitions_the_ring_and_coarsens_outward() {
    let view = view();
    let tiles = select_tiles(view);
    for (x, z) in [(260, 0), (0, -300), (700, 700), (-1100, 40), (250, 250)] {
        let covering = tiles.iter().filter(|tile| contains(**tile, x, z)).count();
        assert_eq!(covering, 1, "block {x},{z}");
    }
    for tile in &tiles {
        let [ox, oz] = tile.origin().map(|v| v as f32);
        let w = tile.width_blocks() as f32;
        let dx = (ox - view.centre[0])
            .max(view.centre[0] - (ox + w))
            .max(0.0);
        let dz = (oz - view.centre[1])
            .max(view.centre[1] - (oz + w))
            .max(0.0);
        if dx.hypot(dz) < view.near_blocks {
            assert_eq!(tile.level, 0, "{tile:?} reaches into near terrain");
        }
    }
    let levels: std::collections::BTreeSet<u8> = tiles.iter().map(|tile| tile.level).collect();
    assert!(levels.len() >= 3, "{levels:?}");
    assert!(
        !tiles.iter().any(|tile| contains(*tile, 8, 8)),
        "the camera column is near terrain"
    );
}

#[test]
fn near_mask_matches_chunk_grid_retention() {
    let mask = NearMask::retained([5, -3], 8);
    for x in -20..30 {
        for z in -25..20 {
            assert_eq!(
                mask.contains(ColumnPos::new(x, z)),
                world::chunk_in_view(8, [x, z], [5, -3]),
                "{x},{z}"
            );
        }
    }
}

/// A flat tile is one top per column at level 0, plus skirts; no quad leaves its column.
#[test]
fn flat_level_zero_tiles_merge_within_columns() {
    let key = TileKey {
        level: 0,
        x: 0,
        z: 0,
    };
    let columns: Vec<_> = key.columns().map(|pos| (pos, flat(70))).collect();
    let mesh = mesh_tile(&TileGrid::build(key, &columns));
    let tops: Vec<_> = mesh
        .quads
        .iter()
        .filter(|q| q[0] >> 28 == FACE_UP)
        .collect();
    assert_eq!(tops.len(), 16);
    for quad in &mesh.quads {
        let (x, z) = (quad[0] & 127, (quad[0] >> 7) & 127);
        let (w, l) = ((quad[0] >> 14) & 127, (quad[0] >> 21) & 127);
        assert!(
            x % 16 + w <= 16 && z % 16 + l <= 16,
            "quad crosses a column: {quad:?}"
        );
    }
    let skirts = mesh.quads.len() - tops.len();
    assert_eq!(skirts, 16, "four columns of skirt on each edge");
    assert_eq!(mesh.colours.len(), TILE_CELLS * TILE_CELLS * 4);
    assert_eq!(&mesh.colours[..4], &[10, 200, 30, 0]);
}

/// Coarse cells keep ridges: a single tall column raises its whole 16-block cell.
#[test]
fn coarse_cells_lean_toward_their_highest_blocks() {
    let key = TileKey {
        level: 4,
        x: 0,
        z: 0,
    };
    let mut summary = flat(60);
    summary.cells_mut()[0].height = 100;
    let grid = TileGrid::build(key, &[(ColumnPos::new(0, 0), summary)]);
    let cell = grid.cell(0, 0);
    assert!(cell.height > 60 && cell.height < 100, "{}", cell.height);
    assert!(grid.cell(1, 0).is_empty());
    let mesh = mesh_tile(&grid);
    assert_eq!(
        mesh.quads.iter().filter(|q| q[0] >> 28 == FACE_UP).count(),
        1
    );
}

/// Built tiles appear once their worker finishes, and a column change rebuilds its tile.
#[test]
fn tiles_build_on_workers_and_rebuild_on_change() {
    let mut store = SummaryStore::new(usize::MAX);
    for x in -40..40 {
        for z in -40..40 {
            store.admit(ColumnPos::new(x, z), &flat(64));
        }
    }
    let mut tiles = FarTiles::default();
    tiles.set_view(view());
    let now = Instant::now();
    test_time::eventually("far tiles build", || {
        tiles.update(&mut store, now);
        !tiles.drawn().is_empty()
    });
    let edited = |tiles: &FarTiles| {
        tiles
            .drawn()
            .into_iter()
            .find(|mesh| mesh.key.level == 0 && contains(mesh.key, 320, 0))
            .map(|mesh| mesh.revision)
    };
    test_time::eventually("the edited column's tile builds", || {
        tiles.update(&mut store, now);
        edited(&tiles).is_some()
    });
    let before = edited(&tiles);
    store.admit(ColumnPos::new(20, 0), &flat(90));
    tiles.mark_changed(&store.take_changed());
    let later = now + far_terrain::config::TILE_REBUILD_INTERVAL;
    test_time::eventually("the edited column's tile rebuilds", || {
        tiles.update(&mut store, later);
        edited(&tiles) != before
    });
}

/// A mostly-water coarse cell stays at sea level even with an island in it.
#[test]
fn coarse_water_cells_keep_the_sea_level() {
    let key = TileKey {
        level: 4,
        x: 0,
        z: 0,
    };
    let mut summary = ColumnSummary::default();
    summary.cells_mut().fill(SurfaceCell {
        height: 63,
        rgb: [20, 60, 200],
        water_depth: 6,
    });
    summary.cells_mut()[0] = SurfaceCell {
        height: 90,
        rgb: [30, 140, 40],
        water_depth: 0,
    };
    let grid = TileGrid::build(key, &[(ColumnPos::new(0, 0), summary)]);
    assert_eq!(grid.cell(0, 0).height, 63);
    assert!(grid.cell(0, 0).water_depth > 0);
}
