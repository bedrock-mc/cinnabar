use super::*;

/// Supplies a full column snapshot for tests that begin with individual keys.
fn retain_keys(source: &TerrainPublications, keys: impl IntoIterator<Item = SubChunkKey>) {
    let mut columns: HashMap<ChunkKey, Vec<SubChunkKey>> = HashMap::new();
    for key in keys {
        columns.entry(key.chunk()).or_default().push(key);
    }
    source.retain(true, columns);
}

#[test]
fn empty_receipts_follow_retention_generations_and_source_identity() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let source = TerrainPublications::default();
    retain_keys(&source, [key]);
    source.record(key, 2, true);
    let (revision, empty) = source.changed_empty(None).unwrap();
    assert_eq!(empty, HashMap::from([(key, 2)]));
    source.record(key, 1, false);
    assert!(source.changed_empty(Some(revision)).is_none());
    source.record(key, 3, false);
    source.record(key, 2, true);
    source.record(key, 3, true);
    assert!(source.changed_empty(None).unwrap().1.is_empty());
    source.record(key, 4, true);
    assert_eq!(source.changed_empty(None).unwrap().1[&key], 4);

    retain_keys(&source, []);
    source.record(key, 5, true);
    assert!(source.0.lock().unwrap().latest.is_empty());
    retain_keys(&source, [key]);
    assert!(source.changed_empty(None).unwrap().1.is_empty());

    let next = TerrainPublications::default();
    retain_keys(&next, [key]);
    let late = source.clone();
    late.record(key, 100, true);
    assert!(!next.same_source(&late));
    assert!(next.changed_empty(None).unwrap().1.is_empty());
    next.record(key, 1, true);
    assert_eq!(next.changed_empty(None).unwrap().1[&key], 1);
}

#[test]
fn unchanged_receipts_do_not_rebuild_the_snapshot() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let source = TerrainPublications::default();
    retain_keys(&source, [key]);
    source.record(key, 1, true);
    let (revision, _) = source.changed_empty(None).unwrap();
    let (_, allocations) = test_alloc::count(|| {
        source.record(key, 1, true);
        assert!(source.changed_empty(Some(revision)).is_none());
    });
    assert_eq!(allocations, 0);
}

#[test]
fn column_deltas_preserve_shared_receipts_and_reject_removed_heights() {
    let source = TerrainPublications::default();
    let extracted = source.clone();
    let first = SubChunkKey::new(0, 0, i32::MIN, 0);
    let kept = SubChunkKey { y: 0, ..first };
    let added = SubChunkKey {
        y: i32::MAX,
        ..first
    };
    let unrelated: Vec<_> = (1..2048).map(|x| SubChunkKey { x, ..kept }).collect();
    retain_keys(
        &source,
        [first, kept].into_iter().chain(unrelated.iter().copied()),
    );
    for key in [first, kept].into_iter().chain(unrelated.iter().copied()) {
        extracted.record(key, 7, true);
    }
    let (before, _) = source.changed_empty(None).unwrap();
    source.retain(false, [(first.chunk(), vec![kept, added, kept])]);
    extracted.record(first, 100, true);
    let (revision, receipts) = source.changed_empty(Some(before)).unwrap();
    assert_eq!(receipts.len(), unrelated.len() + 1);
    assert_eq!(receipts[&kept], 7);
    assert!(unrelated.iter().all(|key| receipts.get(key) == Some(&7)));
    assert!(!receipts.contains_key(&first));
    assert!(!receipts.contains_key(&added));
    assert!(!source.0.lock().unwrap().latest.contains_key(&first));
    let (_, allocations) = test_alloc::count(|| source.retain(false, []));
    assert_eq!(allocations, 0);
    assert!(source.changed_empty(Some(revision)).is_none());

    extracted.record(added, 9, true);
    source.record(added, 9, false);
    extracted.record(added, 9, true);
    assert!(!source.changed_empty(None).unwrap().1.contains_key(&added));
    source.retain(false, [(first.chunk(), vec![])]);
    extracted.record(kept, 100, true);
    assert_eq!(source.changed_empty(None).unwrap().1.len(), unrelated.len());
    source.retain(false, [(first.chunk(), vec![kept])]);
    assert!(!source.changed_empty(None).unwrap().1.contains_key(&kept));
    extracted.record(kept, 10, true);
    retain_keys(&source, [kept]);
    assert_eq!(
        source.changed_empty(None).unwrap().1,
        HashMap::from([(kept, 10)])
    );
}
