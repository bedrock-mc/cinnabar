use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, Instant},
};

use far_terrain::{
    ColumnPos, ColumnSummary, FarTerrainCache, FarTerrainConfig, SummaryStore, SurfaceCell,
    config::{REGION_FLUSH_AFTER, WORLD_PROBE_COLUMNS, WORLD_PROBE_TIMEOUT},
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

/// Region files in a namespace directory; resolving reserves the directory itself.
fn region_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir).map_or(0, |entries| {
        entries
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "lod"))
            .count()
    })
}

/// Each cache made by a test starts its versions above every earlier one's, as a new run does.
fn cache(root: &std::path::Path) -> FarTerrainCache {
    with_config(root, FarTerrainConfig::default())
}

fn with_config(root: &std::path::Path, config: FarTerrainConfig) -> FarTerrainCache {
    static RUNS: AtomicU64 = AtomicU64::new(1);
    let base = RUNS.fetch_add(1, Ordering::Relaxed) << 32;
    FarTerrainCache::new(config, Some(root.to_owned()), base)
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
    assert_eq!(
        region_files(&region_dir),
        0,
        "nothing is written before the batch is due"
    );
    cache.poll(now + REGION_FLUSH_AFTER);
    cache.sync();
    assert!(region_files(&region_dir) > 0);

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

/// Returning to an older map whose columns the newest epoch never saw finds the older epoch.
#[test]
fn an_older_matching_epoch_beats_reusing_the_newest() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut cache = cache(root.path());
    join(&mut cache, 64, now);
    let first = cache.namespace().unwrap();
    for x in 200..205 {
        cache.admit(0, ColumnPos::new(x, 0), flat(64).encode(), now);
    }
    leave(&mut cache);
    join(&mut cache, 20, now);
    assert_ne!(cache.namespace(), Some(first));
    leave(&mut cache);

    cache.enter_world(SERVER, 42, 0);
    for x in 200..205 {
        cache.admit(0, ColumnPos::new(x, 0), flat(64).encode(), now);
    }
    cache.sync();
    cache.poll(now);
    assert_eq!(cache.namespace(), Some(first));
}

/// Columns evicted from memory before their region is written still reach disk.
#[test]
fn pending_writes_survive_memory_eviction() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let config = FarTerrainConfig {
        memory_budget_bytes: 1,
        ..FarTerrainConfig::default()
    };
    let mut tiny = with_config(root.path(), config);
    join(&mut tiny, 64, now);
    tiny.admit(0, ColumnPos::new(300, 0), flat(90).encode(), now);
    assert!(tiny.store().is_empty(), "the budget evicted everything");
    leave(&mut tiny);
    drop(tiny);

    let mut cache = cache(root.path());
    join(&mut cache, 64, now);
    assert_eq!(
        reload(&mut cache, ColumnPos::new(300, 0), now),
        Some(flat(90))
    );
}

/// A resolution sent before leaving does not apply to a later visit of the same world.
#[test]
fn a_stale_resolution_is_ignored_after_reentering() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut cache = cache(root.path());
    cache.enter_world(SERVER, 42, 0);
    for x in 0..WORLD_PROBE_COLUMNS as i32 {
        cache.admit(0, ColumnPos::new(x, 0), flat(64).encode(), now);
    }
    cache.leave_world();
    cache.enter_world(SERVER, 42, 0);
    cache.sync();
    cache.poll(now);
    assert_eq!(cache.namespace(), None);
}

/// A cache whose memory holds about two flat columns, so a third admission evicts.
fn two_column_cache(root: &std::path::Path) -> FarTerrainCache {
    let mut probe = SummaryStore::new(usize::MAX);
    probe.admit(ColumnPos::new(0, 0), &flat(64));
    let config = FarTerrainConfig {
        memory_budget_bytes: probe.bytes() * 5 / 2,
        ..FarTerrainConfig::default()
    };
    with_config(root, config)
}

/// Writes `height` for `pos` to disk in an earlier visit.
fn store_on_disk(root: &std::path::Path, pos: ColumnPos, height: i16, now: Instant) {
    let mut earlier = cache(root);
    join(&mut earlier, 64, now);
    earlier.admit(0, pos, flat(height).encode(), now);
    leave(&mut earlier);
}

/// Evicts `pos` from memory by admitting newer columns elsewhere.
fn evict(cache: &mut FarTerrainCache, pos: ColumnPos, now: Instant) {
    for x in 0..3 {
        cache.admit(0, ColumnPos::new(-100 - x, 0), flat(50).encode(), now);
    }
    assert!(!cache.store().contains(pos));
}

