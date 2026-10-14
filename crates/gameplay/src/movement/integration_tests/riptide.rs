use super::*;

#[test]
fn riptide_waits_for_a_tick_and_emits_one_start_and_one_stop() {
    let mut physics = LocalPhysicsController::default();
    let mut ticker = MovementTicker::default();
    let origin = [0.0, 65.620_01, 0.0];
    physics.reanchor_network_position(origin, 100, false);
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    physics.request_riptide(3);
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
    physics.request_riptide(2);
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
