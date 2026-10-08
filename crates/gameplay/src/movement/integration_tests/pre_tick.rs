// Pre-tick state for build actions across every path that completes, moves or replays ticks.

/// Build actions precede each simulation tick, so they read the previous tick's end state.
#[test]
fn build_actions_observe_the_end_state_of_the_previous_tick() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    let mut ground = completed_sample(1_001, [1.2, 64.0, 2.0]);
    ground.velocity = [0.12, -0.0784, 0.0];
    ticker.enqueue_completed_physics(ground).unwrap();
    let anchored = ticker.pre_tick_sample().unwrap();
    assert_eq!(
        (anchored.tick, anchored.position, anchored.delta),
        (1_000, [1.0, 64.0, 2.0], [0.0; 3])
    );
    let mut jump = completed_sample(1_002, [1.4, 64.42, 2.0]);
    jump.velocity = [0.12, 0.3332, 0.0];
    ticker.enqueue_completed_physics(jump).unwrap();
    let queued = ticker.pre_tick_sample().unwrap();
    assert_eq!(
        (queued.tick, queued.position, queued.delta),
        (1_001, [1.2, 64.0, 2.0], [0.12, -0.0784, 0.0])
    );
    ticker.pop_pending().unwrap();
    assert_eq!(ticker.pre_tick_sample(), Some(queued));
    ticker.pop_pending().unwrap();
    assert_eq!(ticker.pre_tick_sample(), None);
    ticker
        .enqueue_completed_physics(completed_sample(1_003, [1.6, 64.2, 2.0]))
        .unwrap();
    let sent = ticker.pre_tick_sample().unwrap();
    assert_eq!((sent.tick, sent.delta), (1_002, [0.12, 0.3332, 0.0]));
}

/// A send the transport refuses returns to the queue without losing its predecessor.
#[test]
fn a_refused_send_keeps_the_pre_tick_state_of_the_restored_tick() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(1_001, [1.2, 64.0, 2.0]))
        .unwrap();
    flush_player_auth_inputs(&mut ticker, 1, Some(evidence_context()), |_, _| {
        Ok::<_, &'static str>(())
    })
    .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(1_002, [1.4, 64.0, 2.0]))
        .unwrap();
    let before = ticker.pre_tick_sample().unwrap();
    assert_eq!(before.tick, 1_001);
    for _ in 0..2 {
        assert!(
            flush_player_auth_inputs(&mut ticker, 1, Some(evidence_context()), |_, _| Err("full"))
                .is_err()
        );
        assert_eq!(ticker.pre_tick_sample(), Some(before));
    }
}

/// A queue that resumes right after the corrected tick reads the corrected anchor.
#[test]
fn a_replayed_correction_keeps_its_anchor_as_the_pre_tick_state() {
    for (queued_ms, queued_after) in [(100, 1), (50, 0)] {
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
        assert_eq!(ticker.pending_samples().len(), queued_after);
        if queued_after == 0 {
            let next = physics.advance_with_context(
                Duration::from_millis(50),
                forward_physics_input(),
                PhysicsSampleContext::default(),
                &VersionedFloor(1),
            );
            for sample in next.samples {
                ticker.enqueue_completed_physics(sample).unwrap();
            }
        }
        let anchor = ticker.pre_tick_sample().unwrap();
        assert_eq!((anchor.tick, anchor.position), (101, [0.25, 2.620_01, 0.0]));
    }
}

/// A cancelled transport-owned replay tick returns to the queue with the corrected
/// anchor, then its own replayed state, as predecessors.
#[test]
fn restored_replay_sends_keep_their_replayed_predecessors() {
    let (mut ticker, _physics, admitted) =
        replay_with_admitted_future_ticks(MovementTicker::default());
    assert!(ticker.resolve_cancelled_physics_send(admitted[0], true));
    let anchor = ticker.pre_tick_sample().unwrap();
    assert_eq!((anchor.tick, anchor.position), (101, [0.25, 2.620_01, 0.0]));
    let restored = ticker.newest_unsent_sample().unwrap();
    assert!(ticker.resolve_cancelled_physics_send(admitted[1], true));
    let replayed = ticker.pre_tick_sample().unwrap();
    assert_eq!((replayed.tick, replayed.position), (102, restored.position));
}

/// A teleport snap restarts the timeline from the snapped pose with cleared motion.
#[test]
fn a_snapped_correction_is_the_next_tick_pre_tick_state() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    let advance = |physics: &mut LocalPhysicsController| {
        physics
            .advance_with_context(
                Duration::from_millis(50),
                forward_physics_input(),
                PhysicsSampleContext::default(),
                &VersionedFloor(1),
            )
            .samples
    };
    for sample in advance(&mut physics) {
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
    // The snapped clock may need more than one frame to complete its next tick.
    for _ in 0..4 {
        for sample in advance(&mut physics) {
            ticker.enqueue_completed_physics(sample).unwrap();
        }
        if ticker.newest_unsent_sample().is_some() {
            break;
        }
    }
    let snapped = ticker.pre_tick_sample().unwrap();
    assert_eq!(
        (snapped.tick, snapped.position, snapped.delta),
        (tick, [8.0, 71.620_01, 9.0], [0.0; 3])
    );
}

/// A respawn or dimension reanchor is the predecessor of the first following tick.
#[test]
fn a_surface_reanchor_is_the_next_tick_pre_tick_state() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 1_000, [1.0, 64.0, 2.0]);
    ticker.set_source(MovementSource::Physics);
    for tick in [1_001, 1_002] {
        ticker
            .enqueue_completed_physics(completed_sample(tick, [1.0, 64.0, 2.0]))
            .unwrap();
    }
    ticker.reanchor_surface_spawn(1_005, [30.0, 80.0, -4.0]);
    ticker
        .enqueue_completed_physics(completed_sample(1_006, [30.0, 80.0, -4.0]))
        .unwrap();
    let anchored = ticker.pre_tick_sample().unwrap();
    assert_eq!(
        (anchored.tick, anchored.position, anchored.delta),
        (1_005, [30.0, 80.0, -4.0], [0.0; 3])
    );
}

/// Ticks withheld during a respawn search still end in a state the next tick follows.
#[test]
fn a_withheld_respawn_tick_is_the_next_tick_pre_tick_state() {
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
    ticker
        .enqueue_completed_physics(completed_sample(1_003, [5.0, 70.0, 5.0]))
        .unwrap();
    let withheld = ticker.pre_tick_sample().unwrap();
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
    ticker.reset(8, 2_000, [3.0, 64.0, 3.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(2_001, [3.0, 64.0, 3.0]))
        .unwrap();
    let anchored = ticker.pre_tick_sample().unwrap();
    assert_eq!(
        (anchored.tick, anchored.position),
        (2_000, [3.0, 64.0, 3.0])
    );
}
