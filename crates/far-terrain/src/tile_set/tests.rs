use super::*;
use crate::{SurfaceCell, Versioned};

/// A level-four tile and all of its halo, with shared flat encodings.
fn full_tile() -> (TileKey, SummaryStore) {
    let key = TileKey {
        level: LEVEL_COUNT - 1,
        x: 0,
        z: 0,
    };
    let side = key.width_blocks() as usize / COLUMN_SIDE + 2;
    let mut summary = ColumnSummary::default();
    summary.cells_mut().fill(SurfaceCell {
        height: 64,
        rgb: [10, 200, 30],
        water_depth: 0,
    });
    let encoded = summary.encode();
    let mut store = SummaryStore::new(usize::MAX);
    for index in 0..side * side {
        store.admit_versioned(
            ColumnPos::new((index % side) as i32 - 1, (index / side) as i32 - 1),
            Versioned {
                version: 1,
                summary: encoded.clone(),
            },
        );
    }
    (key, store)
}

/// Even the first level-four tile must carry its remaining halo lookups into later updates.
#[test]
fn gathering_counts_every_halo_lookup_and_resumes() {
    let (key, mut store) = full_tile();
    let mut tiles = FarTiles::default();
    tiles.selected.insert(key, Tile::default());
    let now = Instant::now();
    for frame in 1..=4 {
        tiles.update(&mut store, now);
        let gather = tiles
            .gathering
            .as_ref()
            .expect("large tile still gathering");
        assert_eq!(gather.next, frame * TILE_BUILD_COLUMNS_PER_UPDATE);
        assert_eq!(gather.columns.len(), gather.next);
        for (pos, encoded) in &gather.columns {
            assert!(Arc::ptr_eq(
                &encoded.bytes,
                &store.encoded(*pos).unwrap().bytes
            ));
        }
        assert!(!tiles.ready(ColumnPos::new(0, 0)));
    }
    tiles.update(&mut store, now);
    assert!(tiles.gathering.is_none());
    test_time::eventually("gathered tile builds", || {
        tiles.update(&mut store, now);
        tiles.ready(ColumnPos::new(0, 0))
    });
}

/// Retained bytes stop gathering before the column cap, without repeating earlier lookups.
#[test]
fn gathering_also_limits_retained_bytes() {
    let (key, store) = full_tile();
    let bytes = store.encoded(ColumnPos::new(-1, -1)).unwrap().bytes.len();
    let mut gather = Gathering {
        key,
        token: 1,
        next: 0,
        columns: Vec::new(),
    };
    assert!(!gather.advance(&store, &mut SummaryBudget::new(100, 2 * bytes)));
    assert_eq!(gather.next, 2);
    assert!(!gather.advance(&store, &mut SummaryBudget::new(100, bytes)));
    assert_eq!(gather.next, 3);
}

/// Resuming a snapshot preserves exactly the grid, skirt and colour inputs of a full gather.
#[test]
fn resumed_gathers_preserve_the_mesh() {
    let (key, store) = full_tile();
    let mut gather = Gathering {
        key,
        token: 1,
        next: 0,
        columns: Vec::new(),
    };
    while !gather.advance(&store, &mut SummaryBudget::new(700, usize::MAX)) {}
    let decoded: Vec<_> = gather
        .columns
        .into_iter()
        .map(|(pos, encoded)| (pos, ColumnSummary::decode(&encoded.bytes).unwrap()))
        .collect();
    let resumed = mesh_tile(&TileGrid::build(key, &decoded));
    let [x, z] = key.origin().map(|block| block / COLUMN_SIDE as i32);
    let side = key.width_blocks() / COLUMN_SIDE as i32;
    let expected: Vec<_> = (z - 1..=z + side)
        .flat_map(|cz| (x - 1..=x + side).map(move |cx| ColumnPos::new(cx, cz)))
        .map(|pos| {
            (
                pos,
                ColumnSummary::decode(&store.encoded(pos).unwrap().bytes).unwrap(),
            )
        })
        .collect();
    let expected = mesh_tile(&TileGrid::build(key, &expected));
    assert_eq!(resumed.quads, expected.quads);
    assert_eq!(resumed.colours, expected.colours);
    assert_eq!(
        (resumed.min_y, resumed.max_y),
        (expected.min_y, expected.max_y)
    );
}

/// Missing halo entries still consume work, while an empty tile needs no surface allocation.
#[test]
fn empty_gathers_are_bounded_and_finish_without_a_grid() {
    let key = TileKey {
        level: LEVEL_COUNT - 1,
        x: 0,
        z: 0,
    };
    let mut tiles = FarTiles::default();
    tiles.selected.insert(key, Tile::default());
    let mut store = SummaryStore::new(0);
    let now = Instant::now();
    for _ in 0..4 {
        tiles.update(&mut store, now);
        assert!(!tiles.ready(ColumnPos::new(0, 0)));
    }
    tiles.update(&mut store, now);
    assert!(
        !tiles.ready(ColumnPos::new(0, 0)),
        "missing summaries are not empty evidence"
    );
    assert_eq!(tiles.empty_columns().count(), 0);
    let mesh = tiles.selected[&key].mesh.as_ref().unwrap();
    assert!(mesh.quads.is_empty() && mesh.colours.is_empty());
    assert_eq!((mesh.min_y, mesh.max_y), (0, 0));
}

