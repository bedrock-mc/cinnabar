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
fn start_game_anchor_tracks_fifo_move_correction_and_dimension_before_surface_resolution() {
    let correction = PlayerMovementCorrectionEvent {
        position: [-1351.25, 92.62, 1647.75],
        delta: [0.0; 3],
        pitch: 0.0,
        yaw: 0.0,
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
    };
    let dimension_control = CommittedControlEvent::ChangeDimension {
        change,
        resolved: client_world::ResolvedServerPosition {
            position: change.position,
            surface_anchor: None,
        },
    };
    let mut view = crate::local_player::LocalViewPose::default();
    let mut camera_settings = crate::camera::CameraSettingsAuthority::default();
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
fn relative_socket_dir_falls_back_to_the_development_project_root() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-mcbe-socket-resolution-{}-{unique}",
        std::process::id()
    ));
    let current_dir = root.join("launcher");
    let executable = root.join("project/target/debug/bedrock-client.exe");
    let expected = root.join("project/.local/run");
    std::fs::create_dir_all(&current_dir).unwrap();
    std::fs::create_dir_all(&expected).unwrap();
    let endpoint_name = if cfg!(windows) {
        "game.addr"
    } else {
        "game.sock"
    };
    std::fs::write(expected.join(endpoint_name), "endpoint").unwrap();

    assert_eq!(
        resolve_socket_dir_from(
            std::path::Path::new(".local/run"),
            &current_dir,
            &executable,
        ),
        expected
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bridge_endpoint_exists_only_for_the_platform_marker() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "rust-mcbe-platform-endpoint-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let (expected, wrong) = if cfg!(windows) {
        ("game.addr", "game.sock")
    } else {
        ("game.sock", "game.addr")
    };

    std::fs::write(root.join(wrong), "wrong platform").unwrap();
    assert!(!bridge_endpoint_exists(&root));
    preflight_bridge_endpoint(&root)
        .expect_err("a wrong-platform marker must not pass startup preflight");

    std::fs::write(root.join(expected), "expected platform").unwrap();
    assert!(bridge_endpoint_exists(&root));

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn bridge_endpoint_path_selects_the_platform_marker() {
    let socket_dir = Path::new("custom/run");
    let expected_name = if cfg!(windows) {
        "game.addr"
    } else {
        "game.sock"
    };

    assert_eq!(
        bridge_endpoint_path(socket_dir),
        socket_dir.join(expected_name)
    );
}

#[test]
fn missing_bridge_endpoint_returns_an_actionable_error() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let socket_dir = std::env::temp_dir().join(format!(
        "rust-mcbe-missing-core-{}-{unique}",
        std::process::id()
    ));
    let expected_endpoint = bridge_endpoint_path(&socket_dir);

    let error = preflight_bridge_endpoint(&socket_dir)
        .expect_err("a missing platform endpoint must stop startup")
        .to_string();

    assert!(error.contains("Go core is not running"));
    assert!(error.contains(&expected_endpoint.display().to_string()));
    assert!(error.contains("make core UPSTREAM=host:port"));
}

#[test]
fn existing_platform_endpoint_passes_preflight() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let socket_dir = std::env::temp_dir().join(format!(
        "rust-mcbe-running-core-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&socket_dir).unwrap();
    std::fs::write(bridge_endpoint_path(&socket_dir), "endpoint").unwrap();

    preflight_bridge_endpoint(&socket_dir)
        .expect("an existing platform endpoint must continue startup");

    let _ = std::fs::remove_dir_all(socket_dir);
}
