use crate::{
    BlockEntityError, BlockEntityKey, BlockEntityNbt, ChunkStore, CollisionRevisionError,
    DecodedBlockEntities, ShapeActor,
};
use crate::{NbtCompound, NbtValue};
use std::sync::Arc;

/// Encodes a geometry-bearing actor at the origin through the public NBT decoder.
fn actor(id: &str, progress: f32) -> BlockEntityNbt {
    let mut root = NbtCompound::default();
    root.insert("id", NbtValue::String(id.into()));
    for key in ["x", "y", "z"] {
        root.insert(key, NbtValue::Int(0));
    }
    root.insert("Progress", NbtValue::Float(progress));
    BlockEntityNbt::decode_prefix(&root.encode_root().unwrap())
        .unwrap()
        .0
}

#[test]
fn actor_geometry_changes_retire_snapshots_and_preserve_historical_facts() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    store
        .commit_block_entity_update(key, actor("PistonArm", 0.0))
        .unwrap();
    let old = store.collision_snapshot();
    let revision = old.collision_revision(key.chunk());
    store
        .commit_block_entity_update(key, actor("PistonArm", 0.5))
        .unwrap();
    let current = store.collision_snapshot();
    assert!(!Arc::ptr_eq(&old, &current));
    assert_ne!(revision, current.collision_revision(key.chunk()));
    assert_eq!(
        old.block_entity(key).unwrap().shape_actor(),
        &ShapeActor::Piston {
            progress_bits: 0.0_f32.to_bits()
        }
    );
    assert_eq!(
        current.block_entity(key).unwrap().shape_actor(),
        &ShapeActor::Piston {
            progress_bits: 0.5_f32.to_bits()
        }
    );
    let revision = current.collision_revision(key.chunk());
    store
        .commit_block_entity_update(key, actor("PistonArm", 0.5))
        .unwrap();
    assert_eq!(revision, store.collision_revision(key.chunk()));
    assert!(Arc::ptr_eq(&current, &store.collision_snapshot()));
}

#[test]
fn whole_column_actor_removal_invalidates_geometry_without_palette_changes() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    store
        .commit_block_entity_update(key, actor("PistonArm", 0.5))
        .unwrap();
    let revision = store.collision_revision(key.chunk());
    let empty = DecodedBlockEntities::decode_level_chunk_tail(key.chunk(), 0..16, &[0]);
    store
        .commit_chunk_block_entities(key.chunk(), empty)
        .unwrap();
    assert!(store.block_entity(key).is_none());
    assert_ne!(revision, store.collision_revision(key.chunk()));
}

#[test]
fn all_air_removes_actor_geometry_even_without_a_stored_palette() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    store
        .commit_block_entity_update(key, actor("PistonArm", 0.5))
        .unwrap();
    let revision = store.collision_revision(key.chunk());
    assert_eq!(store.apply_all_air(key.sub_chunk()).unwrap(), None);
    assert!(store.block_entity(key).is_none());
    assert_ne!(revision, store.collision_revision(key.chunk()));
}

#[test]
fn unrelated_actor_nbt_does_not_churn_collision_history() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    let before = store.collision_snapshot();
    let revision = store.collision_revision(key.chunk());
    store
        .commit_block_entity_update(key, actor("Chest", 0.0))
        .unwrap();
    assert_eq!(revision, store.collision_revision(key.chunk()));
    assert!(Arc::ptr_eq(&before, &store.collision_snapshot()));
}

#[test]
fn exhausted_revision_space_rejects_actor_geometry_before_mutation() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store.mark_sub_chunk_loaded(key.sub_chunk()).unwrap();
    store.collision_revision_allocator =
        Arc::new(crate::collision_revision::CollisionRevisionAllocator::with_next(0));
    assert!(matches!(
        store.commit_block_entity_update(key, actor("PistonArm", 0.5)),
        Err(BlockEntityError::CollisionRevision(
            CollisionRevisionError::Exhausted
        ))
    ));
    assert!(store.block_entity(key).is_none());
}

