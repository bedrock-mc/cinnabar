#[test]
fn correction_during_terminal_drain_keeps_definitely_unsent_retry_pending_and_times_out() {
    let (mut ticker, _physics, admitted) =
        replay_with_admitted_future_ticks(MovementTicker::default());

    ticker.begin_terminal_drain();
    for identity in admitted {
        assert!(ticker.resolve_cancelled_physics_send(identity, true));
    }

    assert_eq!(
        ticker.pending_count(),
        2,
        "terminal drain must retain definitely-unsent replay work that it cannot flush"
    );
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::BudgetDeferred
    );

    let deadline = Instant::now();
    let mut acceptance = AcceptanceRun::new(Some(60), None, false, false);
    acceptance.deadline = Some(deadline);
    assert_eq!(
        acceptance.phase3_terminal_drain_decision(
            deadline + TRANSPARENT_PRESENTATION_EXIT_GRACE,
            true,
            ticker.pending_count(),
        ),
        Phase3TerminalDrainDecision::TimedOut,
        "stranded replay work must make terminal acceptance fail closed"
    );
}

#[test]
fn production_replay_reconciliation_notifies_the_network_invalidation_channel() {
    let (network, reanchor) = crate::runtime::network::session::NetworkHandle::stub();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    let mut ticker = network.movement_ticker();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    flush_player_auth_inputs(
        &mut ticker,
        3,
        Some(evidence_context()),
        |_identity, _packet| Ok::<_, &str>(()),
    )
    .unwrap();

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

    assert_eq!(
        *reanchor.borrow(),
        ticker.reanchor_epoch(),
        "production reconciliation must publish its new epoch to the network worker"
    );
    assert_ne!(*reanchor.borrow(), 0);
}

#[test]
fn app_respawn_snap_publishes_its_epoch_from_the_authority_event() {
    let (network, reanchor) = crate::runtime::network::session::NetworkHandle::stub();
    let (mut ticker, mut physics, _admitted) =
        replay_with_admitted_future_ticks(network.movement_ticker());

    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [8.0, 71.620_01, 9.0],
        0,
        false,
        PhysicsCorrectionMode::Snap,
        &VersionedFloor(1),
    )
    .unwrap();

    assert_eq!(
        *reanchor.borrow(),
        ticker.reanchor_epoch(),
        "the respawn snap event must publish its new epoch without an outer call site"
    );
    assert_ne!(*reanchor.borrow(), 0);
}

#[test]
fn world_stream_fatal_deactivation_publishes_before_early_return() {
    let (network, reanchor) = crate::runtime::network::session::NetworkHandle::stub();
    let mut ticker = network.movement_ticker();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    assert_eq!(*reanchor.borrow(), ticker.reanchor_epoch());

    ticker.deactivate();

    assert_eq!(
        *reanchor.borrow(),
        ticker.reanchor_epoch(),
        "world-stream fatal deactivation must publish before its caller returns"
    );
}

#[test]
fn bootstrap_reset_publishes_before_equipment_identity_early_exit() {
    let (network, reanchor) = crate::runtime::network::session::NetworkHandle::stub();
    let mut ticker = network.movement_ticker();

    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);

    assert_eq!(
        *reanchor.borrow(),
        ticker.reanchor_epoch(),
        "bootstrap reset must publish even when equipment identity routing exits early"
    );
}

#[test]
fn snap_fallback_invalidates_transport_owned_commands() {
    let (network, reanchor) = crate::runtime::network::session::NetworkHandle::stub();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    let mut ticker = network.movement_ticker();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ticker
        .enqueue_completed_physics(frame.samples[0].clone())
        .unwrap();
    let mut admitted = None;
    flush_player_auth_inputs(
        &mut ticker,
        1,
        Some(evidence_context()),
        |identity, _packet| {
            admitted = Some(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    let admitted_epoch = admitted.unwrap().reanchor_epoch;

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [4.0, 70.620_01, 5.0],
            999,
            false,
            PhysicsCorrectionMode::ReplayIfRetained,
            &VersionedFloor(1),
        ),
        Ok(PhysicsCorrectionOutcome::Snapped { tick: 999 })
    );
    assert!(
        ticker.reanchor_epoch() > admitted_epoch,
        "snap fallback must advance the position-authority epoch"
    );
    assert_eq!(
        *reanchor.borrow(),
        ticker.reanchor_epoch(),
        "snap fallback must publish its invalidation epoch to the network worker"
    );
    assert!(ticker.physics_is_authorized());
    assert!(physics.is_active());
}
