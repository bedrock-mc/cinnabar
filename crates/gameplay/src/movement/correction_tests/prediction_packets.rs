//! Prediction packet placement, replay distance and deferred motion ordering.

use super::*;

#[test]
fn a_large_prediction_correction_replays_retained_ticks_without_teleporting() {
    let world = VersionedFloor(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let anchor = &frame.samples[0];
    let mut position = anchor.position;
    position[0] += 32.0;
    let mut ticker = ticker_with_samples(frame.samples.iter().cloned());
    let outcome = crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        position,
        anchor.tick,
        anchor.grounded_after_tick,
        anchor.velocity,
        &world,
    )
    .unwrap();
    assert_eq!(
        outcome,
        Some(PhysicsCorrectionOutcome::Replayed {
            corrected_tick: anchor.tick,
            replayed_ticks: 2,
        })
    );
    assert_eq!(physics.state().unwrap().tick, 103);
    assert!(physics.retains_tick(anchor.tick));
    assert_eq!(ticker.pending_count(), 2);
}

#[test]
fn a_future_prediction_correction_waits_on_the_current_frame_for_a_later_rewind() {
    let world = VersionedFloor(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let mut ticker = ticker_with_samples(frame.samples.iter().cloned());
    let before = physics.state().cloned();
    let outcome = crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        [30.0, 2.62, 0.0],
        500,
        true,
        [0.0; 3],
        &world,
    )
    .unwrap();
    assert_eq!(outcome, None);
    assert_eq!(physics.state().cloned(), before);
    assert_eq!(ticker.pending_count(), 3);

    let anchor = &frame.samples[0];
    let position = anchor.position;
    let outcome = crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        position,
        anchor.tick,
        true,
        anchor.velocity,
        &world,
    )
    .unwrap();
    assert!(matches!(
        outcome,
        Some(PhysicsCorrectionOutcome::Replayed {
            replayed_ticks: 2,
            ..
        })
    ));
    assert_eq!(physics.state().unwrap().tick, 103);
    assert_eq!(physics.state().unwrap().position.x, 30.0);
    assert_eq!(physics.state().unwrap().velocity.x, 0.0);
}

#[test]
fn a_later_correction_to_the_same_frame_supersedes_the_deferred_correction() {
    let world = VersionedFloor(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let mut ticker = ticker_with_samples(frame.samples.iter().cloned());
    crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        [30.0, 2.62, 0.0],
        500,
        true,
        [0.0; 3],
        &world,
    )
    .unwrap();
    let anchor = &frame.samples[1];
    let mut position = anchor.position;
    position[0] += 0.01;
    crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        position,
        anchor.tick,
        true,
        anchor.velocity,
        &world,
    )
    .unwrap();
    assert!((physics.state().unwrap().position.x - f64::from(position[0])).abs() < 1.0e-7);
}

#[test]
fn later_server_motion_overrides_deferred_velocity_on_the_same_frame() {
    let world = VersionedFloor(1);
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &world,
    );
    let mut ticker = ticker_with_samples(frame.samples.iter().cloned());
    crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        [30.0, 3.0, 0.0],
        500,
        false,
        [0.0; 3],
        &world,
    )
    .unwrap();
    let tick = physics.queue_server_motion([0.5, 0.1, 0.0], 102).unwrap();
    reconcile_timeline_rewind(&mut ticker, &mut physics, tick, &world).unwrap();
    assert!(physics.state().unwrap().position.x > 30.4);
}

/// A deferred relocation cannot carry a wall collision into an unobstructed ladder tick.
#[test]
fn a_deferred_prediction_correction_clears_collision_flags_before_ladder_replay() {
    let world = ClimbableWall(VersionedWall(1));
    let (mut physics, frame) = collided_prediction(&world);
    let anchor = &frame.samples[0];
    assert!(frame.samples[frame.samples.len() - 2].horizontal_collision);
    let final_tick = physics.state().unwrap().tick;
    let mut ticker = ticker_with_samples(frame.samples.iter().cloned());
    let destination = [0.0, 3.0 + protocol::PLAYER_NETWORK_OFFSET, 8.0];
    assert_eq!(
        crate::movement::reconcile_prediction_correction(
            &mut ticker,
            &mut physics,
            destination,
            500,
            false,
            [0.0; 3],
            &world,
        )
        .unwrap(),
        None
    );
    assert!(matches!(
        crate::movement::reconcile_prediction_correction(
            &mut ticker,
            &mut physics,
            anchor.position,
            anchor.tick,
            anchor.grounded_after_tick,
            anchor.velocity,
            &world,
        )
        .unwrap(),
        Some(PhysicsCorrectionOutcome::Replayed { .. })
    ));
    let replayed = physics.sample_at(final_tick).unwrap();
    assert!(!replayed.horizontal_collision);
    assert!(
        replayed.movement[1] <= 0.0,
        "the old wall collision invented a ladder ascent: {:?}",
        replayed.movement
    );
    assert!(replayed.velocity[1] < 0.0);
    assert!(replayed.position[1] <= destination[1]);
}

