#[test]
fn perspective_changes_leave_physics_history_and_outbox_unchanged() {
    let physics = physics_after_one_second(60);
    let expected_state = physics.state().unwrap().clone();
    let expected_history_len = physics.history_len();

    let mut ticker = MovementTicker::default();
    ticker.reset(7, 100, [0.0, 2.620_01, 0.0]);
    ticker.set_source(MovementSource::Physics);
    ticker
        .enqueue_completed_physics(completed_sample(101, [0.0, 2.620_01, -0.5]))
        .unwrap();
    ticker
        .enqueue_completed_physics(completed_sample(102, [0.0, 2.620_01, -1.0]))
        .unwrap();
    let expected_outbox = ticker.pending_snapshots();

    let mut camera = CameraSettingsAuthority::default();
    let mut settings = UserSettings::default();
    settings.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonFront;
    camera.replace(1, &settings).unwrap();

    assert_eq!(physics.state(), Some(&expected_state));
    assert_eq!(physics.history_len(), expected_history_len);
    assert_eq!(ticker.pending_snapshots(), expected_outbox);
}

#[test]
fn unavailable_collision_does_not_overflow_after_a_long_block() {
    let world = DeferredCollisionWorld::default();
    let mut physics = LocalPhysicsController::default();
    physics.reanchor_network_position([4.0, 65.620_01, 6.0], 7, true);
    let mut ticker = MovementTicker::default();
    ticker.reset(1, 7, [4.0, 65.620_01, 6.0]);
    ticker.set_source(MovementSource::Physics);
    let mut evidence = Phase3EvidenceEmitter::default();
    let before = physics.state().unwrap().clone();
    let before_eye = physics.render_eye_position();
    let mut violation_markers = Vec::new();

    let frame = physics.advance(
        Duration::from_millis(600),
        forward_physics_input(),
        &world,
    );
    assert_eq!(frame.due_ticks, 12);
    assert_eq!(frame.completed_ticks, 0);
    assert_eq!(frame.dropped_ticks, 0);
    assert!(frame.blocked.is_some());
    if let Some(fault) = physics_authority_fault_for_frame(&frame) {
        ticker.record_physics_fault(fault);
        if let Some(record) = ticker.take_authority_fault() {
            violation_markers.extend(evidence.observe_authority_fault(record));
        }
    }

    assert!(ticker.physics_is_authorized());
    assert!(ticker.take_authority_fault().is_none());
    assert!(violation_markers.is_empty());
    assert!(evidence.take_violation_marker().is_empty());
    assert_eq!(physics.state(), Some(&before));
    assert_eq!(physics.render_eye_position(), before_eye);
    assert_eq!(physics.history_len(), 0);
    assert_eq!(physics.dropped_tick_count(), 0);
}
