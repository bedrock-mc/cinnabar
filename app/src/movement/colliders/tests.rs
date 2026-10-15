use super::*;
use sim::CollisionWorld;

#[test]
fn unchanged_source_reuses_conversion_without_allocating() {
    let cache = MovementColliderCache::default();
    let source: Arc<[ActorCollisionBounds]> = vec![([1.0, 2.0, 3.0], [4.0, 5.0, 6.0]); 32].into();
    let first = cache.snapshot(Arc::clone(&source));
    let (_, allocations) = test_alloc::measure(|| {
        for _ in 0..120 {
            assert!(Arc::ptr_eq(&first, &cache.snapshot(Arc::clone(&source))));
        }
    });
    assert_eq!(allocations.calls, 0);
    assert_eq!(
        first[0],
        sim::Aabb::new(sim::Vec3::new(1.0, 2.0, 3.0), sim::Vec3::new(4.0, 5.0, 6.0))
    );
}

#[test]
fn replacement_sources_drop_unused_conversions_and_keep_old_readers_immutable() {
    let cache = MovementColliderCache::default();
    let source: Arc<[ActorCollisionBounds]> = [([1.0, 2.0, 3.0], [4.0, 5.0, 6.0])].into();
    let first = cache.snapshot(Arc::clone(&source));
    let first_value = first.to_vec();
    let old = Arc::downgrade(&first);
    let old_source = Arc::downgrade(&source);
    drop(source);
    let replacement = cache.snapshot([([7.0, 8.0, 9.0], [10.0, 11.0, 12.0])].into());
    assert!(!Arc::ptr_eq(&first, &replacement));
    assert_eq!(&*first, first_value.as_slice());
    assert!(old_source.upgrade().is_none());
    drop(first);
    assert!(old.upgrade().is_none());
    let same_values_new_session = cache.snapshot([([7.0, 8.0, 9.0], [10.0, 11.0, 12.0])].into());
    assert!(!Arc::ptr_eq(&replacement, &same_values_new_session));
    assert_eq!(replacement, same_values_new_session);
    assert!(cache.snapshot(Arc::from([])).is_empty());
}

#[test]
fn replay_queries_keep_the_original_actor_bounds_and_query_errors() {
    let cache = MovementColliderCache::default();
    let mut terrain = world::ChunkStore::new();
    terrain
        .mark_chunk_loaded(world::ChunkKey::new(0, 0, 0))
        .unwrap();
    let mut registry = sim::CollisionRegistry::new();
    registry.register(0, []).unwrap();
    let before_bounds = cache.snapshot([([2.0, 2.0, 2.0], [3.0, 4.0, 3.0])].into());
    let before = sim::PaletteWorld::for_movement(&terrain, &registry, 0)
        .with_actor_colliders(Arc::clone(&before_bounds));
    let query = sim::Aabb::new(sim::Vec3::new(1.0, 1.0, 1.0), sim::Vec3::new(4.0, 5.0, 4.0));
    let expected = before.collision_boxes_with_provenance(query).unwrap();
    assert_eq!(expected.value.len(), 1);
    assert_eq!(expected.value[0].aabb, before_bounds[0]);
    assert_eq!(expected.value[0].block, None);
    let replay = before.snapshot().unwrap();
    let moved = sim::PaletteWorld::for_movement(&terrain, &registry, 0)
        .with_actor_colliders(cache.snapshot([([8.0, 2.0, 8.0], [9.0, 4.0, 9.0])].into()));
    assert!(moved.collision_boxes(query).unwrap().value.is_empty());
    assert_eq!(
        replay.collision_boxes_with_provenance(query).unwrap(),
        expected
    );
    cache.snapshot(Arc::from([]));
    assert_eq!(
        replay.collision_boxes_with_provenance(query).unwrap(),
        expected
    );
    let invalid = sim::Aabb::new(sim::Vec3::new(f64::NAN, 1.0, 1.0), query.max);
    let error = moved.collision_boxes(invalid);
    assert!(error.is_err());
    assert_eq!(replay.collision_boxes(invalid), error);
}
