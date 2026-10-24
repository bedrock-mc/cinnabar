#[test]
fn world_ready_requires_two_exact_gpu_presented_frames_bound_to_the_raw_cohort() {
    let started = Instant::now();
    let snapshot = settled_world_snapshot();
    let key = SubChunkKey::new(0, 64, 65, 65);
    let mut proposed = proposed_render_expectation(started, [(key, 7)]);
    proposed.source_cohort = None;
    let mut settler = WorldReadySettler::default();

    let expectation = settler
        .reconcile_presentation(snapshot, proposed.clone(), started)
        .expect("exact raw cohort should arm a downstream presentation gate");
    assert!(!settler.has_stable_presentation(snapshot));
    assert!(!settler.observe_presented_frame(presented_acknowledgement(
        &expectation,
        10,
        Duration::from_millis(1),
        Duration::from_millis(2),
    )));
    assert!(settler.observe_presented_frame(presented_acknowledgement(
        &expectation,
        11,
        Duration::from_millis(3),
        Duration::from_millis(4),
    )));
    assert!(settler.has_stable_presentation(snapshot));
    assert!(settler.observe_presented_frame(presented_acknowledgement(
        &expectation,
        12,
        Duration::from_millis(5),
        Duration::from_millis(6),
    )));
    let mut invalid = presented_acknowledgement(
        &expectation,
        13,
        Duration::from_millis(7),
        Duration::from_millis(8),
    );
    invalid.foreign_instances = 1;
    assert!(!settler.observe_presented_frame(invalid));
    assert!(!settler.has_stable_presentation(snapshot));
    assert!(!settler.observe_presented_frame(presented_acknowledgement(
        &expectation,
        14,
        Duration::from_millis(9),
        Duration::from_millis(10),
    )));
    assert!(settler.observe_presented_frame(presented_acknowledgement(
        &expectation,
        15,
        Duration::from_millis(11),
        Duration::from_millis(12),
    )));

    let mut transport_refilled = snapshot;
    transport_refilled.work.network_events = client_session::WORLD_EVENT_CAPACITY;
    let same_expectation = settler
        .reconcile_presentation(
            transport_refilled,
            proposed.clone(),
            started + Duration::from_millis(13),
        )
        .expect("transport depth does not replace an exact presentation candidate");
    assert_eq!(
        same_expectation.view_generation,
        expectation.view_generation
    );
    assert!(settler.has_stable_presentation(transport_refilled));

    let mut changed = snapshot;
    changed.cohort.as_mut().unwrap().required_hash ^= 1;
    assert!(
        settler
            .reconcile_presentation(changed, proposed, started + Duration::from_secs(1))
            .is_some()
    );
    assert!(!settler.has_stable_presentation(changed));
}

#[test]
fn world_ready_binds_announced_columns_without_rewriting_raw_radius_identity() {
    let started = Instant::now();
    let mut snapshot = settled_world_snapshot();
    let raw_cohort = RenderViewCohort::new(0, [65, 65], 2);
    let status = snapshot.cohort.as_mut().expect("settled status");
    status.target.radius = raw_cohort.radius;
    status.committed = Some(status.target);
    status.expected = 2;
    status.loaded_target = 2;
    let inside = SubChunkKey::new(0, 65, 0, 65);
    let announced_outside = SubChunkKey::new(0, 69, 0, 65);
    let mut proposed = TargetRenderExpectation {
        cohort: raw_cohort,
        source_cohort: None,
        target_columns: Some(Arc::from([inside.chunk(), announced_outside.chunk()])),
        target_keys: None,
        manifest: Arc::from([(inside, 7), (announced_outside, 8)]),
        view_generation: 0,
        render_ready_at: started,
    };
    let mut settler = WorldReadySettler::default();

    let expectation = settler
        .reconcile_presentation(snapshot, proposed.clone(), started)
        .expect("exact status should arm the announced-column presentation target");
    assert_eq!(expectation.cohort, raw_cohort);
    assert_eq!(expectation.target_columns, proposed.target_columns);

    let mut missing_announced = presented_acknowledgement(
        &expectation,
        10,
        Duration::from_millis(1),
        Duration::from_millis(2),
    );
    missing_announced.allocation_manifest = Arc::from([(inside, 7)]);
    missing_announced.visible_allocation_manifest =
        Arc::clone(&missing_announced.allocation_manifest);
    missing_announced.drawn_manifest = Arc::clone(&missing_announced.allocation_manifest);
    missing_announced.missing_target_instances = 1;
    assert!(
        !settler.observe_presented_frame(missing_announced),
        "missing announced terrain must not certify readiness"
    );
    assert!(!settler.has_stable_presentation(snapshot));

    assert!(!settler.observe_presented_frame(presented_acknowledgement(
        &expectation,
        11,
        Duration::from_millis(3),
        Duration::from_millis(4),
    )));
    assert!(settler.observe_presented_frame(presented_acknowledgement(
        &expectation,
        12,
        Duration::from_millis(5),
        Duration::from_millis(6),
    )));

    proposed.target_columns = Some(Arc::from([
        inside.chunk(),
        announced_outside.chunk(),
        world::ChunkKey::new(0, 70, 65),
    ]));
    let expanded = settler
        .reconcile_presentation(snapshot, proposed, started + Duration::from_millis(7))
        .expect("expanded announced membership should arm a new candidate");
    assert_eq!(expanded.cohort, raw_cohort);
    assert_ne!(expanded.view_generation, expectation.view_generation);
    assert!(
        !settler.has_stable_presentation(snapshot),
        "a previous presented pair survived an explicit membership change"
    );
}