/// A newer column evicted from memory before it is written is not replaced by an older read.
#[test]
fn waiting_writes_win_over_disk_reads() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    store_on_disk(root.path(), pos, 70, now);

    let mut cache = two_column_cache(root.path());
    join(&mut cache, 64, now);
    cache.admit(0, pos, flat(90).encode(), now);
    evict(&mut cache, pos, now);
    assert_eq!(reload(&mut cache, pos, now), Some(flat(90)));
}

/// A read served before a newer column's write landed does not replace it.
#[test]
fn a_read_served_before_a_write_keeps_the_newer_column() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    store_on_disk(root.path(), pos, 70, now);

    let mut cache = two_column_cache(root.path());
    join(&mut cache, 64, now);
    cache.admit(0, pos, flat(90).encode(), now);
    evict(&mut cache, pos, now);
    cache.set_focus(pos, 4);
    cache.flush_all();
    cache.sync();
    cache.poll(now);
    assert_eq!(
        cache.store().summary(pos).map(Result::unwrap),
        Some(flat(90))
    );
}

/// Leaving a world before its namespace resolves still writes what it showed.
#[test]
fn leaving_before_resolution_writes_the_columns() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    let mut first = cache(root.path());
    join(&mut first, 64, now);
    leave(&mut first);
    first.enter_world(SERVER, 42, 0);
    first.admit(0, ColumnPos::new(0, 0), flat(64).encode(), now);
    first.admit(0, pos, flat(90).encode(), now);
    assert_eq!(first.namespace(), None);
    leave(&mut first);

    join(&mut first, 64, now);
    assert_eq!(reload(&mut first, pos, now), Some(flat(90)));
}

/// A retirement's summaries reach disk even when the session never resolved its namespace.
#[test]
fn an_unresolved_retirement_is_written() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    let mut cache = cache(root.path());
    join(&mut cache, 64, now);
    leave(&mut cache);
    cache.enter_world(SERVER, 42, 0);
    let session = cache.retire_session().unwrap();
    cache.leave_world();
    let probes = (0..WORLD_PROBE_COLUMNS as i32).map(|x| (ColumnPos::new(x, 0), flat(64).encode()));
    cache.persist(session, probes.chain([(pos, flat(90).encode())]), now);
    cache.sync();

    join(&mut cache, 64, now);
    assert_eq!(reload(&mut cache, pos, now), Some(flat(90)));
}

/// A world with too few non-empty columns to probe still shows and persists them.
#[test]
fn a_sparse_world_resolves_after_the_probe_timeout() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut cache = cache(root.path());
    cache.enter_world(SERVER, 42, 0);
    let pos = ColumnPos::new(7, 7);
    cache.admit(0, pos, flat(64).encode(), now);
    assert!(
        cache.store().contains(pos),
        "shown before the namespace resolves"
    );
    cache.sync();
    cache.poll(now);
    assert_eq!(cache.namespace(), None);

    let later = now + WORLD_PROBE_TIMEOUT;
    cache.poll(later);
    cache.sync();
    cache.poll(later);
    let namespace = cache.namespace().expect("resolved after the timeout");
    leave(&mut cache);
    let region_dir = root
        .path()
        .join(namespace.server.to_string())
        .join(namespace.directory());
    assert!(
        region_files(&region_dir) > 0,
        "the sparse world was written"
    );
}

/// A quick reconnect's columns are not overwritten by the previous session's late summaries.
#[test]
fn a_retiring_session_never_overwrites_a_later_visit() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    let mut cache = cache(root.path());
    join(&mut cache, 64, now);
    let old = cache.retire_session().unwrap();
    leave(&mut cache);

    join(&mut cache, 64, now);
    cache.admit(0, pos, flat(90).encode(), now);
    cache.persist(
        old,
        [
            (pos, flat(70).encode()),
            (ColumnPos::new(301, 0), flat(71).encode()),
        ],
        now,
    );
    assert_eq!(
        cache.store().summary(pos).map(Result::unwrap),
        Some(flat(90))
    );
    assert_eq!(
        cache
            .store()
            .summary(ColumnPos::new(301, 0))
            .map(Result::unwrap),
        Some(flat(71)),
        "columns the new visit has not written are shown"
    );
    leave(&mut cache);

    join(&mut cache, 64, now);
    assert_eq!(reload(&mut cache, pos, now), Some(flat(90)));
}

