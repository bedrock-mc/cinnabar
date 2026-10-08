use super::*;

#[test]
fn bounded_flush_restores_the_exact_front_snapshot_when_transport_is_full() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ticker
        .enqueue_completed_physics(completed_sample(11, [1.0, 2.0, 3.0]))
        .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(12, [1.5, 2.0, 3.0]))
        .unwrap();
    let expected = ticker.peek_pending().unwrap().clone();

    let error = flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |_identity, _packet| Err("full"),
    )
    .unwrap_err();
    assert!(matches!(error, MovementSendError::Transport("full")));
    assert_eq!(ticker.pending_count(), 2);
    assert_eq!(ticker.peek_pending().unwrap(), &expected);
    assert_eq!(ticker.sent_physics_packet_count(), 0);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::TransportRestored
    );
    ticker.note_full_restore();
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::FullRestored
    );

    let mut sent_packets = 0;
    let mut identities = Vec::new();
    let sent = flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |identity, _packet| {
            identities.push(identity);
            sent_packets += 1;
            Ok::<_, &str>(())
        },
    )
    .unwrap();
    assert_eq!(sent, 2);
    assert_eq!(sent_packets, 2);
    assert_eq!(ticker.pending_count(), 2);
    assert_eq!(ticker.sent_physics_packet_count(), 0);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::SocketPending
    );
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(ticker.sent_physics_packet_count(), 2);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained
    );
}

#[test]
fn terminal_drain_stops_admissions_then_reconciles_a_healthy_final_write() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ticker
        .enqueue_completed_physics(completed_sample(41, [0.0, 64.0, 0.25]))
        .unwrap();
    let mut identities = Vec::new();
    assert_eq!(
        flush_player_auth_inputs(
            &mut ticker,
            1,
            Some(evidence_context()),
            |identity, _packet| {
                identities.push(identity);
                Ok::<_, &str>(())
            },
        )
        .unwrap(),
        1
    );
    ticker.begin_terminal_drain();
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::SocketPending
    );
    assert_eq!(
        ticker.enqueue_completed_physics(completed_sample(42, [0.0, 64.0, 0.5])),
        Err(PhysicsAuthorityFault::Unauthorized),
        "the acceptance boundary must stop completed ticks from entering the outbound FIFO"
    );
    assert_eq!(
        flush_player_auth_inputs(
            &mut ticker,
            1,
            Some(PhysicsTickEvidenceContext {
                perspective: semantic_input::PerspectiveMode::ThirdPersonFront,
                ..evidence_context()
            }),
            |_identity, _packet| Ok::<_, &str>(()),
        )
        .unwrap(),
        0
    );
    assert!(ticker.acknowledge_physics_send(identities[0]));
    assert_eq!(ticker.pending_count(), 0);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::Drained
    );
}

#[test]
fn surface_reanchor_keeps_an_already_admitted_send_nonterminal_until_transport_resolves_it() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ticker
        .enqueue_completed_physics(completed_sample(41, [0.0, 64.0, 0.25]))
        .unwrap();
    let mut admitted = None;
    assert_eq!(
        flush_player_auth_inputs(
            &mut ticker,
            1,
            Some(evidence_context()),
            |identity, _packet| {
                admitted = Some(identity);
                Ok::<_, &str>(())
            },
        ),
        Ok(1)
    );

    ticker.reanchor_surface_spawn(41, [8.0, 71.620_01, 9.0]);

    assert_eq!(
        ticker.pending_count(),
        1,
        "an admitted command remains transport-owned until cancellation or socket acknowledgement"
    );
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::SocketPending,
        "acceptance must not report a clean drain while the admitted command can still reach the socket"
    );
    assert!(
        ticker.acknowledge_physics_send(admitted.unwrap()),
        "a raced successful socket write must remain countable after the reanchor"
    );
    assert_eq!(ticker.sent_physics_packet_count(), 1);
}

