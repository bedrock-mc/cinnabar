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
