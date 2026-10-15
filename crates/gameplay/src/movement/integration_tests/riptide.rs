use super::*;

#[test]
fn riptide_waits_for_a_tick_and_emits_one_start_and_one_stop() {
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    let origin = [0.0, 65.620_01, 0.0];
    physics.reanchor_network_position(origin, 100, false);
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    physics.request_riptide(3, [0.0; 2]);
    assert!(
        physics
            .advance(Duration::ZERO, MovementInput::default(), &VersionedFloor(1))
            .samples
            .is_empty()
    );
    for index in 0..20 {
        let frame = physics.advance(
            simulation_clock::TICK_DURATION,
            MovementInput::default(),
            &VersionedFloor(1),
        );
        let sample = frame.samples.into_iter().next().unwrap();
        if index == 0 {
            assert_eq!(sample.movement[2], 3.0);
        }
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let snapshots = ticker.pending_snapshots();
    assert_eq!(
        snapshots
            .iter()
            .filter(|sample| sample.flags.bits() & PlayerInputFlags::START_SPIN_ATTACK.bits() != 0)
            .count(),
        1
    );
    assert_eq!(
        snapshots
            .iter()
            .filter(|sample| sample.flags.bits() & PlayerInputFlags::STOP_SPIN_ATTACK.bits() != 0)
            .count(),
        1
    );
}

#[test]
fn correction_replays_the_launch_once_and_rebuilds_spin_flags() {
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    let origin = [0.0, 65.620_01, 0.0];
    physics.reanchor_network_position(origin, 100, false);
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let before = physics.advance(
        simulation_clock::TICK_DURATION,
        MovementInput::default(),
        &VersionedFloor(1),
    );
    ticker
        .enqueue_completed_physics(before.samples[0].clone())
        .unwrap();
    physics.request_riptide(2, [0.0; 2]);
    let launch = physics.advance(
        simulation_clock::TICK_DURATION * 3,
        MovementInput::default(),
        &VersionedFloor(1),
    );
    for sample in launch.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let expected = physics.state().unwrap().clone();
    let anchor = ticker.pending_samples()[0].clone();
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        anchor.snapshot.position,
        101,
        false,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    assert_eq!(physics.state().unwrap().velocity, expected.velocity);
    assert_eq!(
        physics.state().unwrap().spin_attack_ticks,
        expected.spin_attack_ticks
    );
    assert_eq!(
        ticker
            .pending_snapshots()
            .iter()
            .filter(|sample| sample.flags.bits() & PlayerInputFlags::START_SPIN_ATTACK.bits() != 0)
            .count(),
        1
    );
}

#[test]
fn riptide_keeps_release_facing_when_look_changes_and_replays() {
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    let origin = [0.0, 65.620_01, 0.0];
    physics.reanchor_network_position(origin, 100, false);
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let before = physics.advance(
        simulation_clock::TICK_DURATION,
        MovementInput::default(),
        &VersionedFloor(1),
    );
    let anchor = before.samples[0].position;
    ticker
        .enqueue_completed_physics(before.samples[0].clone())
        .unwrap();
    physics.request_riptide(3, [-30.0, 90.0]);
    let context = PhysicsSampleContext {
        pitch: 60.0,
        ..Default::default()
    };
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION,
        MovementInput {
            yaw_degrees: 180.0,
            ..Default::default()
        },
        context,
        &VersionedFloor(1),
    );
    let sample = frame.samples.into_iter().next().unwrap();
    assert!(
        sample.movement[0] < -2.5,
        "the impulse keeps the release yaw: {:?}",
        sample.movement
    );
    assert!(
        sample.movement[1] > 1.4,
        "the impulse keeps the release pitch: {:?}",
        sample.movement
    );
    assert!(sample.movement[2].abs() < 0.001);
    assert_eq!(sample.pitch, context.pitch);
    assert_eq!(sample.yaw, 180.0);
    ticker.enqueue_completed_physics(sample).unwrap();
    let expected = physics.state().unwrap().clone();
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        anchor,
        101,
        false,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    assert_eq!(physics.state().unwrap(), &expected);
}

#[test]
fn unsupported_riptide_is_retired_without_stalling_later_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 65.620_01, 0.0], 100, false);
    physics.request_riptide(u8::MAX, [0.0; 2]);
    for tick in 101..=103 {
        let frame = physics.advance(
            simulation_clock::TICK_DURATION,
            MovementInput::default(),
            &VersionedFloor(1),
        );
        assert!(
            frame.blocked.is_none(),
            "an unsupported release must not block later movement: {:?}",
            frame.blocked
        );
        assert_eq!(frame.samples.len(), 1);
        assert_eq!(frame.samples[0].tick, tick);
        assert!(!frame.samples[0].processed.spin_started);
        assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
    }
}

#[test]
fn riptide_that_exceeds_the_budget_with_existing_motion_is_retired() {
    let origin = [0.0, 65.620_01, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    physics.queue_server_motion([0.0, 0.0, 120.0], 0);
    let mut ordinary = physics.clone();
    physics.request_riptide(20, [0.0; 2]);
    for tick in 101..=103 {
        let frame = physics.advance(
            simulation_clock::TICK_DURATION,
            MovementInput::default(),
            &VersionedFloor(1),
        );
        let baseline = ordinary.advance(
            simulation_clock::TICK_DURATION,
            MovementInput::default(),
            &VersionedFloor(1),
        );
        assert!(
            frame.blocked.is_none(),
            "the launch must not keep retrying: {:?}",
            frame.blocked
        );
        assert_eq!(frame.samples.len(), 1);
        assert_eq!(frame.samples[0].tick, tick);
        assert!(!frame.samples[0].processed.spin_started);
        assert_eq!(physics.state(), ordinary.state());
        assert_eq!(frame.samples[0].position, baseline.samples[0].position);
    }
}
