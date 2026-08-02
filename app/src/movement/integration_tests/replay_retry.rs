#[test]
fn retained_correction_replays_physics_and_replaces_only_unsent_fifo_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 3);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    let sent = ticker.pop_pending().unwrap();
    assert_eq!(sent.snapshot.tick, 101);
    let before = ticker.pending_samples();

    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.25, 2.620_01, 0.0],
        101,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();

    assert!(matches!(
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 101,
            replayed_ticks: 2,
        }
    ));
    assert_eq!(physics.state().unwrap().tick, 103);
    assert_eq!(ticker.next_tick(), 104);
    let after = ticker.pending_samples();
    assert_eq!(
        after
            .iter()
            .map(|pending| pending.snapshot.tick)
            .collect::<Vec<_>>(),
        [102, 103]
    );
    assert!(after.iter().all(|pending| pending.session_generation == 7));
    assert_eq!(
        after
            .iter()
            .map(|pending| &pending.world_identity)
            .collect::<Vec<_>>(),
        before
            .iter()
            .map(|pending| &pending.world_identity)
            .collect::<Vec<_>>()
    );
    assert_ne!(after[0].snapshot.position, before[0].snapshot.position);
    assert_ne!(after[1].snapshot.position, before[1].snapshot.position);
    assert_eq!(
        after[0].evidence.network_position, after[0].snapshot.position,
        "replay must update the evidence snapshot to the exact position that will be encoded"
    );
    assert_eq!(
        after[1].evidence.network_position, after[1].snapshot.position,
        "every replay-adjusted packet must retain matching immutable evidence"
    );
}

#[test]
fn retained_correction_cancels_admitted_future_ticks_and_requeues_replayed_positions() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    assert!(frame.blocked.is_none(), "{:?}", frame.blocked);
    assert_eq!(frame.samples.len(), 3);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    let mut confirmed = None;
    flush_player_auth_inputs(
        &mut ticker,
        1,
        Some(evidence_context()),
        |identity, _packet| {
            confirmed = Some(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert!(ticker.acknowledge_physics_send(confirmed.unwrap()));

    let mut admitted = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        2,
        Some(evidence_context()),
        |identity, _packet| {
            admitted.push(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert_eq!(
        admitted
            .iter()
            .map(|identity| identity.tick)
            .collect::<Vec<_>>(),
        [102, 103]
    );
    let admitted_epoch = ticker.reanchor_epoch();

    let outcome = reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.25, 2.620_01, 0.0],
        101,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();
    assert!(matches!(
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 101,
            replayed_ticks: 2,
        }
    ));
    assert!(
        ticker.reanchor_epoch() > admitted_epoch,
        "replay changed transport-owned future ticks without invalidating their admission epoch"
    );

    for identity in admitted {
        assert!(ticker.resolve_cancelled_physics_send(identity, true));
    }
    let replayed = ticker.pending_snapshots();
    assert_eq!(
        replayed
            .iter()
            .map(|snapshot| snapshot.tick)
            .collect::<Vec<_>>(),
        [102, 103]
    );
    assert!(replayed.iter().all(|snapshot| snapshot.position[0] > 0.2));
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::BudgetDeferred,
        "definitely-unsent obsolete packets must restore corrected ticks instead of silently draining"
    );

    let mut readmitted = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        2,
        Some(evidence_context()),
        |identity, _packet| {
            readmitted.push(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert!(
        readmitted
            .iter()
            .all(|identity| identity.reanchor_epoch > admitted_epoch)
    );
    for identity in readmitted {
        assert!(ticker.acknowledge_physics_send(identity));
    }
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained
    );
    let evidence = ticker.take_tick_evidence();
    assert_eq!(
        evidence
            .iter()
            .filter(|sample| sample.tick > 101)
            .map(|sample| sample.network_position)
            .collect::<Vec<_>>(),
        replayed
            .iter()
            .map(|snapshot| snapshot.position)
            .collect::<Vec<_>>()
    );
}

#[test]
fn respawn_snap_after_replay_drops_pre_respawn_retries() {
    let (mut ticker, mut physics, admitted) =
        replay_with_admitted_future_ticks(MovementTicker::default());

    assert_eq!(
        reconcile_candidate_physics_correction(
            &mut ticker,
            &mut physics,
            [8.0, 71.620_01, 9.0],
            0,
            false,
            PhysicsCorrectionMode::Snap,
            &VersionedFloor(1),
        ),
        Ok(PhysicsCorrectionOutcome::Snapped { tick: 103 })
    );
    for identity in admitted {
        assert!(ticker.resolve_cancelled_physics_send(identity, true));
    }

    assert_eq!(
        ticker.pending_count(),
        0,
        "pre-respawn replay retries must not survive the hard respawn anchor"
    );
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained
    );
}

#[test]
fn surface_spawn_reanchor_after_replay_drops_pre_anchor_retries() {
    let (mut ticker, _physics, admitted) =
        replay_with_admitted_future_ticks(MovementTicker::default());

    ticker.reanchor_surface_spawn(103, [8.0, 71.620_01, 9.0]);
    for identity in admitted {
        assert!(ticker.resolve_cancelled_physics_send(identity, true));
    }

    assert_eq!(
        ticker.pending_count(),
        0,
        "pre-anchor replay retries must not survive a surface-spawn reanchor"
    );
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained
    );
}

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
fn newest_retained_tick_correction_invalidates_its_admitted_packet_without_replaying() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 1);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
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
    let admitted = admitted.expect("newest retained tick was admitted");

    let outcome = reconcile_candidate_physics_correction(
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
        outcome,
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 101,
            replayed_ticks: 0,
        }
    );
    assert!(
        ticker.reanchor_epoch() > admitted.reanchor_epoch,
        "correcting only the newest retained tick must still invalidate its admitted command"
    );
    assert!(!ticker.accepting_physics_admissions());
    assert!(ticker.resolve_cancelled_physics_send(admitted, true));
    assert_eq!(ticker.pending_count(), 0);
}

