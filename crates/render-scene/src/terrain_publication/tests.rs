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

/// Replayed membership changes, repeats included, leave the same receipts and empty-proof
/// revisions as reconciling the complete section set after every step.
#[test]
fn applied_membership_changes_match_full_reconciliation() {
    let keys: Vec<_> = (0..24).map(|y| SubChunkKey::new(0, y % 3, y, 0)).collect();
    let (full, incremental) = (
        TerrainPublications::default(),
        TerrainPublications::default(),
    );
    let mut members = HashSet::new();
    let mut seed = 0x2545_f491_u64;
    let mut next = |bound: usize| {
        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (seed >> 33) as usize % bound
    };
    for step in 0..2_000 {
        let mut changes = Vec::new();
        for _ in 0..next(5) {
            let key = keys[next(keys.len())];
            if next(2) == 0 {
                members.insert(key);
            } else {
                members.remove(&key);
            }
            changes.push(key);
        }
        // A journal may report a section more than once, or one whose membership returned.
        changes.extend(changes.clone());
        full.retain(members.iter().copied());
        incremental.apply_membership(changes.iter().map(|key| (*key, members.contains(key))));
        for _ in 0..next(4) {
            let (key, generation, empty) = (keys[next(keys.len())], next(6) as u64, next(2) == 0);
            full.record(key, generation + step, empty);
            incremental.record(key, generation + step, empty);
        }
        let (full, incremental) = (full.0.lock().unwrap(), incremental.0.lock().unwrap());
        assert_eq!(full.retained, incremental.retained, "step {step}");
        assert_eq!(full.latest, incremental.latest, "step {step}");
        assert_eq!(full.empty, incremental.empty, "step {step}");
        assert_eq!(full.revision, incremental.revision, "step {step}");
    }
}
