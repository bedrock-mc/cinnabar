use super::*;

/// Sends an exact network-origin height and independent rotation targets.
fn network_move(store: &mut ActorStore, sequence: u64, y: Option<f32>, snap: bool) {
    store.apply(
        1,
        sequence,
        ActorEvent::Move(ActorMoveEvent {
            dimension: 0,
            runtime_id: 1,
            position: [None, y, None],
            position_origin: ActorPositionOrigin::NetworkOffset,
            pitch: Some(10.0),
            yaw: Some(10.0),
            head_yaw: Some(10.0),
            on_ground: Some(false),
            teleported: snap,
            force_local_move: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
}

#[test]
fn body_and_head_rotation_multiply_by_the_rounded_reciprocal() {
    let mut store = player(0.0);
    network_move(&mut store, 2, None, false);
    store.advance_interpolation_ticks(1);
    let actor = store.get(1).unwrap();
    for value in [actor.pitch, actor.yaw] {
        assert_eq!(value.to_bits(), wrap_degrees(10.0 * (1.0 / 3.0)).to_bits());
    }
    assert_eq!(actor.head_yaw.to_bits(), 0x4055_5556);
}

#[test]
fn vertical_steps_round_at_the_network_origin_before_converting_to_feet() {
    let mut store = player(0.0);
    let offset = store.get(1).unwrap().network_position_offset();
    let target = offset + 1.0;
    network_move(&mut store, 2, Some(target), false);
    let mut expected = offset;
    for remaining in (1..=3).rev() {
        expected = if remaining == 1 {
            target
        } else {
            expected + (target - expected) * (1.0 / remaining as f32)
        };
        store.advance_interpolation_ticks(1);
        let actor = store.get(1).unwrap();
        assert_eq!(actor.native_position()[1].to_bits(), expected.to_bits());
        assert_eq!(actor.position[1].to_bits(), (expected - offset).to_bits());
    }
}

#[test]
fn teleports_and_partial_updates_keep_the_exact_network_height() {
    let mut store = player(0.0);
    let height = 0.000_001_f32;
    network_move(&mut store, 2, Some(height), true);
    assert_eq!(
        store.get(1).unwrap().native_position()[1].to_bits(),
        height.to_bits()
    );
    network_move(&mut store, 3, None, false);
    for _ in 0..4 {
        store.advance_interpolation_ticks(1);
        assert_eq!(
            store.get(1).unwrap().native_position()[1].to_bits(),
            height.to_bits()
        );
    }
}

#[test]
fn cached_height_offset_changes_keep_render_and_partial_moves_on_the_same_feet() {
    use protocol::{ActorMetadata, ActorMetadataUpdateEvent, ActorMetadataValue};
    use protocol_ids::{ActorDataKey, PlayerFlag};

    for sleeping_player in [false, true] {
        let mut store = if sleeping_player {
            player(0.0)
        } else {
            let mut store = ActorStore::new(1, 0);
            store.apply(
                1,
                1,
                crate::actor_store::tests::entity_spawn(
                    1,
                    1,
                    "minecraft:tnt",
                    [ActorMetadata {
                        key: ActorDataKey::Height.wire_value(),
                        value: ActorMetadataValue::Float(2.0),
                    }]
                    .into(),
                ),
            );
            store
        };
        let height = 64.0 + store.get(1).unwrap().network_position_offset();
        network_move(&mut store, 2, Some(height), true);
        let feet = store.get(1).unwrap().position[1];
        store.apply(
            1,
            3,
            ActorEvent::Metadata(ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: [if sleeping_player {
                    ActorMetadata {
                        key: ActorDataKey::PlayerFlags.wire_value(),
                        value: ActorMetadataValue::Byte(PlayerFlag::Sleeping.wire_value() as i8),
                    }
                } else {
                    ActorMetadata {
                        key: ActorDataKey::Height.wire_value(),
                        value: ActorMetadataValue::Float(4.0),
                    }
                }]
                .into(),
                properties: [].into(),
                tick: 3,
            }),
        );
        for alpha in [0.0, 0.5, 1.0] {
            assert_eq!(
                store.get(1).unwrap().interpolated_position(alpha).unwrap()[1],
                feet
            );
        }
        network_move(&mut store, 4, None, false);
        for _ in 0..4 {
            store.predict_remote_motion(1);
            assert_eq!(store.pick_state(1).unwrap().pose.position[1], feet);
            store.advance_interpolation_frame(1);
            assert_eq!(store.get(1).unwrap().position[1], feet);
            assert_eq!(
                store.get(1).unwrap().interpolated_position(1.0).unwrap()[1],
                feet
            );
        }
    }
}
