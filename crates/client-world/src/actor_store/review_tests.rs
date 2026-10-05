use super::*;
use std::sync::Arc;

#[test]
fn falling_block_uses_its_variant_metadata_as_the_retained_block_identity() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut event) = tests::spawn(42, -7) else {
        unreachable!()
    };
    event.kind = ActorKind::Entity {
        identifier: "minecraft:falling_block".into(),
    };
    let hash = i32::from_ne_bytes(0x8000_0007_u32.to_ne_bytes());
    event.metadata = Arc::from([
        protocol::ActorMetadata {
            key: 2,
            value: ActorMetadataValue::Int(hash),
        },
        protocol::ActorMetadata {
            key: 16,
            value: ActorMetadataValue::Int(99),
        },
    ]);
    store.apply(1, 1, ActorEvent::Spawn(event));
    store.apply_terrain_sync(protocol::ActorBlockSyncMessage {
        actor_unique_id: -7,
        message: 1,
    });
    let views = store.block_entities(0.0);
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].center, [1.0, 2.0, 3.0]);
    assert_eq!(
        views[0].kind,
        super::entities::BlockEntityKind::Falling {
            block_runtime_id: hash
        }
    );
    let ActorEvent::Move(mut movement) = tests::player_move(42, 1.0, false) else {
        unreachable!()
    };
    movement.position = [Some(1.0), Some(2.0), Some(3.0)];
    movement.position_origin = protocol::ActorPositionOrigin::NetworkOffset;
    store.apply(1, 2, ActorEvent::Move(movement));
    for _ in 0..ACTOR_INTERPOLATION_TICKS {
        store.advance_interpolation_ticks(1);
        assert_eq!(store.block_entities(1.0)[0].center, [1.0, 2.0, 3.0]);
    }
}

#[test]
fn review_lead_holder_resolves_negative_unique_ids() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, tests::spawn(42, -7));
    let ActorEvent::Spawn(mut mob) = tests::spawn(43, -8) else {
        unreachable!()
    };
    mob.metadata = Arc::from([protocol::ActorMetadata {
        key: 37,
        value: protocol::ActorMetadataValue::Long(-7),
    }]);
    store.apply(1, 2, ActorEvent::Spawn(mob));
    assert_eq!(store.ropes(0.0).len(), 1);
}

#[test]
fn review_nonfinite_movement_retains_valid_components_of_the_previous_pose() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, tests::spawn(42, -7));
    let ActorEvent::Move(mut movement) = tests::player_move(42, f32::NAN, true) else {
        unreachable!()
    };
    movement.position[2] = Some(9.0);
    movement.pitch = Some(f32::INFINITY);
    movement.yaw = Some(45.0);
    movement.head_yaw = Some(f32::NEG_INFINITY);
    store.apply(1, 2, ActorEvent::Move(movement));
    assert_eq!(store.ignored_movement_components, 3);
    let pose = store.get(42).unwrap().received_pose;
    assert_eq!(pose.position, [1.0, 2.0, 9.0]);
    assert_eq!((pose.pitch, pose.yaw, pose.head_yaw), (0.0, 45.0, 0.0));
}
