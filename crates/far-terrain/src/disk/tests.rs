use super::*;

/// A worker with a real index, without a background consumer.
fn worker(root: &Path, cap: u64) -> Worker {
    Worker {
        root: root.into(),
        bounds: DiskBounds {
            server_cap_bytes: cap,
            global_cap_bytes: cap,
        },
        index: None,
        claimed: HashSet::new(),
        visits: HashMap::new(),
        observed_version: Arc::new(AtomicU64::new(0)),
    }
}

/// A failed oldest unlink stays accounted, later candidates are tried, and retry works.
#[test]
fn failed_eviction_keeps_physical_bytes_in_the_index() {
    let root = tempfile::tempdir().unwrap();
    let mut worker = worker(root.path(), 10);
    let server = ServerKey::from_destination("remote:cache.example:1");
    let paths = ["old", "next", "new"].map(|name| root.path().join(name));
    let mut index = FileIndex::default();
    for path in &paths {
        fs::write(path, [0; 10]).unwrap();
        index.record(path.clone(), server, 10);
    }
    worker.index = Some(index);
    worker.enforce_caps_with(|path| path != paths[0] && remove_cached_file(path));
    assert!(paths[0].exists());
    assert!(!paths[1].exists());
    assert!(!paths[2].exists());
    assert_eq!(worker.index().files.len(), 1);
    worker.bounds.server_cap_bytes = 0;
    worker.enforce_caps();
    assert!(!paths[0].exists());
    assert!(worker.index().files.is_empty());
}

/// A sparse oversized cache file is rejected before allocating its claimed length.
#[test]
fn oversized_region_is_a_cache_miss() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("oversized.lod");
    fs::File::create(&path)
        .unwrap()
        .set_len(crate::region::MAX_REGION_BYTES as u64 + 1)
        .unwrap();
    assert!(read_region(&path).is_none());
}

/// Completed reads retain their admission until the owner consumes the payloads.
#[test]
fn unconsumed_results_bound_focus_requests_and_release_admission() {
    let root = tempfile::tempdir().unwrap();
    let disk = DiskCache::spawn(
        root.path().into(),
        DiskBounds {
            server_cap_bytes: u64::MAX,
            global_cap_bytes: u64::MAX,
        },
    )
    .unwrap();
    let namespace = WorldKey {
        server: ServerKey::from_destination("remote:cache.example:1"),
        dimension: 0,
        level: 1,
        resources: [0; 32],
    }
    .namespace(0);
    let mut admitted = 0;
    for x in 0..1000 {
        admitted += usize::from(disk.load(namespace, RegionPos { x, z: 0 }));
    }
    assert!(admitted > 0 && admitted < 1000);
    disk.sync();
    assert!(!disk.load(namespace, RegionPos { x: 1001, z: 0 }));
    assert_eq!(disk.events().count(), admitted);
    assert!(disk.load(namespace, RegionPos { x: 1001, z: 0 }));
}