#[test]
fn invalidated_retries_resolve_before_a_newer_queued_tick_can_reach_the_wire() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(200),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &VersionedFloor(1),
    );
    assert_eq!(frame.samples.len(), 4);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    let mut confirmed = None;
    flush_player_auth_inputs(
        &mut ticker,
        1,
        Some(evidence_context()),
        |identity, _packet| {
            confirmed = Some(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert!(ticker.acknowledge_physics_send(confirmed.unwrap()));

    let mut invalidated = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        2,
        Some(evidence_context()),
        |identity, _packet| {
            invalidated.push(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert_eq!(
        invalidated
            .iter()
            .map(|identity| identity.tick)
            .collect::<Vec<_>>(),
        [102, 103]
    );

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

    let mut overtaking_wire_ticks = Vec::new();
    let sent = flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |identity, _packet| {
            overtaking_wire_ticks.push(identity.tick);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert_eq!(
        sent, 0,
        "tick {overtaking_wire_ticks:?} overtook unresolved retries for ticks 102 and 103"
    );
    assert!(!ticker.accepting_physics_admissions());

    for identity in invalidated {
        assert!(ticker.resolve_cancelled_physics_send(identity, true));
    }
    assert!(ticker.accepting_physics_admissions());

    let mut ordered_wire_ticks = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |identity, _packet| {
            ordered_wire_ticks.push(identity.tick);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert_eq!(ordered_wire_ticks, [102, 103, 104]);
}

#[test]
fn newer_correction_drops_a_retry_that_is_now_the_corrected_tick() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
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

    let mut confirmed = None;
    flush_player_auth_inputs(
        &mut ticker,
        1,
        Some(evidence_context()),
        |identity, _packet| {
            confirmed = Some(identity);
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert!(ticker.acknowledge_physics_send(confirmed.unwrap()));

    let mut invalidated = Vec::new();
    flush_player_auth_inputs(
        &mut ticker,
        2,
        Some(evidence_context()),
        |identity, _packet| {
            invalidated.push(identity);
            Ok::<_, &str>(())
        },
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
    reconcile_candidate_physics_correction(
        &mut ticker,
        &mut physics,
        [0.5, 2.620_01, 0.0],
        102,
        true,
        PhysicsCorrectionMode::ReplayIfRetained,
        &VersionedFloor(1),
    )
    .unwrap();

    for identity in invalidated {
        assert!(ticker.resolve_cancelled_physics_send(identity, true));
    }
    assert_eq!(
        ticker
            .pending_snapshots()
            .iter()
            .map(|snapshot| snapshot.tick)
            .collect::<Vec<_>>(),
        [103],
        "a later correction owns tick 102, so only the still-future tick may be retried"
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