/// The original floor is available while the deferred destination can still be loading.
struct DeferredDestinationWorld {
    destination_loaded: bool,
}

impl CollisionWorld for DeferredDestinationWorld {
    /// Rejects collision queries at the destination until its terrain arrives.
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        if !self.destination_loaded && query.max.x > 16.0 {
            return Err(WorldQueryError::UnknownRuntimeId {
                runtime_id: 99,
                block: [32, 0, 0],
            });
        }
        VersionedFloor(1).collision_boxes(query)
    }

    /// Keeps material identity stable while collision availability changes.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        VersionedFloor(1).block_physics(block)
    }
}

/// Retains three ordinary ticks before a correction targets unavailable terrain.
fn prediction_before_unavailable_destination(
    world: &DeferredDestinationWorld,
) -> (
    LocalPhysicsController,
    MovementTicker,
    Vec<crate::movement::PhysicsMovementSample>,
) {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(
        [0.0, 1.000_01 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        100,
        true,
    );
    let frame = physics.advance(Duration::from_millis(150), Default::default(), world);
    assert!(frame.blocked.is_none());
    assert_eq!(frame.samples.len(), 3);
    let ticker = ticker_with_samples(frame.samples.iter().cloned());
    (physics, ticker, frame.samples)
}

/// A replay fallback retains the most recent later-frame spatial authority while terrain loads.
#[test]
fn unavailable_replay_keeps_the_latest_deferred_destination() {
    let world = DeferredDestinationWorld {
        destination_loaded: false,
    };
    let (mut physics, mut ticker, samples) = prediction_before_unavailable_destination(&world);
    let mut destination = samples[2].position;
    destination[0] = 32.0;
    crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        destination,
        500,
        true,
        [0.0; 3],
        &world,
    )
    .unwrap();
    let next = physics.advance(Duration::from_millis(50), Default::default(), &world);
    assert_eq!(next.samples.len(), 1);
    ticker
        .enqueue_completed_physics(next.samples[0].clone())
        .unwrap();
    destination[0] = 48.0;
    crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        destination,
        600,
        true,
        [0.0; 3],
        &world,
    )
    .unwrap();
    let anchor = &samples[0];
    let outcome = crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        anchor.position,
        anchor.tick,
        anchor.grounded_after_tick,
        anchor.velocity,
        &world,
    )
    .unwrap();
    assert!(matches!(
        outcome,
        Some(PhysicsCorrectionOutcome::Snapped { .. })
    ));
    assert_eq!(physics.network_position(), Some(destination));
    let loaded = DeferredDestinationWorld {
        destination_loaded: true,
    };
    // The first frame consumes the snap's elapsed-time barrier; the second resumes travel.
    for _ in 0..2 {
        let resumed = physics.advance(Duration::from_millis(50), Default::default(), &loaded);
        assert!(resumed.blocked.is_none());
    }
    assert_eq!(physics.network_position().unwrap()[0], destination[0]);
}

/// Motion applied on or after a deferred frame remains authoritative when replay falls back.
#[test]
fn unavailable_replay_keeps_later_motion_after_the_deferred_destination() {
    for motion_tick in [102, 103] {
        let world = DeferredDestinationWorld {
            destination_loaded: false,
        };
        let (mut physics, mut ticker, samples) = prediction_before_unavailable_destination(&world);
        let mut destination = samples[2].position;
        destination[0] = 32.0;
        crate::movement::reconcile_prediction_correction(
            &mut ticker,
            &mut physics,
            destination,
            500,
            true,
            [0.0; 3],
            &world,
        )
        .unwrap();
        let motion = [0.5, 0.1, 0.0];
        physics.queue_server_motion(motion, motion_tick);
        let anchor = &samples[0];
        crate::movement::reconcile_prediction_correction(
            &mut ticker,
            &mut physics,
            anchor.position,
            anchor.tick,
            anchor.grounded_after_tick,
            anchor.velocity,
            &world,
        )
        .unwrap();
        assert_eq!(physics.network_position(), Some(destination));
        assert_eq!(
            physics.state().unwrap().velocity,
            sim::Vec3::new(
                f64::from(motion[0]),
                f64::from(motion[1]),
                f64::from(motion[2])
            ),
            "motion stamped {motion_tick} must survive the fallback"
        );
    }
}

