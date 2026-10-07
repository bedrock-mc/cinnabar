use super::*;
use client_world::ResolvedServerPosition;

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
    let position = [3.5, 70.0, -4.5];
    assert!(lifecycle.consume_nonspatial_phase(7, &control(1, 0, position), 42, &mut movement));
    assert!(!lifecycle.consume_nonspatial_phase(7, &control(2, 1, position), 42, &mut movement));
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
    for duplicate in [2, 3] {
        assert!(lifecycle.consume_nonspatial_phase(
            7,
            &control(duplicate, 1, position),
            42,
            &mut movement
        ));
    }
    lifecycle
        .queue_completion(|_| panic!("duplicate ready completion"))
        .unwrap();
    assert!(!lifecycle.input_held());
}

#[test]
fn respawn_search_and_session_replacement_drop_obsolete_completion() {
    let mut lifecycle = RespawnLifecycle::default();
    let mut movement = MovementTicker::default();
    let position = [3.5, 70.0, -4.5];
    lifecycle.consume_nonspatial_phase(7, &control(1, 1, position), 42, &mut movement);
    lifecycle.consume_nonspatial_phase(7, &control(2, 0, position), 42, &mut movement);
    lifecycle
        .queue_completion(|_| panic!("completion from preceding respawn"))
        .unwrap();
    assert!(lifecycle.input_held());
    lifecycle.consume_nonspatial_phase(7, &control(3, 1, position), 42, &mut movement);
    lifecycle.synchronize_session(Some(8));
    assert!(!lifecycle.input_held());
    lifecycle
        .queue_completion(|_| panic!("completion from replaced session"))
        .unwrap();
    assert!(!lifecycle.consume_nonspatial_phase(8, &control(1, 1, position), 56, &mut movement));
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
    let position = [3.5, 70.0, -4.5];
    lifecycle.consume_nonspatial_phase(7, &control(1, 0, position), 42, &mut movement);
    for (sequence, state) in [(2, 2), (3, u8::MAX)] {
        assert!(lifecycle.consume_nonspatial_phase(
            7,
            &control(sequence, state, [f32::NAN; 3]),
            42,
            &mut movement
        ));
    }
    assert!(lifecycle.input_held());
    assert_eq!(lifecycle.pending.unwrap().position, position);
    lifecycle
        .queue_completion(|_| panic!("unknown phase cannot complete respawn"))
        .unwrap();
}