/// Late summaries of a dimension left before its namespace resolved still reach disk.
#[test]
fn late_summaries_of_an_unresolved_world_are_written() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    let mut cache = cache(root.path());
    cache.enter_world(SERVER, 42, 0);
    cache.admit(0, ColumnPos::new(0, 0), flat(64).encode(), now);
    cache.enter_world(SERVER, 42, -1);
    cache.admit(0, pos, flat(90).encode(), now);
    cache.poll(now);
    leave(&mut cache);

    join(&mut cache, 64, now);
    assert_eq!(reload(&mut cache, pos, now), Some(flat(90)));
}

/// A retirement older than a version being written is not shown in its place.
#[test]
fn a_retirement_never_replaces_a_newer_write_in_flight() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    let mut cache = two_column_cache(root.path());
    join(&mut cache, 64, now);
    let session = cache.retire_session().unwrap();
    cache.admit(0, pos, flat(90).encode(), now);
    cache.flush_all();
    evict(&mut cache, pos, now);
    cache.persist(session, [(pos, flat(70).encode())], now);
    assert_ne!(
        cache.store().summary(pos).map(Result::unwrap),
        Some(flat(70))
    );
}

/// A focused column memory dropped is read back from disk.
#[test]
fn evicted_columns_in_focus_are_read_again() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let pos = ColumnPos::new(300, 0);
    let mut cache = two_column_cache(root.path());
    join(&mut cache, 64, now);
    cache.admit(0, pos, flat(90).encode(), now);
    cache.flush_all();
    cache.sync();
    cache.poll(now);
    cache.set_focus(pos, 4);
    cache.sync();
    cache.poll(now);
    evict(&mut cache, pos, now);
    cache.poll(now);
    cache.sync();
    cache.poll(now);
    assert_eq!(
        cache.store().summary(pos).map(Result::unwrap),
        Some(flat(90))
    );
}

/// Late batches of a world left before it resolved, even without probes, land in one epoch.
#[test]
fn late_batches_of_an_unresolved_world_share_one_epoch() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut cache = cache(root.path());
    cache.enter_world(SERVER, 42, 0);
    cache.enter_world(SERVER, 42, -1);
    for x in [300, 400] {
        cache.admit(0, ColumnPos::new(x, 0), flat(90).encode(), now);
        cache.poll(now);
    }
    cache.sync();
    let server = std::fs::read_dir(root.path())
        .unwrap()
        .flatten()
        .next()
        .expect("a server directory")
        .path();
    let overworld = std::fs::read_dir(server)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("0."))
        .count();
    assert_eq!(overworld, 1, "one overworld namespace");
}

/// A dimension left before its first summary still compares its late columns with the
/// cached maps, so a different map under the same identity gets its own epoch.
#[test]
fn late_columns_probe_before_reusing_a_cached_epoch() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut earlier = cache(root.path());
    join(&mut earlier, 64, now);
    let cached = earlier.namespace().unwrap();
    leave(&mut earlier);
    drop(earlier);

    let mut cache = cache(root.path());
    cache.enter_world(SERVER, 42, 0);
    cache.enter_world(SERVER, 42, -1);
    for x in 0..=WORLD_PROBE_COLUMNS as i32 {
        cache.admit(0, ColumnPos::new(x, 0), flat(20).encode(), now);
    }
    cache.poll(now);
    leave(&mut cache);
    drop(cache);

    let mut later = self::cache(root.path());
    join(&mut later, 64, now);
    assert_eq!(later.namespace(), Some(cached), "the first map is intact");
}

/// Two lobbies under one identity whose probe area matches: the second's live columns
/// differ from what the first cached, so its visit drops the cached map and takes a new epoch.
#[test]
fn a_cached_map_that_differs_from_live_terrain_is_dropped() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut first = cache(root.path());
    join(&mut first, 64, now);
    let shared = first.namespace().unwrap();
    for x in 300..=305 {
        first.admit(0, ColumnPos::new(x, 0), flat(70).encode(), now);
    }
    leave(&mut first);
    drop(first);

    let mut second = cache(root.path());
    join(&mut second, 64, now);
    assert_eq!(second.namespace(), Some(shared), "the probe area matched");
    assert_eq!(
        reload(&mut second, ColumnPos::new(305, 0), now),
        Some(flat(70))
    );
    for x in 300..=303 {
        second.admit(0, ColumnPos::new(x, 0), flat(20).encode(), now);
    }
    assert!(
        !second.store().contains(ColumnPos::new(305, 0)),
        "the other lobby's terrain is no longer shown"
    );
    assert_eq!(
        second
            .store()
            .summary(ColumnPos::new(300, 0))
            .map(Result::unwrap),
        Some(flat(20))
    );
    second.sync();
    second.poll(now);
    let fresh = second.namespace().expect("a fresh epoch");
    assert_ne!(fresh, shared);
}

