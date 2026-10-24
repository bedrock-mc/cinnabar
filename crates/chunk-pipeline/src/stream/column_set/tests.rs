use super::*;

#[test]
fn column_changes_are_coalesced_consumed_and_owned_by_a_live_cursor() {
    let mut keys = ColumnSubChunkSet::default();
    for x in 0..2048 {
        let key = SubChunkKey::new(0, x, 0, 0);
        keys.insert(key);
        keys.remove(&key);
    }
    assert!(keys.changes.columns.is_empty(), "no unobserved history");
    let cursor = Arc::new(());
    assert!(keys.take_changed_columns(&cursor).is_none());
    let key = SubChunkKey::new(0, 0, 0, 0);
    for y in 0..128 {
        let changed = SubChunkKey { y, ..key };
        keys.insert(changed);
        keys.remove(&changed);
    }
    assert_eq!(keys.changes.columns.len(), 1);
    assert_eq!(
        keys.take_changed_columns(&cursor),
        Some(HashSet::from([key.chunk()]))
    );
    let (changes, allocations) = test_alloc::count(|| keys.take_changed_columns(&cursor));
    assert_eq!(changes, Some(HashSet::new()));
    assert_eq!(allocations, 0);

    keys.insert(key);
    let next = Arc::new(());
    assert!(
        keys.take_changed_columns(&next).is_none(),
        "new owner bootstraps"
    );
    assert!(
        keys.take_changed_columns(&cursor).is_none(),
        "replaced owner bootstraps"
    );
    keys.remove(&key);
    drop(cursor);
    drop(next);
    keys.insert(key);
    assert!(
        keys.changes.columns.is_empty(),
        "dropped owner retains no journal"
    );
}

#[test]
fn changed_columns_include_same_column_replacements_but_not_noops() {
    let mut keys = ColumnSubChunkSet::default();
    let cursor = Arc::new(());
    let first = SubChunkKey::new(0, 0, i32::MIN, 0);
    let next = SubChunkKey {
        y: i32::MAX,
        ..first
    };
    keys.insert(first);
    assert!(keys.take_changed_columns(&cursor).is_none());
    keys.insert(first);
    keys.remove(&next);
    keys.replace_column(first.chunk(), [first, first]);
    assert!(keys.take_changed_columns(&cursor).unwrap().is_empty());
    keys.replace_column(first.chunk(), [next]);
    assert_eq!(
        keys.take_changed_columns(&cursor),
        Some(HashSet::from([first.chunk()]))
    );
    assert_eq!(
        keys.column(first.chunk()).copied().collect::<Vec<_>>(),
        [next]
    );
    keys.remove(&next);
    assert_eq!(
        keys.take_changed_columns(&cursor),
        Some(HashSet::from([first.chunk()]))
    );
    assert_eq!(keys.column(first.chunk()).count(), 0);
}

#[test]
fn retired_and_cloned_indexes_require_an_independent_full_snapshot() {
    let mut keys = ColumnSubChunkSet::default();
    let cursor = Arc::new(());
    let key = SubChunkKey::new(0, 0, 0, 0);
    keys.insert(key);
    assert!(keys.take_changed_columns(&cursor).is_none());
    let mut copied = keys.clone();
    assert!(copied.take_changed_columns(&cursor).is_none());
    assert!(keys.take_changed_columns(&cursor).unwrap().is_empty());
    let mut retired = keys.retire();
    assert!(keys.take_changed_columns(&cursor).is_none());
    assert!(retired.take_changed_columns(&cursor).is_none());
    assert_eq!(
        retired.column(key.chunk()).copied().collect::<Vec<_>>(),
        [key]
    );
    keys.insert(key);
    assert_eq!(
        keys.take_changed_columns(&cursor),
        Some(HashSet::from([key.chunk()]))
    );
}
