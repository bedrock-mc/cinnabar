use super::*;
use protocol::{ActorEvent, ActorMetadata, ActorMetadataValue, ActorSpawnEvent, NetworkItemStack};
use std::sync::Arc;

#[test]
fn copy_tiers_grow_with_the_native_stack_thresholds() {
    for (count, copies) in [(1, 1), (2, 2), (5, 2), (6, 3), (20, 3), (21, 4), (64, 4)] {
        assert_eq!(dropped_item_copy_count(count), copies);
    }
    assert_eq!(
        dropped_item_copy_count(u16::MAX),
        MAX_DROPPED_ITEM_COPIES as u8
    );
}

#[test]
fn copy_offsets_are_renderer_wide_isotropic_and_first_copy_is_centred() {
    let a = copy_offsets(MAX_DROPPED_ITEM_COPIES as u8);
    assert_eq!(a, copy_offsets(MAX_DROPPED_ITEM_COPIES as u8));
    assert_eq!(a[0], [0.0; 3]);
    for offset in &a[1..] {
        assert!(offset.iter().all(|value| value.abs() <= COPY_SPREAD));
    }
    assert_eq!(copy_offsets(1)[1], [0.0; 3]);
}

#[test]
fn phase_is_bounded_stable_and_owned_by_actor_lifetime() {
    for id in 0..64 {
        let value = phase((id, 1));
        assert!((0.0..std::f32::consts::TAU).contains(&value));
        assert_eq!(value, phase((id, 1)));
        assert_ne!(value, phase((id, 2)));
    }
}

fn dropped_spawn(id: u64, count: u16) -> ActorEvent {
    let apple = protocol::vanilla_item_registry()
        .iter()
        .find(|entry| entry.identifier.as_ref() == "minecraft:apple")
        .unwrap()
        .network_id;
    ActorEvent::Spawn(ActorSpawnEvent {
        dimension: 0,
        unique_id: id as i64,
        runtime_id: id,
        kind: ActorKind::Entity {
            identifier: "minecraft:item".into(),
        },
        position: [1.0, 2.0, 3.0],
        velocity: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item: NetworkItemStack {
            network_id: apple,
            count,
            ..Default::default()
        },
        metadata: Arc::from([ActorMetadata {
            key: 38,
            value: ActorMetadataValue::Float(2.0),
        }]),
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    })
}

#[test]
fn drop_view_preserves_actor_origin_scale_and_half_tick_spin_without_random_yaw() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, dropped_spawn(7, 64));
    store.apply(1, 2, dropped_spawn(8, 64));
    let views = store.dropped_items(0.0);
    assert_eq!(views.len(), 2);
    for view in &views {
        assert_eq!(view.position, [1.0, 2.0 + ITEM_ACTOR_NETWORK_OFFSET, 3.0]);
        assert_eq!(view.render_scale, 2.0);
        assert_eq!(view.yaw_radians, 0.0);
        assert_eq!(view.copy_count, MAX_DROPPED_ITEM_COPIES as u8);
    }
    assert_eq!(views[0].copy_offsets, views[1].copy_offsets);
    store.actors.get_mut(&7).unwrap().status.age_ticks = 10;
    let later = store.dropped_items(0.5);
    assert_eq!(
        later[0].yaw_radians,
        10.5 * SPIN_RATE_PER_TICK - SPIN_HALF_TICK
    );
    assert_eq!(later[0].bob_phase, views[0].bob_phase);
}

#[test]
fn malformed_pose_and_empty_stack_are_not_drawable() {
    let mut store = ActorStore::new(1, 0);
    store.apply(1, 1, dropped_spawn(7, 0));
    assert!(store.dropped_items(0.0).is_empty());
    store.apply(1, 2, dropped_spawn(7, 1));
    store.actors.get_mut(&7).unwrap().position[0] = f32::NAN;
    assert!(store.dropped_items(0.0).is_empty());
}

#[test]
fn spawn_absolute_and_partial_move_keep_feet_and_restore_the_same_native_origin() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut spawn) = dropped_spawn(7, 1) else {
        unreachable!()
    };
    spawn.metadata = Arc::from([
        ActorMetadata {
            key: super::super::BOUNDING_BOX_WIDTH_METADATA_KEY,
            value: ActorMetadataValue::Float(0.25),
        },
        ActorMetadata {
            key: super::super::BOUNDING_BOX_HEIGHT_METADATA_KEY,
            value: ActorMetadataValue::Float(0.25),
        },
    ]);
    store.apply(1, 1, ActorEvent::Spawn(spawn));
    let feet = [1.0, 2.0, 3.0];
    let native_origin = [feet[0], feet[1] + ITEM_ACTOR_NETWORK_OFFSET, feet[2]];
    assert_eq!(store.dropped_items(0.0)[0].position, native_origin);
    for (sequence, partial) in [(2, false), (3, true)] {
        store.apply(
            1,
            sequence,
            ActorEvent::Move(protocol::ActorMoveEvent {
                dimension: 0,
                runtime_id: 7,
                position: if partial {
                    [None, Some(native_origin[1]), None]
                } else {
                    native_origin.map(Some)
                },
                position_origin: protocol::ActorPositionOrigin::NetworkOffset,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported: true,
                player_mode: None,
                source_tick: None,
                interpolation: Default::default(),
            }),
        );
        let actor = store.get(7).unwrap();
        assert_eq!(actor.position, feet);
        assert_eq!(actor.bounding_box().unwrap().0[1], feet[1]);
        assert_eq!(
            actor.brightness_sample_position(feet)[1],
            feet[1] + 0.66 * 0.25
        );
        assert_eq!(store.dropped_items(0.0)[0].position, native_origin);
    }
}
