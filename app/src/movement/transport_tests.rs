use super::*;

#[test]
fn bounded_flush_restores_the_exact_front_snapshot_when_transport_is_full() {
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 10, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
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
fn socket_ack_publishes_the_immutable_admission_evidence_context() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(41, [0.0, 64.0, 0.25]))
        .unwrap();
    let admitted = evidence_context();
    let mut identity = None;
    flush_player_auth_inputs(&mut ticker, 1, Some(admitted), |send_identity, _packet| {
        identity = Some(send_identity);
        Ok::<_, &str>(())
    })
    .unwrap();

    let changed_render_frame = PhysicsTickEvidenceContext {
        fifo_sequence: 99,
        pose_generation: 212,
        dimension: 1,
        perspective: semantic_input::PerspectiveMode::ThirdPersonFront,
        camera_blocked: true,
        local_avatar_visible: true,
        look_delta: [-8.0, 4.0],
        outbox_depth: 0,
        ..admitted
    };
    assert_ne!(changed_render_frame, admitted);
    assert!(ticker.acknowledge_physics_send(identity.unwrap()));

    let published = ticker.take_tick_evidence();
    assert_eq!(published.len(), 1);
    assert_eq!(published[0].context, admitted);
    let mut emitter = crate::runtime::phase3_evidence::Phase3EvidenceEmitter::default();
    let markers = emitter.observe_completed_ticks(&published);
    let frame: serde_json::Value =
        serde_json::from_str(markers[0].strip_prefix("RUST_MCBE_PHASE3_FRAME=").unwrap()).unwrap();
    assert_eq!(frame["fifo_sequence"], admitted.fifo_sequence);
    assert_eq!(frame["pose_generation"], admitted.pose_generation);
    assert_eq!(frame["dimension"], admitted.dimension);
    assert_eq!(frame["input_mode"], "KeyboardMouse");
    assert_eq!(frame["perspective"], "FirstPerson");
    assert_eq!(frame["camera_blocked"], false);
    assert_eq!(frame["local_avatar_visible"], false);
    assert_eq!(frame["look_delta"], serde_json::json!([0.25, -0.5]));
}

#[test]
fn socket_pending_is_runtime_emittable_but_never_a_candidate_terminal_pass() {
    let identity = crate::runtime::phase3_evidence::Phase3EvidenceIdentity::new(
        "0123456789abcdef0123456789abcdef01234567",
        crate::args::Phase3Target::Bds,
        7,
        [0x11; 32],
        [0x22; 32],
        true,
    )
    .unwrap();
    let mut emitter = crate::runtime::phase3_evidence::Phase3EvidenceEmitter::default();
    let markers = emitter.observe_terminal(
        identity,
        MovementSource::Physics,
        3,
        0,
        1,
        MovementOutboxReconciliation::SocketPending,
    );
    assert!(
        markers
            .iter()
            .any(|marker| marker.contains("terminal_outbox_not_drained"))
    );
    assert!(markers.iter().any(|marker| {
        marker.contains("\"pending_outbox_depth\":1")
            && marker.contains("\"outbox_reconciliation\":\"SocketPending\"")
    }));
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
