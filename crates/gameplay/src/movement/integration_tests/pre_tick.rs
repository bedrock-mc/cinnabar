// The state build actions observe before the next tick, across every path that completes,
// moves or replays ticks.

/// Build actions precede each simulation tick, so they read the last completed tick's end.
#[test]
fn build_actions_observe_the_end_state_of_the_last_completed_tick() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    let anchored = ticker.build_action_state().unwrap();
    assert_eq!(
        (anchored.tick, anchored.position, anchored.delta),
        (1_000, [1.0, 64.0, 2.0], [0.0; 3])
    );
    let mut ground = completed_sample(1_001, [1.2, 64.0, 2.0]);
    ground.velocity = [0.12, -0.0784, 0.0];
    ticker.enqueue_completed_physics(ground).unwrap();
    let queued = ticker.build_action_state().unwrap();
    assert_eq!(
        (queued.tick, queued.position, queued.delta),
        (1_001, [1.2, 64.0, 2.0], [0.12, -0.0784, 0.0])
    );
    ticker.pop_pending().unwrap();
    assert_eq!(ticker.build_action_state(), Some(queued));
    let mut jump = completed_sample(1_002, [1.4, 64.42, 2.0]);
    jump.velocity = [0.12, 0.3332, 0.0];
    ticker.enqueue_completed_physics(jump).unwrap();
    let jumped = ticker.build_action_state().unwrap();
    assert_eq!((jumped.tick, jumped.delta), (1_002, [0.12, 0.3332, 0.0]));
}

/// A send the transport refuses returns to the queue without moving the observed state.
#[test]
fn a_refused_send_keeps_the_build_action_state() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    for tick in [1_001, 1_002] {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [1.2, 64.0, 2.0]))
            .unwrap();
    }
    let before = ticker.build_action_state().unwrap();
    assert_eq!(before.tick, 1_002);
    for _ in 0..2 {
        assert!(
            flush_player_auth_inputs(&mut ticker, 2, Some(evidence_context()), |_, _| Err("full"))
                .is_err()
        );
        assert_eq!(ticker.build_action_state(), Some(before));
    }
}

/// A replay rewrites the observed state; with no replayed ticks it is the corrected anchor.
#[test]
fn a_replayed_correction_rewrites_the_build_action_state() {
    for (queued_ms, replayed) in [(100, true), (50, false)] {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
        let frame = physics.advance_with_context(
            Duration::from_millis(queued_ms),
            forward_physics_input(),
            PhysicsSampleContext::default(),
            &VersionedFloor(1),
        );
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
        ticker.set_source(MovementSource::Physics);
        for sample in frame.samples {
            ticker.enqueue_completed_physics(sample).unwrap();
        }
        let before = ticker.build_action_state().unwrap();
        ticker.pop_pending().unwrap();
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [0.25, 2.620_01, 0.0],
            101,
            true,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        )
        .unwrap();
        let after = ticker.build_action_state().unwrap();
        assert_eq!(after.tick, before.tick);
        if replayed {
            assert_ne!(after.position, before.position);
        } else {
            assert_eq!(after.position, [0.25, 2.620_01, 0.0]);
        }
    }
}

/// Cancelled transport-owned replay ticks return to the queue; the replayed state stays.
#[test]
fn restored_replay_sends_keep_the_replayed_build_action_state() {
    let (mut ticker, _physics, admitted) =
        replay_with_admitted_future_ticks(MovementTicker::default());
    let replayed = ticker.build_action_state().unwrap();
    for identity in admitted {
        assert!(ticker.resolve_cancelled_physics_send(identity, true));
    }
    let restored = ticker.newest_unsent_sample().unwrap();
    assert_eq!(
        (replayed.tick, replayed.position),
        (restored.tick, restored.position)
    );
    assert_eq!(ticker.build_action_state(), Some(replayed));
}

/// A teleport snap restarts the timeline from the snapped pose with cleared motion.
#[test]
fn a_snapped_correction_is_the_build_action_state() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    ticker.pop_pending().unwrap();
    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [8.0, 71.620_01, 9.0],
        101,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(1),
    )
    .unwrap();
    let PhysicsCorrectionOutcome::Snapped { tick } = outcome else {
        panic!("snap correction");
    };
    let snapped = ticker.build_action_state().unwrap();
    assert_eq!(
        (snapped.tick, snapped.position, snapped.delta),
        (tick, [8.0, 71.620_01, 9.0], [0.0; 3])
    );
}

/// A respawn or dimension reanchor is the state the following tick's build actions read.
#[test]
fn a_surface_reanchor_is_the_build_action_state() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    for tick in [1_001, 1_002] {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [1.0, 64.0, 2.0]))
            .unwrap();
    }
    ticker.reanchor_surface_spawn(1_005, [30.0, 80.0, -4.0]);
    let anchored = ticker.build_action_state().unwrap();
    assert_eq!(
        (anchored.tick, anchored.position, anchored.delta),
        (1_005, [30.0, 80.0, -4.0], [0.0; 3])
    );
}

/// Ticks withheld during a respawn search still end in a state build actions read.
#[test]
fn a_withheld_respawn_tick_is_the_build_action_state() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(1_001, [1.0, 64.0, 2.0]))
        .unwrap();
    ticker.begin_respawn_search();
    ticker
        .withhold_respawn_input(completed_sample(1_002, [5.0, 70.0, 5.0]))
        .unwrap();
    let withheld = ticker.build_action_state().unwrap();
    assert_eq!(
        (withheld.tick, withheld.position),
        (1_002, [5.0, 70.0, 5.0])
    );
}

/// Deactivation forgets the session's tick-end states.
#[test]
fn deactivation_forgets_tick_end_states() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(1_001, [1.0, 64.0, 2.0]))
        .unwrap();
    ticker.deactivate();
    assert_eq!(ticker.build_action_state(), None);
    ticker.reset(8, 2_000, [3.0, 64.0, 3.0]);
    ticker.set_source(MovementSource::Physics);
    let anchored = ticker.build_action_state().unwrap();
    assert_eq!(
        (anchored.tick, anchored.position),
        (2_000, [3.0, 64.0, 3.0])
    );
}