/// Cached columns stay off screen until the visit's namespace is resolved from live probes.
#[test]
fn nothing_cached_shows_before_the_namespace_resolves() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    store_on_disk(root.path(), ColumnPos::new(300, 0), 70, now);
    let mut cache = cache(root.path());
    cache.enter_world(SERVER, 42, 0);
    cache.admit(0, ColumnPos::new(0, 0), flat(64).encode(), now);
    cache.set_focus(ColumnPos::new(300, 0), 4);
    cache.sync();
    cache.poll(now);
    assert_eq!(cache.namespace(), None);
    assert!(!cache.store().contains(ColumnPos::new(300, 0)));
}

/// A transfer to the same address starts a fresh visit: nothing from the last one stays.
#[test]
fn a_new_visit_to_the_same_world_starts_empty() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut cache = cache(root.path());
    join(&mut cache, 64, now);
    cache.admit(0, ColumnPos::new(300, 0), flat(70).encode(), now);
    cache.begin_visit(SERVER, 42, 0);
    assert!(cache.store().is_empty());
    assert_eq!(cache.namespace(), None, "resolved again from new probes");
}

/// When a visit turns out to be another map, what it already wrote moves to its fresh epoch,
/// so its visited terrain stays cached for it and leaves the other map's epoch.
#[test]
fn a_diverged_visit_keeps_what_it_already_wrote() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut first = cache(root.path());
    join(&mut first, 64, now);
    for x in 300..=305 {
        first.admit(0, ColumnPos::new(x, 0), flat(70).encode(), now);
    }
    leave(&mut first);
    drop(first);

    let mut second = cache(root.path());
    join(&mut second, 64, now);
    let shared = second.namespace().unwrap();
    assert_eq!(
        reload(&mut second, ColumnPos::new(305, 0), now),
        Some(flat(70))
    );
    let visited = ColumnPos::new(500, 0);
    second.admit(0, visited, flat(33).encode(), now);
    second.flush_all();
    second.sync();
    second.poll(now);
    for x in 300..=303 {
        second.admit(0, ColumnPos::new(x, 0), flat(20).encode(), now);
    }
    second.sync();
    second.poll(now);
    assert_ne!(second.namespace(), Some(shared));
    leave(&mut second);
    drop(second);

    let mut later = self::cache(root.path());
    join(&mut later, 64, now);
    assert_ne!(
        later.namespace(),
        Some(shared),
        "the fresh epoch matches first"
    );
    assert_eq!(reload(&mut later, visited, now), Some(flat(33)));
}

/// A full region arrives over several polls without exceeding the column admission limit.
#[test]
fn full_region_completions_are_spread_across_polls() {
    use far_terrain::{REGION_SIDE, config::SUMMARY_COLUMNS_PER_UPDATE};
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let mut writer = cache(root.path());
    join(&mut writer, 64, now);
    let encoded = flat(80).encode();
    for z in 0..REGION_SIDE {
        for x in 0..REGION_SIDE {
            writer.admit(
                0,
                ColumnPos::new(10 * REGION_SIDE + x, z),
                encoded.clone(),
                now,
            );
        }
    }
    leave(&mut writer);
    drop(writer);
    let mut reader = cache(root.path());
    join(&mut reader, 64, now);
    let before = reader.store().len();
    reader.set_focus(
        ColumnPos::new(10 * REGION_SIDE + REGION_SIDE / 2, REGION_SIDE / 2),
        1,
    );
    reader.sync();
    reader.poll(now);
    assert_eq!(reader.store().len() - before, SUMMARY_COLUMNS_PER_UPDATE);
    let mut previous = reader.store().len();
    for _ in 0..REGION_SIDE * REGION_SIDE {
        reader.poll(now);
        let current = reader.store().len();
        assert!(current - previous <= SUMMARY_COLUMNS_PER_UPDATE);
        previous = current;
        if current == before + (REGION_SIDE * REGION_SIDE) as usize {
            break;
        }
    }
    assert_eq!(
        reader.store().len(),
        before + (REGION_SIDE * REGION_SIDE) as usize
    );
}