#[test]
fn indeterminate_reanchor_cancellation_fails_physics_authority_closed() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ticker
        .enqueue_completed_physics(completed_sample(41, [0.0, 64.0, 0.25]))
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
    ticker.reanchor_surface_spawn(41, [8.0, 71.620_01, 9.0]);

    assert!(ticker.resolve_cancelled_physics_send(admitted.unwrap(), false));
    assert!(!ticker.physics_is_authorized());
    assert_eq!(
        ticker.pending_count(),
        0,
        "the indeterminate transport result is resolved even though it poisons authority"
    );
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::NotAuthoritative
    );
    assert_eq!(
        ticker.take_authority_fault().unwrap().fault,
        PhysicsAuthorityFault::IndeterminatePhysicsSend { tick: 41 }
    );
}

#[test]
fn socket_ack_publishes_the_immutable_admission_evidence_context() {
    let mut ordering_probe = MovementTicker::default();
    ordering_probe.reset(7, 40, [0.0; 3]);
    ordering_probe.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ordering_probe
        .enqueue_completed_physics(completed_sample(41, [0.0, 64.0, 0.25]))
        .unwrap();
    let admitted = evidence_context();
    let handoff = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = flush_player_auth_inputs(
            &mut ordering_probe,
            1,
            Some(admitted),
            |_identity, _packet| -> Result<(), &str> {
                panic!("send closure observed");
            },
        );
    }));
    assert!(handoff.is_err());
    assert_eq!(
        ordering_probe.pending_sends.len(),
        1,
        "the complete evidence record must be staged before the send closure can begin handoff"
    );
    assert_eq!(ordering_probe.pending_sends[0].evidence.context, admitted);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ticker
        .enqueue_completed_physics(completed_sample(41, [0.0, 64.0, 0.25]))
        .unwrap();
    let mut identity = None;
    flush_player_auth_inputs(&mut ticker, 1, Some(admitted), |send_identity, _packet| {
        identity = Some(send_identity);
        Ok::<_, &str>(())
    })
    .unwrap();
    let admitted_network_position = ticker.pending_sends[0].evidence.network_position;
    ticker.pending_sends[0].sample.snapshot.position = [9.0, 70.0, -4.0];
    assert_ne!(
        ticker.pending_sends[0].sample.snapshot.position, admitted_network_position,
        "the fixture must distinguish immutable admission evidence from mutable queued state"
    );

    assert!(ticker.acknowledge_physics_send(identity.unwrap()));

    let published = ticker.take_tick_evidence();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].context, admitted);
    assert_eq!(published[0].network_position, admitted_network_position);
}

#[test]
fn multi_tick_catch_up_exposes_only_successfully_sent_ticks_to_evidence() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let frame = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext {
            input_mode: PlayerInputMode::GamePad,
            ..PhysicsSampleContext::default()
        },
        &Floor,
    );
    assert_eq!(frame.samples.len(), 3);

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    for sample in frame.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }

    assert!(ticker.take_tick_evidence().is_empty());
    let mut identities = Vec::new();
    assert_eq!(
        flush_player_auth_inputs(
            &mut ticker,
            8,
            Some(evidence_context()),
            |identity, _packet| {
                identities.push(identity);
                Ok::<_, &str>(())
            }
        ),
        Ok(3)
    );
    assert!(ticker.take_tick_evidence().is_empty());
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );
    let evidence = ticker.take_tick_evidence();
    assert_eq!(
        evidence
            .iter()
            .map(|sample| sample.tick)
            .collect::<Vec<_>>(),
        [101, 102, 103]
    );
    assert!(
        evidence.iter().all(|sample| sample.session_generation == 7
            && sample.input_mode == PlayerInputMode::GamePad)
    );
    assert!(ticker.take_tick_evidence().is_empty());
}