/// Editing an already gathered column keeps the snapshot unready until its replacement builds.
#[test]
fn edits_during_gathering_are_not_reported_ready() {
    let (key, mut store) = full_tile();
    let mut tiles = FarTiles::default();
    tiles.selected.insert(key, Tile::default());
    let now = Instant::now();
    tiles.update(&mut store, now);
    let pos = ColumnPos::new(0, 0);
    store.admit(pos, &ColumnSummary::default());
    tiles.mark_changed(&[pos]);
    for _ in 0..4 {
        tiles.update(&mut store, now);
    }
    test_time::eventually("dirty snapshot finishes", || {
        tiles.update(&mut store, now);
        tiles.selected[&key].mesh.is_some()
    });
    assert!(!tiles.ready(pos));
    let later = now + TILE_REBUILD_INTERVAL;
    test_time::eventually("latest summary rebuilds", || {
        tiles.update(&mut store, later);
        tiles.ready(pos)
    });
}

/// Clearing a namespace abandons partial gathering without reserving a worker slot.
#[test]
fn clearing_abandons_partial_gathers() {
    let (key, mut store) = full_tile();
    let mut tiles = FarTiles::default();
    tiles.selected.insert(key, Tile::default());
    tiles.update(&mut store, Instant::now());
    tiles.clear();
    assert!(tiles.gathering.is_none());
    assert_eq!(tiles.in_flight, 0);
}

/// A missing or corrupt interior remains unknown even when another decoded summary is empty.
#[test]
fn empty_summary_evidence_is_limited_to_its_decoded_column() {
    let key = TileKey {
        level: 0,
        x: 0,
        z: 0,
    };
    let known = ColumnPos::new(0, 0);
    let corrupt = ColumnPos::new(1, 0);
    let missing = ColumnPos::new(2, 0);
    let mismatched_hash = ColumnPos::new(3, 0);
    let mut store = SummaryStore::new(usize::MAX);
    store.admit(known, &ColumnSummary::default());
    store.admit(ColumnPos::new(-1, 0), &ColumnSummary::default());
    store.admit_versioned(
        corrupt,
        Versioned {
            version: 1,
            summary: EncodedSummary {
                hash: 42,
                bytes: Arc::from([255_u8]),
            },
        },
    );
    let mut invalid = ColumnSummary::default().encode();
    invalid.hash ^= 1;
    store.admit_versioned(
        mismatched_hash,
        Versioned {
            version: 1,
            summary: invalid,
        },
    );
    let mut tiles = FarTiles::default();
    tiles.selected.insert(key, Tile::default());
    let now = Instant::now();
    test_time::eventually("partial empty tile builds", || {
        tiles.update(&mut store, now);
        tiles.selected[&key].mesh.is_some()
    });
    assert!(tiles.ready(known));
    assert!(!tiles.ready(corrupt));
    assert!(!tiles.ready(missing));
    assert!(!tiles.ready(mismatched_hash));
    assert_eq!(tiles.empty_columns().collect::<Vec<_>>(), [known]);
    let mesh = tiles.ready_mesh(known).unwrap();
    assert!(mesh.quads.is_empty());
    assert!(!mesh.covers(corrupt) && !mesh.covers(missing));
}

/// Partial rebuilds retain the visible fallback both at the same key and across LOD changes.
#[test]
fn incomplete_summaries_cannot_retire_visible_coverage() {
    for split in [false, true] {
        let old_key = TileKey {
            level: 1,
            x: 0,
            z: 0,
        };
        let columns: Vec<_> = old_key.columns().collect();
        let mut solid = ColumnSummary::default();
        solid.cells_mut().fill(SurfaceCell {
            height: 64,
            rgb: [80; 3],
            water_depth: 0,
        });
        let mut store = SummaryStore::new(usize::MAX);
        for &column in &columns {
            store.admit(column, &solid);
        }
        let mut tiles = FarTiles::default();
        tiles.selected.insert(old_key, Tile::default());
        let now = Instant::now();
        test_time::eventually("original solid coverage builds", || {
            tiles.update(&mut store, now);
            tiles.ready(columns[0])
        });
        let old = tiles.drawn()[0].clone();
        if split {
            tiles.selected.clear();
            for (x, z) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                tiles
                    .selected
                    .insert(TileKey { level: 0, x, z }, Tile::default());
            }
            tiles.retire(old.clone());
        }
        store = SummaryStore::new(usize::MAX);
        store.admit(columns[0], &ColumnSummary::default());
        tiles.mark_changed(&columns);
        let later = now + TILE_REBUILD_INTERVAL;
        test_time::eventually("partial replacements finish", || {
            tiles.update(&mut store, later);
            tiles
                .selected
                .values()
                .all(|tile| tile.mesh.is_some() && tile.building.is_none() && !tile.dirty)
        });
        assert_eq!(tiles.drawn(), [old]);
        assert!(columns.iter().all(|column| !tiles.ready(*column)));
        assert_eq!(tiles.empty_columns().count(), 0);
        for &column in &columns {
            store.admit(column, &ColumnSummary::default());
        }
        tiles.mark_changed(&columns);
        let later = later + TILE_REBUILD_INTERVAL;
        test_time::eventually("complete empty replacements retire the fallback", || {
            tiles.update(&mut store, later);
            columns.iter().all(|column| tiles.ready(*column))
        });
        assert!(tiles.drawn().is_empty());
        assert_eq!(
            tiles
                .empty_columns()
                .collect::<std::collections::BTreeSet<_>>(),
            columns.iter().copied().collect()
        );
    }
}
