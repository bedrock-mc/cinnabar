use super::*;
use client_world::ResolvedServerPosition;
use protocol::RespawnEvent;

fn control(sequence: u64, state: u8, position: [f32; 3]) -> CommittedControlEvent {
    CommittedControlEvent::Respawn {
        sequence,
        respawn: RespawnEvent {
            position,
            state,
            runtime_entity_id: 0,
        },
        resolved: ResolvedServerPosition {
            position,
            surface_anchor: None,
        },
    }
}

#[test]
fn respawn_ready_retries_without_duplicate_packets_or_the_server_sentinel_actor() {
    let mut lifecycle = RespawnLifecycle::default();
    let mut movement = MovementTicker::default();
    let mut physics = LocalPhysicsController::default();
    let position = [3.5, 70.0, -4.5];
    assert!(lifecycle.consume_nonspatial_phase(
        7,
        &control(1, 0, position),
        42,
        &mut movement,
        &mut physics,
        None
    ));
    assert!(!lifecycle.consume_nonspatial_phase(
        7,
        &control(2, 1, position),
        42,
        &mut movement,
        &mut physics,
        None
    ));
    assert!(lifecycle.input_held());
    assert!(matches!(
        lifecycle.queue_completion(|packet| Err(PacketSendError::Full(packet))),
        Err(PacketSendError::Full(_))
    ));
    assert!(lifecycle.input_held());
    let mut packets = Vec::new();
    lifecycle
        .queue_completion(|packet| {
            packets.push(packet);
            Ok(())
        })
        .unwrap();
    assert_eq!(packets, [protocol::respawn_ready_packet(42)]);
    assert!(!lifecycle.input_held());
    assert!(lifecycle.consume_nonspatial_phase(
        7,
        &control(2, 1, position),
        42,
        &mut movement,
        &mut physics,
        None
    ));
    lifecycle
        .queue_completion(|_| panic!("duplicate ready completion"))
        .unwrap();
    assert!(!lifecycle.input_held());
    assert!(!lifecycle.consume_nonspatial_phase(
        7,
        &control(3, 1, position),
        42,
        &mut movement,
        &mut physics,
        None
    ));
    lifecycle
        .queue_completion(|packet| {
            assert_eq!(packet, protocol::respawn_ready_packet(42));
            Ok(())
        })
        .unwrap();
    assert!(!lifecycle.input_held());
}

#[test]
fn respawn_search_and_session_replacement_drop_obsolete_completion() {
    let mut lifecycle = RespawnLifecycle::default();
    let mut movement = MovementTicker::default();
    let mut physics = LocalPhysicsController::default();
    let position = [3.5, 70.0, -4.5];
    lifecycle.consume_nonspatial_phase(
        7,
        &control(1, 1, position),
        42,
        &mut movement,
        &mut physics,
        None,
    );
    lifecycle.consume_nonspatial_phase(
        7,
        &control(2, 0, position),
        42,
        &mut movement,
        &mut physics,
        None,
    );
    lifecycle
        .queue_completion(|_| panic!("completion from preceding respawn"))
        .unwrap();
    assert!(lifecycle.input_held());
    lifecycle.consume_nonspatial_phase(
        7,
        &control(3, 1, position),
        42,
        &mut movement,
        &mut physics,
        None,
    );
    lifecycle.synchronize_session(Some(8));
    assert!(!lifecycle.input_held());
    lifecycle
        .queue_completion(|_| panic!("completion from replaced session"))
        .unwrap();
    assert!(!lifecycle.consume_nonspatial_phase(
        8,
        &control(1, 1, position),
        56,
        &mut movement,
        &mut physics,
        None
    ));
    lifecycle
        .queue_completion(|packet| {
            assert_eq!(packet, protocol::respawn_ready_packet(56));
            Ok(())
        })
        .unwrap();
    lifecycle.synchronize_session(None);
    assert!(!lifecycle.input_held());
}

#[test]
fn respawn_unknown_phases_are_nonspatial_and_preserve_the_current_search() {
    let mut lifecycle = RespawnLifecycle::default();
    let mut movement = MovementTicker::default();
    let mut physics = LocalPhysicsController::default();
    let position = [3.5, 70.0, -4.5];
    lifecycle.consume_nonspatial_phase(
        7,
        &control(1, 0, position),
        42,
        &mut movement,
        &mut physics,
        None,
    );
    for (sequence, state) in [(2, 2), (3, u8::MAX)] {
        assert!(lifecycle.consume_nonspatial_phase(
            7,
            &control(sequence, state, [f32::NAN; 3]),
            42,
            &mut movement,
            &mut physics,
            None
        ));
    }
    assert!(lifecycle.input_held());
    lifecycle
        .queue_completion(|_| panic!("unknown phase cannot complete respawn"))
        .unwrap();
}

#[test]
fn local_death_holds_input_behind_older_controls_and_rearms_after_ready() {
    let mut lifecycle = RespawnLifecycle::default();
    lifecycle.synchronize_session(Some(7));
    let mut movement = MovementTicker::default();
    let mut physics = LocalPhysicsController::default();
    let death = CommittedControlEvent::LocalDeath { sequence: 2 };
    lifecycle.retain_committed_input_hold(&[death]);
    assert!(
        lifecycle.input_held(),
        "death must hold input even while its control waits"
    );
    for event in [death, control(3, 0, [0.0; 3])] {
        assert!(lifecycle.consume_nonspatial_phase(
            7,
            &event,
            42,
            &mut movement,
            &mut physics,
            None
        ));
        assert!(lifecycle.input_held());
    }
    assert!(!lifecycle.consume_nonspatial_phase(
        7,
        &control(4, 1, [0.0; 3]),
        42,
        &mut movement,
        &mut physics,
        None
    ));
    lifecycle.queue_completion(|_| Ok(())).unwrap();
    assert!(!lifecycle.input_held());
    assert!(lifecycle.consume_nonspatial_phase(7, &death, 42, &mut movement, &mut physics, None));
    assert!(
        !lifecycle.input_held(),
        "replaying an old death cannot rearm the hold"
    );
    assert!(lifecycle.consume_nonspatial_phase(
        7,
        &CommittedControlEvent::LocalDeath { sequence: 5 },
        42,
        &mut movement,
        &mut physics,
        None
    ));
    assert!(lifecycle.input_held(), "a new death must start a new hold");
    lifecycle.synchronize_session(Some(8));
    assert!(!lifecycle.input_held());
}
