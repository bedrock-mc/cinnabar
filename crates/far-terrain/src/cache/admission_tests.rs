use super::*;

/// A non-empty terrain probe that can match a persisted map.
fn surface(height: i16) -> EncodedSummary {
    let mut summary = ColumnSummary::default();
    summary.cells_mut().fill(crate::SurfaceCell {
        height,
        rgb: [90, 140, 60],
        water_depth: 0,
    });
    summary.encode()
}

/// Starts a resolved visit above a persisted older column's version.
fn resolved_cache(root: &std::path::Path, level: u64) -> (FarTerrainCache, NamespaceKey) {
    let mut cache = FarTerrainCache::new(FarTerrainConfig::default(), Some(root.into()), 100);
    cache.enter_world("remote:cache.example:1", level, 0);
    let namespace = cache.world.unwrap().namespace(0);
    cache.namespace = Some(namespace);
    assert!(cache.disk.as_ref().unwrap().write(
        namespace,
        RegionPos::of(ColumnPos::new(0, 0)),
        vec![(
            0,
            Versioned {
                version: 1,
                summary: surface(64)
            }
        )],
    ));
    cache.sync();
    cache.disk.as_ref().unwrap().events().for_each(drop);
    (cache, namespace)
}

/// Occupies every operation slot with unread results, without blocking the IO worker.
fn fill_admission(cache: &FarTerrainCache) {
    let disk = cache.disk.as_ref().unwrap();
    for _ in 0..crate::disk::MAX_PENDING_OPERATIONS {
        assert!(disk.resolve(
            cache.world.unwrap(),
            u64::MAX,
            ResolutionIntent::Probed,
            Vec::new()
        ));
    }
    cache.sync();
    assert!(!disk.resolve(
        cache.world.unwrap(),
        u64::MAX,
        ResolutionIntent::Probed,
        Vec::new()
    ));
}

/// Reads the worker's stored column after earlier commands have settled.
fn stored(cache: &FarTerrainCache, namespace: NamespaceKey, pos: ColumnPos) -> Option<Versioned> {
    let disk = cache.disk.as_ref().unwrap();
    assert!(disk.load(namespace, RegionPos::of(pos)));
    cache.sync();
    disk.events().find_map(|event| match event {
        crate::DiskEvent::Loaded { columns, .. } => columns
            .into_iter()
            .find_map(|(column_pos, column)| (column_pos == pos).then_some(column)),
        _ => None,
    })
}

/// A refused empty-probe reservation cannot later reuse the matching lobby's epoch.
#[test]
fn uncertain_visit_keeps_fresh_intent_after_admission_retry() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let (mut cache, previous) = resolved_cache(root.path(), 0);
    fill_admission(&cache);
    cache.begin_uncertain_visit("remote:cache.example:1", 0);
    assert!(!cache.resolving);
    cache.admit(0, ColumnPos::new(0, 0), surface(64), now);
    cache.disk.as_ref().unwrap().events().for_each(drop);
    cache.poll(now + WORLD_PROBE_TIMEOUT);
    cache.sync();
    cache.poll(now + WORLD_PROBE_TIMEOUT);
    assert_ne!(cache.namespace().unwrap(), previous);
}

/// Retiring an uncertain visit preserves fresh identity for its final matching columns.
#[test]
fn uncertain_retirement_keeps_fresh_intent_after_admission_retry() {
    let root = tempfile::tempdir().unwrap();
    let now = Instant::now();
    let (mut cache, previous) = resolved_cache(root.path(), 0);
    let pos = ColumnPos::new(0, 0);
    let previous_version = stored(&cache, previous, pos).unwrap().version;
    fill_admission(&cache);
    cache.begin_uncertain_visit("remote:cache.example:1", 0);
    let session = cache.retire_session().unwrap();
    cache.leave_world();
    cache.disk.as_ref().unwrap().events().for_each(drop);
    let mut session = session;
    session.add_probe(pos, &surface(64));
    cache.persist(session, [(pos, surface(64))], now);
    cache.sync();
    assert_eq!(
        stored(&cache, previous, pos).unwrap().version,
        previous_version
    );
}

/// A deferred migration survives departure, even with no columns left to trigger a write.
#[test]
fn diverged_retirement_retries_migration_before_late_batches() {
    for reconnects in 0..=2 {
        let root = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let (mut cache, previous) = resolved_cache(root.path(), 42);
        let pos = ColumnPos::new(300, 0);
        cache.admit(0, pos, surface(90), now);
        cache.flush_all();
        cache.sync();
        cache.poll(now);
        fill_admission(&cache);
        cache.diverge();
        assert!(!cache.resolving);
        let session = cache.retire_session().unwrap();
        cache.leave_world();
        for visit in 0..reconnects {
            cache.begin_uncertain_visit("remote:cache.example:1", visit);
        }
        if reconnects > 1 {
            cache.admit(0, pos, surface(20), now);
        }
        cache.disk.as_ref().unwrap().events().for_each(drop);
        cache.poll(now);
        cache.sync();
        assert!(stored(&cache, previous, pos).is_none());
        let disk = cache.disk.as_ref().unwrap();
        assert!(disk.resolve(session.world, session.entry, session.intent, Vec::new()));
        cache.sync();
        let migrated = disk
            .events()
            .find_map(|event| match event {
                crate::DiskEvent::Resolved { epoch, .. } => Some(session.world.namespace(epoch)),
                _ => None,
            })
            .unwrap();
        assert_ne!(migrated, previous);
        assert_eq!(
            stored(&cache, previous, ColumnPos::new(0, 0))
                .unwrap()
                .summary
                .hash,
            surface(64).hash
        );
        assert_eq!(
            stored(&cache, migrated, pos).unwrap().summary.hash,
            surface(90).hash
        );
        for x in [400, 500] {
            let late = ColumnPos::new(x, 0);
            cache.persist(session.clone(), [(late, surface(80))], now);
            cache.sync();
            assert_eq!(
                stored(&cache, migrated, late).unwrap().summary.hash,
                surface(80).hash
            );
            assert!(stored(&cache, previous, late).is_none());
        }
    }
}

