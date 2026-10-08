use super::*;
use std::collections::{HashSet, VecDeque};

/// Builds populated columns with request-mode block authority.
fn populated_store(columns: i32) -> ChunkStore {
    let mut store = ChunkStore::new();
    for x in 0..columns {
        for y in 0..16 {
            let key = SubChunkKey::new(0, x, y, 0);
            store
                .update_block(key, BlockUpdate::new(0, 0, 0, 0, 1), 0)
                .unwrap();
            store.mark_sub_chunk_loaded(key).unwrap();
        }
    }
    store
}

/// Unchanged ticks share all indexes and the live store never owns the retained world.
#[test]
fn unchanged_ticks_share_indexes_without_retaining_a_history() {
    let store = populated_store(1000);
    let first = store.collision_snapshot();
    for _ in 0..10_000 {
        assert!(Arc::ptr_eq(&first, &store.collision_snapshot()));
    }
    let weak = Arc::downgrade(&first);
    drop(first);
    assert!(weak.upgrade().is_none());
}

/// Changing one column copies only its subchunk index, bounded by history length.
#[test]
fn changed_columns_share_untouched_indexes_under_history_churn() {
    let mut store = populated_store(64);
    let mut history = VecDeque::new();
    const HISTORY: usize = 32;
    for tick in 0..10_000 {
        let key = SubChunkKey::new(0, tick % 64, 0, 0);
        store
            .update_block(key, BlockUpdate::new(1, 0, 0, 0, 1 + (tick / 64) as u32), 0)
            .unwrap();
        if history.len() == HISTORY {
            history.pop_front();
        }
        history.push_back(store.collision_snapshot());
        if tick % 100 == 0 {
            let indexes: HashSet<_> = history
                .iter()
                .flat_map(|snapshot| snapshot.chunks.values())
                .chain(store.chunks.values())
                .map(|chunk| Arc::as_ptr(&chunk.sub_chunks))
                .collect();
            assert!(indexes.len() <= store.chunks.len() + HISTORY);
        }
    }
    let oldest = Arc::downgrade(history.front().unwrap());
    history.pop_front();
    assert!(oldest.upgrade().is_none());
    let newest = Arc::downgrade(history.back().unwrap());
    drop(history);
    assert!(newest.upgrade().is_none());
}

/// Availability, mutation and eviction retire the cached generation atomically.
#[test]
fn snapshot_preserves_authority_and_palettes_across_replacements() {
    let mut store = populated_store(2);
    let key = SubChunkKey::new(0, 0, 0, 0);
    let old = store.collision_snapshot();
    let revision = old.collision_revision(key.chunk());
    store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 2), 0)
        .unwrap();
    let changed = store.collision_snapshot();
    assert!(!Arc::ptr_eq(&old, &changed));
    assert_eq!(old.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0), Some(1));
    assert_eq!(
        changed.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(2)
    );
    assert_eq!(old.collision_revision(key.chunk()), revision);
    assert_ne!(changed.collision_revision(key.chunk()), revision);
    let untouched = ChunkKey::new(0, 1, 0);
    assert!(Arc::ptr_eq(
        &old.chunk(untouched).unwrap().sub_chunks,
        &changed.chunk(untouched).unwrap().sub_chunks,
    ));
    let air = SubChunkKey::new(0, 2, 0, 0);
    store.mark_sub_chunk_loaded(air).unwrap();
    let available = store.collision_snapshot();
    assert!(!changed.is_sub_chunk_loaded(air));
    assert!(available.is_sub_chunk_loaded(air));
    store.mark_chunk_loaded(air.chunk()).unwrap();
    let complete = store.collision_snapshot();
    assert!(!available.is_sub_chunk_loaded(SubChunkKey { y: 1, ..air }));
    assert!(complete.is_sub_chunk_loaded(SubChunkKey { y: 1, ..air }));
    store.evict_chunk(key.chunk());
    let evicted = store.collision_snapshot();
    assert!(old.is_sub_chunk_loaded(key));
    assert!(!evicted.is_sub_chunk_loaded(key));
}

/// Palette changes still invalidate the cache before authority is granted.
#[test]
fn unauthoritative_palette_changes_do_not_reuse_old_indexes() {
    let mut store = ChunkStore::new();
    let key = SubChunkKey::new(0, 0, 0, 0);
    let empty = store.collision_snapshot();
    store
        .update_block(key, BlockUpdate::new(0, 0, 0, 0, 1), 0)
        .unwrap();
    let populated = store.collision_snapshot();
    assert!(!Arc::ptr_eq(&empty, &populated));
    assert!(empty.sub_chunk(key).is_none());
    assert_eq!(
        populated.sub_chunk(key).unwrap().runtime_id(0, 0, 0, 0),
        Some(1)
    );
}
