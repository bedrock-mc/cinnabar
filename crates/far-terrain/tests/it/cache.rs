use std::time::{Duration, Instant};

use far_terrain::{
    ColumnPos, ColumnSummary, FarTerrainCache, FarTerrainConfig, SurfaceCell,
    config::{REGION_FLUSH_AFTER, WORLD_PROBE_COLUMNS},
};

const SERVER: &str = "remote:play.example.net:19132";

fn flat(height: i16) -> ColumnSummary {
    let mut summary = ColumnSummary::default();
    summary.cells_mut().fill(SurfaceCell {
        height,
        rgb: [90, 140, 60],
        water_depth: 0,
    });
    summary
}

fn cache(root: &std::path::Path) -> FarTerrainCache {
    FarTerrainCache::new(FarTerrainConfig::default(), Some(root.to_owned()))
}

/// Admits the probe columns plus one more, then waits for the namespace to resolve.
fn join(cache: &mut FarTerrainCache, height: i16, now: Instant) {
    cache.enter_world(SERVER, 42, 0);
    for x in 0..=WORLD_PROBE_COLUMNS as i32 {
        cache.admit(0, ColumnPos::new(x, 0), flat(height).encode(), now);
    }
    cache.sync();
    cache.poll(now);
    assert!(cache.namespace().is_some());
}

fn leave(cache: &mut FarTerrainCache) {
    cache.leave_world();
    cache.sync();
}

fn reload(cache: &mut FarTerrainCache, pos: ColumnPos, now: Instant) -> Option<ColumnSummary> {
    cache.set_focus(pos, 4);
    cache.sync();
    cache.poll(now);
    cache.store().summary(pos).map(Result::unwrap)
}

#[test]
fn rejoining_the_same_world_reuses_its_cache() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut first = cache(root.path());
    join(&mut first, 64, now);
    first.admit(0, ColumnPos::new(100, 100), flat(80).encode(), now);
    let namespace = first.namespace();
    leave(&mut first);
    drop(first);

    let mut second = cache(root.path());
    join(&mut second, 64, now);
    assert_eq!(second.namespace(), namespace);
    assert_eq!(
        reload(&mut second, ColumnPos::new(100, 100), now),
        Some(flat(80))
    );
}

#[test]
fn a_different_map_under_the_same_identity_starts_a_fresh_namespace() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut cache = cache(root.path());
    join(&mut cache, 64, now);
    let first = cache.namespace().unwrap();
    cache.admit(0, ColumnPos::new(100, 100), flat(80).encode(), now);
    leave(&mut cache);

    join(&mut cache, 20, now);
    let second = cache.namespace().unwrap();
    assert_eq!(second.epoch, first.epoch + 1);
    assert_eq!(reload(&mut cache, ColumnPos::new(100, 100), now), None);
    leave(&mut cache);

    join(&mut cache, 64, now);
    assert_eq!(
        cache.namespace(),
        Some(first),
        "the first map's cache is found again"
    );
}

#[test]
fn writes_wait_for_a_batch_and_dimension_changes_switch_namespace() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut cache = cache(root.path());
    join(&mut cache, 64, now);
    let overworld = cache.namespace().unwrap();
    let region_dir = root
        .path()
        .join(overworld.server.to_string())
        .join(overworld.directory());
    cache.poll(now + Duration::from_millis(10));
    cache.sync();
    assert!(
        !region_dir.exists(),
        "nothing is written before the batch is due"
    );
    cache.poll(now + REGION_FLUSH_AFTER);
    cache.sync();
    assert!(region_dir.exists());

    cache.enter_world(SERVER, 42, -1);
    assert!(
        cache.store().is_empty(),
        "nothing from the overworld stays visible"
    );
    // A column retiring from the old dimension still reaches its own namespace.
    cache.admit(0, ColumnPos::new(500, 0), flat(70).encode(), now);
    cache.sync();
    assert!(cache.store().is_empty());
    leave(&mut cache);
    join(&mut cache, 64, now);
    assert_eq!(cache.namespace(), Some(overworld));
    assert_eq!(
        reload(&mut cache, ColumnPos::new(500, 0), now),
        Some(flat(70))
    );
}
