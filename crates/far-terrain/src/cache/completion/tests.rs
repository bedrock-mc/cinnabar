use super::*;

/// An active cache with one partial disk read ready for application.
fn reading(columns: Vec<(ColumnPos, Versioned)>) -> (FarTerrainCache, NamespaceKey) {
    let mut cache = FarTerrainCache::new(FarTerrainConfig::default(), None, 100);
    cache.enter_world("remote:cache.example.net:19132", 1, 0);
    let namespace = cache.world.unwrap().namespace(0);
    cache.namespace = Some(namespace);
    cache.pending_event = Some(PendingEvent::Loaded {
        namespace,
        region: RegionPos { x: 0, z: 0 },
        columns: columns.into_iter(),
        after: None,
    });
    (cache, namespace)
}

/// A payload of a known size; cache application need not decode it.
fn column(version: u64, bytes: usize) -> Versioned {
    Versioned {
        version,
        summary: EncodedSummary {
            hash: version,
            bytes: vec![0; bytes].into(),
        },
    }
}

/// The byte bound leaves an event pending, and an oversized first column still makes progress.
#[test]
fn loaded_events_resume_at_the_byte_limit() {
    let positions = [
        ColumnPos::new(0, 0),
        ColumnPos::new(1, 0),
        ColumnPos::new(2, 0),
    ];
    let (mut cache, _) = reading(
        positions
            .into_iter()
            .map(|pos| (pos, column(1, 80)))
            .collect(),
    );
    let now = Instant::now();
    cache.poll_budgeted(now, &mut SummaryBudget::new(10, 100));
    assert_eq!(cache.store.len(), 1);
    assert!(cache.pending_event.is_some());
    cache.poll_budgeted(now, &mut SummaryBudget::new(10, 1));
    assert_eq!(cache.store.len(), 2);
    cache.poll_budgeted(now, &mut SummaryBudget::new(10, 100));
    assert_eq!(cache.store.len(), 3);
    assert!(cache.pending_event.is_none());
}

/// Queued writes absent from disk are restored too, and the highest version wins each slice.
#[test]
fn partial_reads_keep_waiting_and_in_flight_versions() {
    let a = ColumnPos::new(0, 0);
    let b = ColumnPos::new(1, 0);
    let (mut cache, namespace) = reading(vec![(a, column(1, 10))]);
    cache.dirty.insert(
        (namespace, RegionPos::of(a)),
        DirtyRegion {
            since: Instant::now(),
            columns: BTreeMap::from([(a, column(4, 10)), (b, column(5, 10))]),
        },
    );
    cache.writing.insert(
        (namespace, RegionPos::of(a)),
        BTreeMap::from([(a, column(6, 10))]),
    );
    let now = Instant::now();
    cache.poll_budgeted(now, &mut SummaryBudget::new(1, 100));
    assert_eq!(cache.store.version(a), Some(6));
    cache.store.admit_versioned(a, column(10, 10));
    for _ in 0..3 {
        cache.poll_budgeted(now, &mut SummaryBudget::new(1, 100));
    }
    assert_eq!(cache.store.version(a), Some(10));
    assert_eq!(cache.store.version(b), Some(5));
}

/// A written acknowledgement cannot clear a newer queued write, even when split across polls.
#[test]
fn partial_acknowledgements_keep_newer_writes() {
    let a = ColumnPos::new(0, 0);
    let b = ColumnPos::new(1, 0);
    let (mut cache, namespace) = reading(Vec::new());
    let region = RegionPos::of(a);
    cache.writing.insert(
        (namespace, region),
        BTreeMap::from([(a, column(5, 10)), (b, column(7, 10))]),
    );
    cache.pending_event = Some(PendingEvent::Written {
        namespace,
        columns: vec![(a, 5), (b, 6)].into_iter(),
    });
    let now = Instant::now();
    cache.poll_budgeted(now, &mut SummaryBudget::new(1, 100));
    assert!(!cache.writing[&(namespace, region)].contains_key(&a));
    assert!(cache.pending_event.is_some());
    cache.poll_budgeted(now, &mut SummaryBudget::new(1, 100));
    assert_eq!(cache.writing[&(namespace, region)][&b].version, 7);
}

/// A namespace switch rejects the remainder of an old read instead of mixing its columns.
#[test]
fn changing_world_rejects_partial_reads() {
    let a = ColumnPos::new(0, 0);
    let b = ColumnPos::new(1, 0);
    let (mut cache, _) = reading(vec![(a, column(1, 10)), (b, column(2, 10))]);
    let now = Instant::now();
    cache.poll_budgeted(now, &mut SummaryBudget::new(1, 100));
    cache.begin_visit("remote:cache.example.net:19132", 2, 0);
    cache.poll_budgeted(now, &mut SummaryBudget::default());
    assert!(cache.store.is_empty());
}

/// Empty events also stop at the event cap instead of draining the whole channel.
#[test]
fn empty_events_obey_the_event_limit() {
    let root = tempfile::tempdir().unwrap();
    let (mut cache, namespace) = reading(Vec::new());
    cache.disk = DiskCache::spawn(
        root.path().into(),
        DiskBounds {
            server_cap_bytes: u64::MAX,
            global_cap_bytes: u64::MAX,
        },
    );
    let disk = cache.disk.as_ref().unwrap();
    for x in 0..CACHE_EVENTS_PER_UPDATE + 10 {
        assert!(disk.write(namespace, RegionPos { x: x as i32, z: 0 }, Vec::new()));
    }
    disk.sync();
    cache.poll(Instant::now());
    assert_eq!(cache.disk.as_ref().unwrap().events().count(), 10);
}

/// A read's full version watermark is known before any of its deferred columns can be edited.
#[test]
fn partial_reads_reserve_all_disk_versions_before_live_edits() {
    let root = tempfile::tempdir().unwrap();
    let (mut cache, namespace) = reading(Vec::new());
    cache.disk = DiskCache::spawn(
        root.path().into(),
        DiskBounds {
            server_cap_bytes: u64::MAX,
            global_cap_bytes: u64::MAX,
        },
    );
    let a = ColumnPos::new(0, 0);
    let b = ColumnPos::new(1, 0);
    let region = RegionPos::of(a);
    let disk = cache.disk.as_ref().unwrap();
    disk.write(
        namespace,
        region,
        vec![
            (region_index(a), column(500, 10)),
            (region_index(b), column(9000, 10)),
        ],
    );
    disk.load(namespace, region);
    disk.sync();
    let now = Instant::now();
    cache.poll_budgeted(now, &mut SummaryBudget::new(1, 100));
    cache.admit(0, b, ColumnSummary::default().encode(), now);
    let live = cache.store.version(b).unwrap();
    assert!(live > 9000);
    cache.poll(now);
    assert_eq!(cache.store.version(b), Some(live));
}
