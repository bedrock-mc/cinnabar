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
        skip_blocks: 15.0 * 16.0,
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

/// The mask holds exactly the given columns; its inner disc holds only near blocks and its
/// outer disc every near block.
#[test]
fn near_mask_bounds_its_columns() {
    let centre = [5, -3];
    let near = |x: i32, z: i32| {
        let (dx, dz) = (x - centre[0], z - centre[1]);
        dx * dx + dz * dz <= 64 && (dx, dz) != (3, 1)
    };
    let columns = (-20..30)
        .flat_map(|x| (-25..20).map(move |z| (x, z)))
        .filter(|&(x, z)| near(x, z))
        .map(|(x, z)| ColumnPos::new(x, z));
    let mask = NearMask::from_columns(centre, 64, columns);
    let middle = [centre[0] as f32 * 16.0 + 8.0, centre[1] as f32 * 16.0 + 8.0];
    for x in -20..30 {
        for z in -25..20 {
            assert_eq!(mask.contains(ColumnPos::new(x, z)), near(x, z), "{x},{z}");
            for (bx, bz) in [(0, 0), (15, 0), (0, 15), (15, 15)] {
                let block = [(x * 16 + bx) as f32 + 0.5, (z * 16 + bz) as f32 + 0.5];
                let distance = (block[0] - middle[0]).hypot(block[1] - middle[1]);
                if distance < mask.inner_blocks() {
                    assert!(near(x, z), "{x},{z} is inside the inner disc");
                }
                if near(x, z) {
                    assert!(
                        distance < mask.outer_blocks(),
                        "{x},{z} is past the outer disc"
                    );
                }
            }
        }
    }
    assert!(
        mask.inner_blocks() > 16.0,
        "only the hole limits the inner disc"
    );
    assert_eq!(NearMask::from_columns(centre, 64, []).inner_blocks(), 0.0);
}

/// Inside the near edge but outside the skipped disc, only finest tiles are selected.
#[test]
fn tiles_between_the_skipped_disc_and_the_near_edge_are_finest() {
    let view = TileView {
        skip_blocks: 4.0 * 16.0,
        ..view()
    };
    let tiles = select_tiles(view);
    let between: Vec<_> = tiles
        .iter()
        .filter(|tile| contains(**tile, 8 + 8 * 16, 8))
        .collect();
    assert_eq!(
        between.len(),
        1,
        "a column between the discs is covered once"
    );
    assert_eq!(between[0].level, 0);
    for tile in &tiles {
        let [ox, oz] = tile.origin().map(|v| v as f32);
        let w = tile.width_blocks() as f32;
        let dx = (ox - view.centre[0])
            .abs()
            .max((ox + w - view.centre[0]).abs());
        let dz = (oz - view.centre[1])
            .abs()
            .max((oz + w - view.centre[1]).abs());
        assert!(
            dx.hypot(dz) >= view.skip_blocks,
            "{tile:?} lies wholly inside the skipped disc"
        );
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

/// A coarser tile replaced by finer ones stays drawn until they are built, unless near terrain
/// now draws one of its columns.
#[test]
fn replaced_coarse_tiles_stay_until_covered_or_near() {
    let mut store = SummaryStore::new(usize::MAX);
    for x in -80..80 {
        for z in -80..80 {
            store.admit(ColumnPos::new(x, z), &flat(64));
        }
    }
    let levels = |tiles: &FarTiles, x: i32, z: i32| -> Vec<u8> {
        tiles
            .drawn()
            .iter()
            .filter(|mesh| contains(mesh.key, x, z))
            .map(|mesh| mesh.key.level)
            .collect()
    };
    let coarse = TileView {
        centre: [8.0, 8.0],
        near_blocks: 32.0,
        skip_blocks: 32.0,
        far_blocks: 1200.0,
    };
    let mut tiles = FarTiles::default();
    tiles.set_view(coarse);
    let now = Instant::now();
    test_time::eventually("coarse tiles build", || {
        tiles.update(&mut store, now);
        levels(&tiles, 600, 8) == [2] && levels(&tiles, 600, 300) == [2]
    });

    tiles.set_view(TileView {
        near_blocks: 640.0,
        ..coarse
    });
    assert_eq!(
        levels(&tiles, 600, 300),
        [2],
        "kept until finer tiles are built"
    );
    tiles.release_near(&NearMask::from_columns([0, 0], 64, [ColumnPos::new(37, 0)]));
    assert!(
        levels(&tiles, 600, 8).iter().all(|level| *level == 0),
        "dropped over a near column"
    );
    assert_eq!(levels(&tiles, 600, 300), [2]);
}

/// Columns left far behind by a teleport, not yet evicted, neither size nor enter the mask.
#[test]
fn near_mask_ignores_columns_past_its_limit() {
    let centre = [1_000_000, -1_000_000];
    let near = [ColumnPos::new(centre[0] + 1, centre[1])];
    let stale = [
        ColumnPos::new(0, 0),
        ColumnPos::new(i32::MIN, i32::MAX),
        ColumnPos::new(centre[0] - 40_000, centre[1]),
    ];
    let mask = NearMask::from_columns(centre, 32, near.into_iter().chain(stale));
    assert!(mask.side() <= 2 * 32 + 1);
    assert!(mask.contains(near[0]));
    for column in stale {
        assert!(!mask.contains(column));
    }
}
