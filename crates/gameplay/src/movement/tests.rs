use super::*;

#[test]
fn flush_refuses_a_stale_queue_without_physics_authority() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(PhysicsMovementSample {
            tick: 11,
            position: [1.0, 2.0, 3.0],
            movement: [0.1, 0.2, 0.3],
            velocity: [0.1, 0.2, 0.3],
            move_vector: [0.0; 2],
            raw_move_vector: [0.0; 2],
            analogue_move_vector: [0.0; 2],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            camera_orientation: [0.0, 0.0, 1.0],
            jumping: false,
            sneaking: false,
            input: Default::default(),
            sprinting: false,
            input_mode: PlayerInputMode::Mouse,
            grounded_before_tick: false,
            grounded_after_tick: false,
            horizontal_collision: false,
            vertical_collision: false,
            jump_repeated: false,
            processed: ProcessedMovementState::default(),
            world_identity: WorldCollisionIdentity::new(
                sim::CollisionRegistryIdentity {
                    protocol: 1001,
                    id_space: sim::CollisionIdSpace::Sequential,
                    preg_sha256: [1; 32],
                },
                [],
            )
            .unwrap(),
        })
        .unwrap();
    assert_eq!(ticker.outbox.len(), 1);

    // Simulate stale state surviving a future refactor so the flush guard
    // is verified independently from set_source's transition cleanup.
    ticker.source = MovementSource::FreeCamera;
    let mut sent_packets = 0;
    let flushed = flush_player_auth_inputs(&mut ticker, 8, None, |_identity, _packet| {
        sent_packets += 1;
        Ok::<_, ()>(())
    })
    .unwrap();

    assert_eq!(flushed, 0);
    assert_eq!(sent_packets, 0);
    assert_eq!(ticker.sent_free_camera_packet_count(), 0);
    assert_eq!(ticker.outbox.len(), 1);
}

#[test]
fn action_rotation_updates_only_the_matching_unsent_actor_facing() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(PhysicsMovementSample {
            tick: 11,
            position: [1.0, 2.0, 3.0],
            movement: [0.1, 0.2, 0.3],
            velocity: [0.1, 0.2, 0.3],
            move_vector: [0.0; 2],
            raw_move_vector: [0.0; 2],
            analogue_move_vector: [0.0; 2],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            camera_orientation: [0.0, 0.0, 1.0],
            jumping: false,
            sneaking: false,
            input: Default::default(),
            sprinting: false,
            input_mode: PlayerInputMode::Mouse,
            grounded_before_tick: false,
            grounded_after_tick: false,
            horizontal_collision: false,
            vertical_collision: false,
            jump_repeated: false,
            processed: ProcessedMovementState::default(),
            world_identity: WorldCollisionIdentity::new(
                sim::CollisionRegistryIdentity {
                    protocol: 1001,
                    id_space: sim::CollisionIdSpace::Sequential,
                    preg_sha256: [1; 32],
                },
                [],
            )
            .unwrap(),
        })
        .unwrap();
    let previous = ticker.outbox[0].snapshot;
    assert!(!ticker.override_action_rotation(previous.tick + 1, 15.0, 30.0));
    assert!(!ticker.override_action_rotation(previous.tick, f32::NAN, 30.0));
    assert_eq!(ticker.outbox[0].snapshot, previous);
    assert!(ticker.override_action_rotation(previous.tick, 15.0, 30.0));
    let changed = ticker.outbox[0].snapshot;
    assert_eq!(
        (changed.pitch, changed.yaw, changed.head_yaw),
        (15.0, 30.0, previous.head_yaw),
        "aim assist leaves head rotation untouched"
    );
    assert_eq!(changed.camera_orientation, previous.camera_orientation);
    assert_eq!(changed.move_vector, previous.move_vector);
    assert_eq!(changed.position, previous.position);
    assert_eq!(changed.delta, previous.delta);
}

