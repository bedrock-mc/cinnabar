use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use super::ChunkStore;
use crate::{
    BlockUpdate, ChunkCollisionRevision, ChunkKey, CollisionRevisionError, DecodeError,
    DecodedLevelChunk, MutationError, SubChunkKey, collision_revision::CollisionRevisionAllocator,
};

#[test]
fn exact_revision_ceiling_is_issued_once_and_plus_one_is_rejected() {
    let allocator = CollisionRevisionAllocator::with_next(u64::MAX);
    assert_eq!(allocator.allocate().unwrap(), u64::MAX);
    assert_eq!(allocator.allocate(), Err(CollisionRevisionError::Exhausted));
}

#[test]
fn exhausted_store_rejects_load_before_mutation() {
    let allocator = Arc::new(CollisionRevisionAllocator::with_next(u64::MAX));
    let mut store = ChunkStore {
        chunks: HashMap::new(),
        loaded_chunks: BTreeSet::new(),
        collision_revisions: HashMap::new(),
        collision_revision_allocator: allocator,
    };
    let ceiling = ChunkKey::new(0, 0, 0);
    let rejected = ChunkKey::new(0, 1, 0);
    assert_eq!(store.mark_chunk_loaded(ceiling), Ok(true));
    assert_eq!(
        store.collision_revision(ceiling).unwrap().revision,
        u64::MAX
    );
    assert_eq!(
        store.mark_chunk_loaded(rejected),
        Err(CollisionRevisionError::Exhausted)
    );
    assert!(!store.is_chunk_loaded(rejected));
    assert_eq!(store.collision_revision(rejected), None);
}

#[test]
fn parallel_stores_share_unique_process_identity_space() {
    let revisions = (0..8)
        .map(|x| {
            std::thread::spawn(move || {
                let mut store = ChunkStore::new();
                let key = ChunkKey::new(0, x, 0);
                store.mark_chunk_loaded(key).unwrap();
                store.collision_revision(key).unwrap().revision
            })
        })
        .map(|thread| thread.join().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(revisions.len(), 8);
}

fn exhausted_loaded_store(key: ChunkKey) -> ChunkStore {
    ChunkStore {
        chunks: HashMap::new(),
        loaded_chunks: BTreeSet::from([key]),
        collision_revisions: HashMap::from([(
            key,
            ChunkCollisionRevision {
                chunk: key,
                revision: u64::MAX,
            },
        )]),
        collision_revision_allocator: Arc::new(CollisionRevisionAllocator::with_next(0)),
    }
}

#[test]
fn mutation_overflow_is_typed_and_transactional() {
    let chunk = ChunkKey::new(0, 0, 0);
    let key = SubChunkKey::from_chunk(chunk, 0);
    let mut store = exhausted_loaded_store(chunk);
    assert_eq!(
        store.update_block(key, BlockUpdate::new(0, 0, 0, 0, 1), 0),
        Err(MutationError::CollisionRevision(
            CollisionRevisionError::Exhausted
        ))
    );
    assert!(store.chunk(chunk).is_none());
    assert_eq!(store.collision_revision(chunk).unwrap().revision, u64::MAX);
}

#[test]
fn full_column_overflow_is_typed_and_transactional() {
    let chunk = ChunkKey::new(1, 0, 0);
    let mut store = exhausted_loaded_store(chunk);
    let decoded = DecodedLevelChunk::decode(0, 1, &[9, 1, 0, 1, 2]).unwrap();
    assert_eq!(
        store.commit_level_chunk(chunk, decoded),
        Err(DecodeError::CollisionRevision(
            CollisionRevisionError::Exhausted
        ))
    );
    assert!(store.chunk(chunk).is_none());
    assert_eq!(store.collision_revision(chunk).unwrap().revision, u64::MAX);
}

#[test]
fn sub_chunk_overflow_does_not_leave_an_empty_sparse_column() {
    let chunk = ChunkKey::new(2, 0, 0);
    let key = SubChunkKey::from_chunk(chunk, 0);
    let mut store = exhausted_loaded_store(chunk);
    let decoded = crate::SubChunk::decode(&[9, 1, 0, 1, 2]).unwrap();
    assert_eq!(
        store.commit_sub_chunk(key, decoded),
        Err(DecodeError::CollisionRevision(
            CollisionRevisionError::Exhausted
        ))
    );
    assert!(store.chunk(chunk).is_none());
}
