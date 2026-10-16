use super::*;

#[test]
fn empty_receipts_follow_retention_generations_and_source_identity() {
    let key = SubChunkKey::new(0, 0, 0, 0);
    let source = TerrainPublications::default();
    source.retain(std::iter::once(key));
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

    source.retain(std::iter::empty());
    source.record(key, 5, true);
    assert!(source.0.lock().unwrap().latest.is_empty());
    source.retain(std::iter::once(key));
    assert!(source.changed_empty(None).unwrap().1.is_empty());

    let next = TerrainPublications::default();
    next.retain(std::iter::once(key));
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
    source.retain(std::iter::once(key));
    source.record(key, 1, true);
    let (revision, _) = source.changed_empty(None).unwrap();
    let (_, allocations) = test_alloc::count(|| {
        source.record(key, 1, true);
        assert!(source.changed_empty(Some(revision)).is_none());
    });
    assert_eq!(allocations, 0);
}
