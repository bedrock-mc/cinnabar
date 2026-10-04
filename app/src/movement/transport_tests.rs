use super::*;

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
fn remote_closed_candidate_terminal_emits_no_outbox_violation() {
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
        0,
        MovementOutboxReconciliation::RemoteClosed,
    );
    assert_eq!(markers.len(), 2);
    assert!(
        !markers
            .iter()
            .any(|marker| marker.contains("terminal_outbox_not_drained"))
    );
    assert!(
        !markers
            .iter()
            .any(|marker| marker.starts_with("RUST_MCBE_PHASE3_VIOLATION="))
    );
    assert!(
        markers
            .iter()
            .any(|marker| marker.contains("\"outbox_reconciliation\":\"RemoteClosed\""))
    );
}

#[test]
fn local_stop_with_undrained_authoritative_state_still_violates() {
    let mut ticker = MovementTicker::default();
    ticker.reset(7, 40, [0.0; 3]);
    ticker.set_source(MovementSource::Physics);
    // Transport-focused fixture: the provisional spawn-settle window is
    // orthogonal to what this test asserts.
    ticker
        .enqueue_completed_physics(completed_sample(41, [0.0, 64.0, 0.25]))
        .unwrap();
    assert_eq!(
        ticker.pending_count(),
        1,
        "the fixture must hold undrained authoritative work when the local stop arrives"
    );

    // A local stop tears the ticker down without a remote-close origin.
    ticker.deactivate();

    assert_eq!(
        ticker.outbox_reconciliation(),
        MovementOutboxReconciliation::NotAuthoritative,
        "a locally stopped candidate session must not claim a remote close"
    );
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
        0,
        0,
        0,
        MovementOutboxReconciliation::NotAuthoritative,
    );
    assert!(
        markers
            .iter()
            .any(|marker| marker.contains("terminal_outbox_not_drained"))
    );
}

#[test]
fn free_camera_remote_closed_reconciliation_still_fails_the_terminal_gate() {
    let identity = crate::runtime::phase3_evidence::Phase3EvidenceIdentity::new(
        "0123456789abcdef0123456789abcdef01234567",
        crate::args::Phase3Target::Bds,
        7,
        [0x11; 32],
        [0x22; 32],
        false,
    )
    .unwrap();
    let mut emitter = crate::runtime::phase3_evidence::Phase3EvidenceEmitter::default();
    let markers = emitter.observe_terminal(
        identity,
        MovementSource::FreeCamera,
        0,
        0,
        0,
        MovementOutboxReconciliation::RemoteClosed,
    );
    assert!(
        markers
            .iter()
            .any(|marker| marker.contains("terminal_outbox_not_drained")),
        "a FreeCamera terminal presenting RemoteClosed reconciliation must stay a violation"
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
    assert!(ticker.acknowledge_physics_send(identity.unwrap()));
    let published = ticker.take_tick_evidence();
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