/// An incoming correction replaces a deferred correction on the same frame boundary.
#[test]
fn unavailable_replay_keeps_the_trigger_when_no_later_deferred_frame_remains() {
    for has_superseded_deferred in [false, true] {
        let world = DeferredDestinationWorld {
            destination_loaded: false,
        };
        let (mut physics, mut ticker, samples) = prediction_before_unavailable_destination(&world);
        let mut destination = samples[2].position;
        destination[0] = 48.0;
        if has_superseded_deferred {
            crate::movement::reconcile_prediction_correction(
                &mut ticker,
                &mut physics,
                destination,
                500,
                true,
                [0.0; 3],
                &world,
            )
            .unwrap();
        }
        destination[0] = 32.0;
        crate::movement::reconcile_prediction_correction(
            &mut ticker,
            &mut physics,
            destination,
            samples[1].tick,
            true,
            [0.0; 3],
            &world,
        )
        .unwrap();
        assert_eq!(physics.network_position(), Some(destination));
    }
}

/// Explicit teleports discard earlier deferred destinations rather than using replay fallback.
#[test]
fn an_explicit_teleport_snap_discards_deferred_destinations() {
    let world = DeferredDestinationWorld {
        destination_loaded: false,
    };
    let (mut physics, mut ticker, samples) = prediction_before_unavailable_destination(&world);
    let mut destination = samples[2].position;
    destination[0] = 48.0;
    crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        destination,
        500,
        true,
        [0.0; 3],
        &world,
    )
    .unwrap();
    destination[0] = 32.0;
    assert!(matches!(
        crate::movement::reconcile_move_player_teleport(
            &mut ticker,
            &mut physics,
            destination,
            samples[0].tick,
            true,
            &world,
        )
        .unwrap(),
        PhysicsCorrectionOutcome::Snapped { .. }
    ));
    assert_eq!(physics.network_position(), Some(destination));
}

/// An unretained source tick snaps the incoming authority without replaying older deferred state.
fn assert_unretained_anchor_replaces_deferred_destination(correction_tick: u64) {
    let world = DeferredDestinationWorld {
        destination_loaded: false,
    };
    let mut physics = LocalPhysicsController::default();
    physics.set_rewind_history_size(2);
    physics.reanchor_network_position(
        [0.0, 1.000_01 + protocol::PLAYER_NETWORK_OFFSET, 0.0],
        100,
        true,
    );
    let frame = physics.advance(Duration::from_millis(150), Default::default(), &world);
    assert!(frame.blocked.is_none());
    assert_eq!(frame.samples.len(), 3);
    assert!(!physics.retains_tick(frame.samples[0].tick));
    assert!(!physics.retains_tick(correction_tick));
    let current = frame.samples.last().unwrap();
    assert!(physics.retains_tick(current.tick));
    let mut ticker = ticker_with_samples(frame.samples.iter().cloned());
    let mut destination = current.position;
    destination[0] = 48.0;
    crate::movement::reconcile_prediction_correction(
        &mut ticker,
        &mut physics,
        destination,
        500,
        true,
        [0.0; 3],
        &world,
    )
    .unwrap();
    destination[0] = 8.0;
    let velocity = [0.25, 0.0, 0.0];
    assert_eq!(
        reconcile_committed_correction(
            &mut ticker,
            &mut physics,
            destination,
            correction_tick,
            false,
            Some(velocity),
            &world,
        )
        .unwrap(),
        Some(PhysicsCorrectionOutcome::Snapped { tick: current.tick })
    );
    assert_eq!(physics.network_position(), Some(destination));
    let state = physics.state().unwrap();
    assert!(!state.on_ground);
    assert_eq!(
        state.velocity,
        sim::Vec3::new(f64::from(velocity[0]), 0.0, 0.0)
    );
}

/// A normal unmarked move has no retained source frame and its live destination wins.
#[test]
fn unretained_replay_keeps_zero_tick_incoming_anchor() {
    assert_unretained_anchor_replaces_deferred_destination(0);
}

/// An expired correction uses the existing snap policy instead of an unrelated deferred frame.
#[test]
fn unretained_replay_keeps_expired_tick_incoming_anchor() {
    assert_unretained_anchor_replaces_deferred_destination(101);
}
