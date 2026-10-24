use super::*;

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
