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

#[test]
fn riptide_socket_receipt_after_snap_resolves_the_restored_launch() {
    for written in [false, true] {
        let origin = [0.0, 65.620_01, 0.0];
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(origin, 100, false);
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, origin);
        ticker.set_source(MovementSource::Physics);
        ticker.request_riptide(&mut physics, 3, [0.0; 2]);
        let launch = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &VersionedFloor(1),
            )
            .samples
            .remove(0);
        assert!(launch.processed.spin_started);
        ticker.enqueue_completed_physics(launch).unwrap();
        let mut identity = None;
        flush_player_auth_inputs(&mut ticker, 1, Some(evidence_context()), |id, _| {
            identity = Some(id);
            Ok::<_, ()>(())
        })
        .unwrap();
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            origin,
            100,
            false,
            PhysicsCorrectionMode::Snap,
            &VersionedFloor(1),
        )
        .unwrap();
        assert!(
            !ticker.can_advance_physics_frame(),
            "the old socket receipt must settle first"
        );
        if written {
            assert!(ticker.acknowledge_physics_send(identity.unwrap()));
        } else {
            assert!(ticker.resolve_cancelled_physics_send(identity.unwrap(), true));
        }
        assert!(ticker.can_advance_physics_frame());
        ticker.settle_riptide_input(&mut physics);
        physics.advance(Duration::ZERO, MovementInput::default(), &VersionedFloor(1));
        let next = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &VersionedFloor(1),
            )
            .samples
            .remove(0);
        assert_eq!(next.processed.spin_started, !written);
    }
}

#[test]
fn riptide_release_slowdown_ends_inside_a_two_tick_frame() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 65.620_01, 0.0], 100, false);
    let input = physics_movement_input([0.0, 1.0], 0.0, true, false, false, false, None);
    let mut using = physics.clone();
    let using = using
        .advance(
            simulation_clock::TICK_DURATION,
            physics_movement_input([0.0, 1.0], 0.0, true, false, false, false, Some(0.2)),
            &VersionedFloor(1),
        )
        .samples
        .remove(0);
    let mut idle = physics.clone();
    let idle = idle
        .advance(simulation_clock::TICK_DURATION, input, &VersionedFloor(1))
        .samples
        .remove(0);
    let frame = physics.advance_with_context(
        simulation_clock::TICK_DURATION * 2,
        input,
        PhysicsSampleContext {
            release_use_modifier: Some((101, 0.2)),
            ..Default::default()
        },
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 2);
    assert_eq!(frame.samples[0].move_vector, using.move_vector);
    assert_eq!(frame.samples[1].move_vector, idle.move_vector);
}

/// A snap that cancels an unsent release also ends the spin its predicted
/// launch started, since the release and its start action never reach the
/// server; a snap otherwise keeps the spin.
#[test]
fn a_snap_ends_the_spin_of_a_cancelled_unsent_launch() {
    let origin = [0.0, 65.620_01, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let advance = |physics: &mut LocalPhysicsController| {
        physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &VersionedFloor(1),
            )
            .samples
            .remove(0)
    };
    let facing = advance(&mut physics);
    ticker.enqueue_completed_physics(facing).unwrap();
    ticker.hold_release_after_tick(101, vec![protocol::stop_sleeping_packet(42)]);
    ticker.request_riptide(&mut physics, 3, [0.0; 2]);
    let launch = advance(&mut physics);
    assert!(launch.processed.spin_started);
    ticker.enqueue_completed_physics(launch).unwrap();
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        origin,
        102,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(!ticker.has_held_release());
    assert_eq!(physics.state().unwrap().spin_attack_ticks, None);
}

