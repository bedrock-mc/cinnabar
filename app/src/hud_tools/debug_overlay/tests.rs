use super::*;

#[test]
fn frame_timing_uses_elapsed_time_and_records_stalls() {
    let mut state = DebugOverlayState::default();
    assert!(!state.sample_frame(Duration::ZERO));
    for _ in 0..4 {
        assert!(!state.sample_frame(Duration::from_millis(25)));
    }
    assert!(state.sample_frame(Duration::from_millis(150)));
    assert!((state.timing.fps - 20.0).abs() < f64::EPSILON);
    assert!((state.timing.average_ms - 50.0).abs() < f64::EPSILON);
    assert!((state.timing.max_ms - 150.0).abs() < f64::EPSILON);
    assert_eq!(state.frames, 0);
    assert!(!state.sample_frame(Duration::from_millis(10)));
    assert_eq!(state.worst_frame, Duration::from_millis(10));
}

#[test]
fn negative_coordinates_floor_and_use_euclidean_chunk_positions() {
    let edge = meshing::biome_lattice::BIOME_QUERY_SIDE;
    let mut lines = Vec::new();
    spatial::append_coordinates(
        &mut lines,
        Vec3::new(-0.1, -0.1, edge as f32 + 0.1),
        Vec3::NEG_Z,
    );
    assert_eq!(lines[1], format!("Block: -1 -1 {edge}"));
    assert_eq!(
        lines[2],
        format!("Chunk: {} {} 0 in -1 -1 1", edge - 1, edge - 1)
    );
    assert!(lines[3].starts_with("Facing: north (-Z)"));
    assert_eq!(spatial::facing(Vec3::X).0, "east");
    assert_eq!(spatial::facing(Vec3::NEG_X).0, "west");
    assert_eq!(spatial::facing(Vec3::Z).0, "south");
}

#[test]
fn third_person_uses_player_feet_and_eye_instead_of_camera_pose() {
    let mut frame = LocalPlayerFrameCarrier::default();
    let feet = Vec3::new(3.0, 61.0, -7.0);
    let eye = feet + Vec3::Y * protocol::STANDING_PLAYER_EYE_HEIGHT;
    frame
        .publish(crate::local_player::LocalPlayerFrameSample {
            session_generation: 1,
            actor_session_id: 1,
            fifo_sequence: 1,
            physics_tick: 1,
            perspective: semantic_input::PerspectiveMode::ThirdPersonBack,
            world_collision_identity: sim::WorldCollisionIdentity {
                registry: sim::CollisionRegistryIdentity {
                    protocol: protocol::PROTOCOL_VERSION as u32,
                    id_space: sim::CollisionIdSpace::Sequential,
                    preg_sha256: [0; 32],
                },
                chunks: Box::new([]),
            },
            pose: Transform::from_xyz(30.0, 80.0, 42.0),
            feet,
            eye,
            rotation: Quat::IDENTITY,
        })
        .unwrap();
    let (position, origin, direction) = spatial::player_origins(&frame, None).unwrap();
    assert_eq!(position, feet);
    assert_eq!(origin, eye);
    assert_eq!(direction, Vec3::NEG_Z);
}

#[test]
fn target_states_are_readable_and_bounded() {
    let mut lines = Vec::new();
    spatial::append_block_states(
        &mut lines,
        r#"{"direction":2,"open_bit":false,"wood_type":"oak"}"#,
    );
    assert_eq!(lines, ["direction: 2", "open_bit: false", "wood_type: oak"]);
    let states: serde_json::Map<String, serde_json::Value> = (0..100)
        .map(|i| (format!("state_{i}"), serde_json::json!(i)))
        .collect();
    lines.clear();
    spatial::append_block_states(&mut lines, &serde_json::to_string(&states).unwrap());
    assert_eq!(lines.last().unwrap(), "... 90 more states");
    assert_eq!(lines.len(), 11);
}

#[test]
fn f3_toggles_rendered_overlay_and_hidden_frames_leave_presentation_untouched() {
    let mut app = App::new();
    app.insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(Time::<Real>::default())
        .insert_resource(ClientWorld::default())
        .insert_resource(LocalPlayerFrameCarrier::default())
        .insert_resource(client_ui::test_support::mini_engine_presentation());
    configure(&mut app);
    let player = player_state::PlayerState::new(1);
    let runtime = client_ui::ui_runtime::UiRuntime::new(1);
    let geometry = |app: &mut App| {
        app.world_mut()
            .resource_mut::<UiPresentationRuntime>()
            .build(
                &player,
                &runtime,
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap()
            .vertices
            .len()
    };
    app.update();
    let bare = geometry(&mut app);
    assert!(!app.world().resource::<DebugOverlayState>().visible);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    app.update();
    assert!(app.world().resource::<DebugOverlayState>().visible);
    assert!(geometry(&mut app) > bare);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::F3);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    app.update();
    assert!(!app.world().resource::<DebugOverlayState>().visible);
    assert_eq!(geometry(&mut app), bare);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut().clear_trackers();
    app.update();
    assert!(
        !app.world()
            .get_resource_ref::<UiPresentationRuntime>()
            .unwrap()
            .is_changed()
    );
}