#[test]
fn presentation_waits_until_the_captured_readiness_ingress_fence_is_consumed() {
    let started = Instant::now();
    let mut snapshot = settled_world_snapshot();
    snapshot.work.readiness_events = 1;
    snapshot.readiness_produced = 8;
    snapshot.readiness_consumed = 7;
    let key = SubChunkKey::new(0, 64, 65, 65);
    let proposed = proposed_render_expectation(started, [(key, 7)]);
    let mut settler = WorldReadySettler::default();

    assert!(
        settler
            .reconcile_presentation(snapshot, proposed.clone(), started)
            .is_none(),
        "the exact candidate must wait for readiness work produced before its fence"
    );

    snapshot.work.readiness_events = 0;
    snapshot.readiness_consumed = 8;
    let expectation = settler
        .reconcile_presentation(
            snapshot,
            proposed.clone(),
            started + Duration::from_millis(1),
        )
        .expect("consuming the captured fence should arm presentation");

    snapshot.work.readiness_events = 1;
    snapshot.readiness_produced = 9;
    let same_expectation = settler
        .reconcile_presentation(snapshot, proposed, started + Duration::from_millis(2))
        .expect("traffic produced after the explicit fence belongs to a later boundary");
    assert_eq!(
        same_expectation.view_generation,
        expectation.view_generation
    );
}

#[test]
fn start_game_anchor_tracks_fifo_move_correction_and_dimension_before_surface_resolution() {
    let correction = PlayerMovementCorrectionEvent {
        position: [-1351.25, 92.62, 1647.75],
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
        subject: protocol::MovementCorrectionSubject::Player,
        on_ground: false,
        tick: 55,
    };
    let correction_control = CommittedControlEvent::PlayerMovementCorrection {
        sequence: 7,
        correction,
        resolved: client_world::ResolvedServerPosition {
            position: correction.position,
            surface_anchor: None,
        },
    };
    let mut acceptance = AcceptanceRun::new(Some(900), None, false, false);
    // This is the StartGame bootstrap anchor; production must not freeze it
    // while later FIFO-authoritative controls are still committing.
    acceptance.set_mutation_surface_anchor([0, 0]);
    let movement = protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [-1340.25, 94.0, 1659.75],
        ..Default::default()
    };
    let move_control = CommittedControlEvent::MovePlayer {
        sequence: 8,
        movement,
        resolved: client_world::ResolvedServerPosition {
            position: movement.position,
            surface_anchor: None,
        },
        source_cohort: None,
    };
    let change = protocol::ChangeDimensionEvent {
        dimension: 1,
        position: [240.75, 82.0, -17.25],
        ..Default::default()
    };
    let dimension_control = CommittedControlEvent::ChangeDimension {
        sequence: 9,
        change,
        resolved: client_world::ResolvedServerPosition {
            position: change.position,
            surface_anchor: None,
        },
    };
    let mut view = client_presentation::local_player::LocalViewPose::default();
    let mut camera_settings = client_presentation::camera::CameraSettingsAuthority::default();
    let mut pending_surface_spawn = None;
    for control in [correction_control, move_control, dimension_control] {
        assert!(refresh_mutation_anchor_from_committed_control(
            &mut acceptance,
            &control,
        ));
        apply_committed_control(
            control,
            &mut view,
            &mut camera_settings,
            &mut pending_surface_spawn,
        );
    }
    assert_eq!(acceptance.mutation_surface_anchor(), Some([240, -18]));
    assert_eq!(view.eye_translation(), Vec3::from_array(change.position));

    let coordinate = deterministic_mutation_coordinate([240.75, 80.62, -17.25], [240, -18]);
    acceptance.set_mutation_coordinate(coordinate);
    assert!(!refresh_mutation_anchor_from_committed_control(
        &mut acceptance,
        &CommittedControlEvent::PlayerMovementCorrection {
            sequence: 10,
            correction,
            resolved: client_world::ResolvedServerPosition {
                position: correction.position,
                surface_anchor: None,
            },
        },
    ));
    assert_eq!(acceptance.source_mutation_coordinate(), Some(coordinate));
}