/// A cancelled launch's spin ends with its snap only while it is still that
/// launch's spin: a later server spin survives, and the launch is tracked even
/// after its input leaves a short rewind window.
#[test]
fn a_snap_ends_only_the_cancelled_launch_spin() {
    for (history, server_restart) in [(12, true), (1, false)] {
        let origin = [0.0, 65.620_01, 0.0];
        let mut physics = LocalPhysicsController::default();
        physics.set_rewind_history_size(history);
        physics.reanchor_network_position(origin, 100, false);
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, origin);
        ticker.set_source(MovementSource::Physics);
        let advance = |physics: &mut LocalPhysicsController, ticker: &mut MovementTicker| {
            let sample = physics
                .advance(
                    simulation_clock::TICK_DURATION,
                    MovementInput::default(),
                    &VersionedFloor(1),
                )
                .samples
                .remove(0);
            ticker.enqueue_completed_physics(sample).unwrap();
        };
        advance(&mut physics, &mut ticker);
        ticker.hold_release_after_tick(101, vec![protocol::stop_sleeping_packet(42)]);
        ticker.request_riptide(&mut physics, 3, [0.0; 2]);
        advance(&mut physics, &mut ticker);
        let spin = |value| client_world::MovementFlagUpdate {
            spinning: Some(value),
            ..Default::default()
        };
        if server_restart {
            physics.apply_server_movement_flags(0, spin(false));
            advance(&mut physics, &mut ticker);
            physics.apply_server_movement_flags(0, spin(true));
        } else {
            advance(&mut physics, &mut ticker);
        }
        let tick = physics.state().unwrap().tick;
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            origin,
            tick,
            false,
            PhysicsCorrectionMode::Snap,
            &VersionedFloor(1),
        )
        .unwrap();
        assert_eq!(
            physics.state().unwrap().spin_attack_ticks.is_some(),
            server_restart,
            "history {history}"
        );
    }
}

/// A launch the simulator rejects, as for a spectator, does not claim an
/// active server spin, so a snap cancelling its unsent release keeps the spin.
#[test]
fn a_rejected_launch_does_not_claim_a_server_spin() {
    let origin = [0.0, 65.620_01, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let advance =
        |physics: &mut LocalPhysicsController, ticker: &mut MovementTicker, spectator: bool| {
            let context = PhysicsSampleContext {
                mode_intent: super::super::ModeIntent {
                    spectator,
                    ..Default::default()
                },
                ..PhysicsSampleContext::default()
            };
            let sample = physics
                .advance_with_context(
                    simulation_clock::TICK_DURATION,
                    MovementInput::default(),
                    context,
                    &VersionedFloor(1),
                )
                .samples
                .remove(0);
            ticker.enqueue_completed_physics(sample).unwrap();
        };
    advance(&mut physics, &mut ticker, false);
    let spin = client_world::MovementFlagUpdate {
        spinning: Some(true),
        ..Default::default()
    };
    physics.apply_server_movement_flags(0, spin);
    ticker.hold_release_after_tick(101, vec![protocol::stop_sleeping_packet(42)]);
    ticker.request_riptide(&mut physics, 3, [0.0; 2]);
    advance(&mut physics, &mut ticker, true);
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
    let tick = physics.state().unwrap().tick;
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        origin,
        tick,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
}

/// A retained server spin start that matches the live spin still makes the
/// spin the server's, so a snap cancelling an unsent launch keeps it.
#[test]
fn a_retained_server_spin_that_matches_live_survives_a_cancelling_snap() {
    let origin = [0.0, 65.620_01, 0.0];
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position(origin, 100, false);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, origin);
    ticker.set_source(MovementSource::Physics);
    let advance = |physics: &mut LocalPhysicsController, ticker: &mut MovementTicker| {
        let sample = physics
            .advance(
                simulation_clock::TICK_DURATION,
                MovementInput::default(),
                &VersionedFloor(1),
            )
            .samples
            .remove(0);
        ticker.enqueue_completed_physics(sample).unwrap();
    };
    advance(&mut physics, &mut ticker);
    ticker.hold_release_after_tick(101, vec![protocol::stop_sleeping_packet(42)]);
    ticker.request_riptide(&mut physics, 3, [0.0; 2]);
    advance(&mut physics, &mut ticker);
    let spin = client_world::MovementFlagUpdate {
        spinning: Some(true),
        ..Default::default()
    };
    physics.apply_server_movement_flags(101, spin);
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        origin,
        102,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(physics.state().unwrap().spin_attack_ticks.is_some());
}