/// Ticker with ticks 101 and 102 queued and a release held behind 101.
fn held_release_ticker() -> MovementTicker {
    use crate::test_support::survival_mining::completed;
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, completed(101).position);
    ticker.set_source(MovementSource::Physics);
    ticker.enqueue_completed_physics(completed(101)).unwrap();
    assert!(ticker.override_action_rotation(101, 15.0, 30.0));
    ticker.hold_release_after_tick(101, vec![protocol::stop_sleeping_packet(42)]);
    ticker.enqueue_completed_physics(completed(102)).unwrap();
    ticker
}

/// Flushes up to `budget` inputs, returning the identities handed to transport.
fn flush_identities(ticker: &mut MovementTicker, budget: usize) -> Vec<PhysicsSendIdentity> {
    let mut sent = Vec::new();
    flush_player_auth_inputs(
        ticker,
        budget,
        Some(crate::test_support::survival_mining::evidence()),
        |identity, _packet| {
            sent.push(identity);
            Ok::<_, ()>(())
        },
    )
    .unwrap();
    sent
}

#[test]
fn held_release_fences_newer_facing_until_the_release_is_admitted() {
    let mut ticker = held_release_ticker();
    let sent = flush_identities(&mut ticker, 8);
    assert_eq!(sent.iter().map(|id| id.tick).collect::<Vec<_>>(), [101]);
    ticker.send_held_release(|_| panic!("the facing tick is not written yet"));
    assert!(ticker.acknowledge_physics_send(sent[0]));
    ticker.send_held_release(|_| Err(crate::BatchSendError::Full));
    assert!(ticker.has_held_release());
    assert!(
        flush_identities(&mut ticker, 8).is_empty(),
        "tick 102 must not overtake a backpressured release"
    );
    let mut released = 0;
    ticker.send_held_release(|packets| {
        released = packets.len();
        Ok(())
    });
    assert_eq!(released, 1);
    assert!(!ticker.has_held_release());
    let sent = flush_identities(&mut ticker, 8);
    assert_eq!(sent.iter().map(|id| id.tick).collect::<Vec<_>>(), [102]);
}

#[test]
fn authority_change_drops_a_held_release_whose_facing_never_went_out() {
    let mut ticker = held_release_ticker();
    ticker.reanchor_surface_spawn(102, [0.0, 70.0, 0.0]);
    assert!(!ticker.has_held_release());
    ticker.send_held_release(|_| panic!("the facing input was discarded"));

    let mut ticker = held_release_ticker();
    let sent = flush_identities(&mut ticker, 8);
    ticker.begin_respawn_search();
    // A write acknowledged under the old epoch must not revive the release.
    ticker.acknowledge_physics_send(sent[0]);
    assert!(!ticker.has_held_release());
}

#[test]
fn held_release_survives_an_authority_change_after_its_facing_was_written() {
    let mut ticker = held_release_ticker();
    let sent = flush_identities(&mut ticker, 1);
    assert!(ticker.acknowledge_physics_send(sent[0]));
    ticker.reanchor_surface_spawn(102, [0.0, 70.0, 0.0]);
    let mut released = false;
    ticker.send_held_release(|_| {
        released = true;
        Ok(())
    });
    assert!(released);
}

/// A reanchor that keeps the tick number retires an action flag latched under the old authority.
#[test]
fn a_reanchor_drops_a_flag_latched_for_the_next_tick() {
    use crate::test_support::survival_mining::{completed, ticker_with_ticks};
    for reanchor in [false, true] {
        let mut ticker = ticker_with_ticks(0);
        let next = ticker.completed_tick() + 1;
        assert!(ticker.mark_missed_swing(next));
        if reanchor {
            ticker.snap_non_authoritative_anchor(next - 1, [0.5, 2.620_01, 0.5]);
            assert_eq!(
                ticker.completed_tick() + 1,
                next,
                "the tick number survives"
            );
        }
        ticker.enqueue_completed_physics(completed(next)).unwrap();
        let flagged =
            ticker.pending_snapshots()[0].flags.bits() & PlayerInputFlags::MISSED_SWING.bits() != 0;
        assert_eq!(flagged, !reanchor);
    }
}