/// Decodes an unchanged inline air palette carrying the supplied actor snapshot.
fn inline_actor(actor: &BlockEntityNbt) -> crate::DecodedLevelChunk {
    let mut payload = vec![9, 1, 0, 1, 0, 1, 0, 0];
    payload.extend_from_slice(actor.bytes());
    crate::DecodedLevelChunk::decode_inline(
        crate::ChunkKey::new(0, 0, 0),
        crate::DimensionSlots {
            base_sub_chunk_y: 0,
            count: 1,
        },
        1,
        &payload,
        &crate::RawBlockIds { air: 0 },
        &crate::RawBiomeIds { default_biome: 0 },
    )
}

#[test]
fn inline_actor_replacement_retires_snapshot_and_pick_revision() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store
        .commit_level_chunk(key.chunk(), inline_actor(&actor("PistonArm", 0.0)))
        .unwrap();
    let old = store.collision_snapshot();
    let revision = old.collision_revision(key.chunk());
    store
        .commit_level_chunk(key.chunk(), inline_actor(&actor("PistonArm", 0.5)))
        .unwrap();
    let current = store.collision_snapshot();
    assert_ne!(revision, store.collision_revision(key.chunk()));
    assert!(!Arc::ptr_eq(&old, &current));
    assert_eq!(
        old.block_entity(key).unwrap().shape_actor(),
        &ShapeActor::Piston {
            progress_bits: 0.0_f32.to_bits()
        }
    );
    assert_eq!(
        current.block_entity(key).unwrap().shape_actor(),
        &ShapeActor::Piston {
            progress_bits: 0.5_f32.to_bits()
        }
    );
    let revision = store.collision_revision(key.chunk());
    store
        .commit_level_chunk(key.chunk(), inline_actor(&actor("PistonArm", 0.5)))
        .unwrap();
    assert_eq!(revision, store.collision_revision(key.chunk()));
    assert!(Arc::ptr_eq(&current, &store.collision_snapshot()));
}

#[test]
fn inline_actor_removal_retires_snapshot_without_palette_changes() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store
        .commit_level_chunk(key.chunk(), inline_actor(&actor("PistonArm", 0.5)))
        .unwrap();
    let old = store.collision_snapshot();
    let revision = store.collision_revision(key.chunk());
    store
        .commit_level_chunk(key.chunk(), inline_actor(&actor("Chest", 0.5)))
        .unwrap();
    assert_ne!(revision, store.collision_revision(key.chunk()));
    let current = store.collision_snapshot();
    assert_eq!(
        current.block_entity(key).unwrap().shape_actor(),
        &ShapeActor::None
    );
    assert!(matches!(
        old.block_entity(key).unwrap().shape_actor(),
        ShapeActor::Piston { .. }
    ));
}

#[test]
fn inline_actor_revision_exhaustion_preserves_the_previous_column() {
    let mut store = ChunkStore::new();
    let key = BlockEntityKey::new(0, 0, 0, 0);
    store
        .commit_level_chunk(key.chunk(), inline_actor(&actor("PistonArm", 0.0)))
        .unwrap();
    let old = store.collision_snapshot();
    store.collision_revision_allocator =
        Arc::new(crate::collision_revision::CollisionRevisionAllocator::with_next(0));
    let result = store.commit_level_chunk(key.chunk(), inline_actor(&actor("PistonArm", 0.5)));
    assert!(
        matches!(
            result,
            Err(crate::DecodeError::CollisionRevision(
                CollisionRevisionError::Exhausted
            ))
        ),
        "{result:?}"
    );
    assert_eq!(
        store.block_entity(key).unwrap().shape_actor(),
        old.block_entity(key).unwrap().shape_actor()
    );
    assert!(Arc::ptr_eq(&old, &store.collision_snapshot()));
}