#[test]
fn model_gallery_camera_marker_only_reports_committed_move_player() {
    let movement = protocol::MovePlayerEvent {
        runtime_id: 1,
        position: [27.0, 87.62, 43.0],
        pitch: 12.5,
        yaw: -45.0,
        ..Default::default()
    };
    let control = CommittedControlEvent::MovePlayer {
        sequence: 19,
        movement,
        resolved: client_world::ResolvedServerPosition {
            position: movement.position,
            surface_anchor: None,
        },
        source_cohort: None,
    };
    let expected_marker = format!(
        "{CAMERA_COMMITTED} sequence=19 position=27.00000,87.62000,43.00000 yaw=-45.00000 pitch=12.50000"
    );
    assert_eq!(
        model_gallery_camera_committed_marker(true, &control).as_deref(),
        Some(expected_marker.as_str())
    );
    assert!(model_gallery_camera_committed_marker(false, &control).is_none());

    let correction = protocol::PlayerMovementCorrectionEvent {
        position: movement.position,
        delta: [0.0; 3],
        pitch: movement.pitch,
        yaw: movement.yaw,
        subject: protocol::MovementCorrectionSubject::Player,
        on_ground: false,
        tick: 1,
    };
    assert!(
        model_gallery_camera_committed_marker(
            true,
            &CommittedControlEvent::PlayerMovementCorrection {
                sequence: 20,
                correction,
                resolved: client_world::ResolvedServerPosition {
                    position: correction.position,
                    surface_anchor: None,
                },
            },
        )
        .is_none()
    );
}






#[test]
fn a_launcher_session_failure_returns_to_the_menu_instead_of_exiting() {
    let mut menu = client_shell::menu::MenuRuntime::new(true, 2, "Tester".to_owned());
    menu.show_connecting();

    assert!(
        menu.absorb_session_failure("bridge connection failed: Connection refused (os error 61)"),
        "the launcher must take ownership of a failed join"
    );

    assert!(
        menu.is_visible(),
        "a failed join must land back on the menu"
    );
    let error = menu
        .view()
        .disconnect_message
        .expect("the launcher must explain the failure");
    let words = launcher::menu::disconnect::describe(&error);
    assert_eq!(words.title.0, "connect.failed", "got {words:?}");
}

#[test]
fn a_direct_address_session_failure_still_exits_the_process() {
    let mut menu = client_shell::menu::MenuRuntime::new(false, 2, "Tester".to_owned());
    menu.show_connecting();

    assert!(
        !menu.absorb_session_failure("network session failed: bridge closed"),
        "--address has no launcher to fall back to and must still exit"
    );
    assert!(fatal_runtime_exit("network session failed: bridge closed").is_some());
}

// The raw chain, however long, never reaches the screen: it reads as vanilla's line.
#[test]
fn a_long_session_failure_shows_vanilla_text() {
    let mut menu = client_shell::menu::MenuRuntime::new(true, 2, "Tester".to_owned());

    assert!(menu.absorb_session_failure(&"boundless detail ".repeat(40)));

    let error = menu.view().disconnect_message.unwrap();
    assert_eq!(
        launcher::menu::disconnect::describe(&error).body,
        launcher::menu::disconnect::DisconnectBody::Key("disconnectionScreen.noReason")
    );
}