/// Repeated edits coalesce and novel regions cannot grow retained maps without a consumer.
#[test]
fn pending_region_reservations_bound_dirty_and_in_flight_payloads() {
    let root = tempfile::tempdir().unwrap();
    let mut cache = FarTerrainCache::new(FarTerrainConfig::default(), Some(root.path().into()), 1);
    cache.enter_world("remote:cache.example:1", 1, 0);
    let namespace = cache.world.unwrap().namespace(0);
    cache.namespace = Some(namespace);
    let now = Instant::now();
    let summary = ColumnSummary::default().encode();
    for version in 1..=1000 {
        let pos = ColumnPos::new(version as i32 * REGION_SIDE, 0);
        cache.queue_write(
            namespace,
            pos,
            Versioned {
                version,
                summary: summary.clone(),
            },
            now,
        );
        cache.flush_all();
        assert!(cache.dirty.len() <= MAX_PENDING_WRITE_REGIONS);
        assert!(cache.writing.len() <= MAX_PENDING_WRITE_REGIONS);
    }
    let pos = *cache
        .dirty
        .values()
        .next()
        .unwrap()
        .columns
        .keys()
        .next()
        .unwrap();
    cache.queue_write(
        namespace,
        pos,
        Versioned {
            version: 2000,
            summary: summary.clone(),
        },
        now,
    );
    cache.queue_write(
        namespace,
        pos,
        Versioned {
            version: 2001,
            summary,
        },
        now,
    );
    // Free admission and let the pending newest version progress.
    cache.sync();
    for _ in 0..20 {
        cache.poll(now);
        cache.flush_all();
        cache.sync();
    }
    cache
        .disk
        .as_ref()
        .unwrap()
        .load(namespace, RegionPos::of(pos));
    cache.sync();
    let versions: Vec<_> = cache
        .disk
        .as_ref()
        .unwrap()
        .events()
        .flat_map(|event| match event {
            crate::DiskEvent::Loaded { columns, .. } => columns,
            _ => Vec::new(),
        })
        .collect();
    assert!(
        versions
            .iter()
            .any(|(loaded, column)| *loaded == pos && column.version == 2001)
    );
}

/// Caller corruption and exhausted persisted clocks cannot wrap live ordering.
#[test]
fn exhausted_clocks_recover_before_admission_and_new_visits() {
    let now = Instant::now();
    for base in [u64::MAX, u64::MAX - 1, crate::region::MAX_VERSION - 1] {
        let mut cache = FarTerrainCache::new(FarTerrainConfig::default(), None, base);
        cache.enter_world("remote:cache.example:1", 1, 0);
        for _ in 0..3 {
            cache.admit(
                0,
                ColumnPos::new(0, 0),
                ColumnSummary::default().encode(),
                now,
            );
        }
        assert!(cache.store.version(ColumnPos::new(0, 0)).unwrap() < crate::region::MAX_VERSION);
        cache.begin_visit("remote:cache.example:1", 2, 0);
    }
}

/// A retirement captured before recovery cannot restore the abandoned high clock afterward.
#[test]
fn clock_recovery_invalidates_outstanding_retirement_handles() {
    let now = Instant::now();
    let mut cache = FarTerrainCache::new(
        FarTerrainConfig::default(),
        None,
        crate::region::MAX_VERSION - 1,
    );
    cache.enter_world("remote:cache.example:1", 1, 0);
    cache.start_resolving();
    let retired = cache.retire_session().unwrap();
    let pos = ColumnPos::new(0, 0);
    let live = ColumnSummary::default().encode();
    cache.admit(0, pos, live.clone(), now);
    let live_version = cache.store.version(pos).unwrap();
    cache.persist(retired, [(pos, live)], now);
    assert_eq!(cache.store.version(pos), Some(live_version));
}

/// A new retirement uses the recovered visit identity and can still publish its summaries.
#[test]
fn retirement_started_during_clock_recovery_remains_usable() {
    let now = Instant::now();
    let mut cache = FarTerrainCache::new(FarTerrainConfig::default(), None, 1);
    cache.enter_world("remote:cache.example:1", 1, 0);
    cache.start_resolving();
    cache.next_version = crate::region::MAX_VERSION;
    let session = cache.retire_session().unwrap();
    let pos = ColumnPos::new(0, 0);
    cache.persist(session, [(pos, ColumnSummary::default().encode())], now);
    assert_eq!(cache.store.version(pos), Some(1));
}