#[test]
fn catch_up_evidence_cursor_does_not_repeat_restored_full_retry_ticks() {
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([0.0, 2.620_01, 0.0], 100, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.

    let catch_up = physics.advance_with_context(
        Duration::from_millis(150),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &Floor,
    );
    for sample in catch_up.samples {
        ticker.enqueue_completed_physics(sample).unwrap();
    }
    assert!(ticker.take_tick_evidence().is_empty());
    let full = flush_player_auth_inputs(
        &mut ticker,
        8,
        Some(evidence_context()),
        |_identity, _packet| Err("full"),
    )
    .unwrap_err();
    assert!(matches!(full, MovementSendError::Transport("full")));
    assert!(ticker.take_tick_evidence().is_empty());

    let next = physics.advance_with_context(
        Duration::from_millis(50),
        forward_physics_input(),
        PhysicsSampleContext::default(),
        &Floor,
    );
    assert_eq!(next.samples.len(), 1);
    ticker
        .enqueue_completed_physics(next.samples.into_iter().next().unwrap())
        .unwrap();

    let mut identities = Vec::new();
    assert_eq!(
        flush_player_auth_inputs(
            &mut ticker,
            8,
            Some(evidence_context()),
            |identity, _packet| {
                identities.push(identity);
                Ok::<_, &str>(())
            }
        ),
        Ok(4)
    );
    assert!(
        identities
            .into_iter()
            .all(|identity| ticker.acknowledge_physics_send(identity))
    );
    assert_eq!(
        ticker
            .take_tick_evidence()
            .iter()
            .map(|sample| sample.tick)
            .collect::<Vec<_>>(),
        [101, 102, 103, 104]
    );
    assert!(ticker.take_tick_evidence().is_empty());
}

#[test]
fn reconciliation_as_str_covers_every_variant() {
    let expected = [
        (
            MovementOutboxReconciliation::NotAuthoritative,
            "NotAuthoritative",
        ),
        (MovementOutboxReconciliation::Drained, "Drained"),
        (MovementOutboxReconciliation::SocketPending, "SocketPending"),
        (
            MovementOutboxReconciliation::BudgetDeferred,
            "BudgetDeferred",
        ),
        (
            MovementOutboxReconciliation::TransportRestored,
            "TransportRestored",
        ),
        (MovementOutboxReconciliation::FullRestored, "FullRestored"),
        (MovementOutboxReconciliation::RemoteClosed, "RemoteClosed"),
    ];
    for (reconciliation, name) in expected {
        assert_eq!(reconciliation.as_str(), name);
    }
}

#[test]
fn remote_close_latches_terminal_classification_across_teardown_and_flush() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    // The receive-side failure is observed while the authoritative session is
    // still live, exactly as the network pump delivers it.
    ticker.note_remote_session_close();
    ticker.deactivate();
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::RemoteClosed
    );

    // The deactivated unauthorized flush path must not clobber the
    // remote-close classification before terminal evidence observes it.
    flush_player_auth_inputs(
        &mut ticker,
        1,
        None,
        |_identity, _packet| -> Result<(), &str> { Ok(()) },
    )
    .unwrap();
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::RemoteClosed
    );
}

#[test]
fn remote_close_is_refused_for_free_camera_faulted_and_pre_session_tickers() {
    // FreeCamera sessions never latch.
    let mut free_camera = MovementTicker::default();
    free_camera.reset(7, 40, [0.0; 3]);
    free_camera.note_remote_session_close();
    assert_eq!(
        free_camera.outbox_reconciliation(),
        MovementOutboxReconciliation::NotAuthoritative
    );

    // An authority-faulted session already lost its physics source and must
    // keep its client-authority fault classification.
    let mut faulted = MovementTicker::default();
    faulted.reset(7, 40, [0.0; 3]);
    faulted.set_source(MovementSource::Physics);
    faulted.record_physics_fault(PhysicsAuthorityFault::Unauthorized);
    faulted.note_remote_session_close();
    assert_ne!(faulted.source(), MovementSource::Physics);
    assert_eq!(
        faulted.outbox_reconciliation(),
        MovementOutboxReconciliation::NotAuthoritative
    );

    // A pre-session ticker never latches either.
    let mut pre_session = MovementTicker::default();
    pre_session.note_remote_session_close();
    assert_eq!(
        pre_session.outbox_reconciliation(),
        MovementOutboxReconciliation::NotAuthoritative
    );
}

#[test]
fn remote_close_classification_does_not_survive_a_new_session_reset() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker.note_remote_session_close();
    ticker.reset(8, 40, [0.0; 3]);
    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::NotAuthoritative,
        "the replacement session must start from the default reconciliation"
    );
}
