use super::*;

#[test]
fn bounding_box_needs_positive_finite_width_and_height_metadata() {
    let ActorEvent::Spawn(mut sized) = spawn(5, 50) else {
        unreachable!();
    };
    sized.metadata = Arc::from([
        ActorMetadata {
            key: 53,
            value: ActorMetadataValue::Float(0.6),
        },
        ActorMetadata {
            key: 54,
            value: ActorMetadataValue::Float(1.8),
        },
    ]);
    let ActorEvent::Spawn(mut flat) = spawn(6, 60) else {
        unreachable!();
    };
    flat.metadata = Arc::from([ActorMetadata {
        key: 54,
        value: ActorMetadataValue::Float(f32::NAN),
    }]);
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, ActorEvent::Spawn(sized));
    store.apply(1, 2, ActorEvent::Spawn(flat));

    assert_eq!(
        store.get(5).unwrap().bounding_box(),
        Some(([0.7, 2.0, 2.7], [1.3, 3.8, 3.3]))
    );
    assert_eq!(store.get(6).unwrap().bounding_box(), None);
    let mut ids = store
        .actors()
        .map(|actor| actor.runtime_id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    assert_eq!(ids, [5, 6]);
}

/// A player spawned without size metadata keeps its definition box, so it stays attackable.
#[test]
fn a_sizeless_player_uses_its_definition_collision_box() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, player_spawn(7, 70, 2.0));
    let (min, max) = store.get(7).unwrap().bounding_box().unwrap();
    let offset = store.get(7).unwrap().position;
    assert!((max[0] - min[0] - 0.6).abs() < 1e-4 && (max[1] - min[1] - 1.8).abs() < 1e-4);
    assert_eq!(min[1], offset[1]);
}
