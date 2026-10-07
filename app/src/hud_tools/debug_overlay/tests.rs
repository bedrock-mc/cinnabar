use super::*;

#[test]
fn frame_timing_uses_elapsed_time_and_records_stalls() {
    let mut state = DebugOverlayState::default();
    assert!(!state.sample_frame(Duration::ZERO));
    for _ in 0..4 {
        assert!(!state.sample_frame(Duration::from_millis(100)));
    }
    assert!(state.sample_frame(Duration::from_millis(600)));
    assert!((state.timing.fps - 5.0).abs() < f64::EPSILON);
    assert!((state.timing.average_ms - 200.0).abs() < f64::EPSILON);
    assert!((state.timing.max_ms - 600.0).abs() < f64::EPSILON);
    assert_eq!(state.frames, 0);
    assert!(!state.sample_frame(Duration::from_millis(10)));
    assert_eq!(state.worst_frame, Duration::from_millis(10));
}

#[test]
fn negative_coordinates_floor_and_use_euclidean_chunk_positions() {
    let edge = meshing::biome_lattice::BIOME_QUERY_SIDE;
    let mut spare = Vec::new();
    let mut lines = Column::new(Vec::new(), &mut spare);
    spatial::append_coordinates(
        &mut lines,
        Vec3::new(-0.1, -0.1, edge as f32 + 0.1),
        Vec3::NEG_Z,
    );
    let lines = lines.finish();
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
    let mut spare = Vec::new();
    let mut lines = Column::new(Vec::new(), &mut spare);
    spatial::append_block_states(
        &mut lines,
        r#"{"direction":2,"open_bit":false,"wood_type":"oak"}"#,
    );
    let lines = lines.finish();
    assert_eq!(lines, ["direction: 2", "open_bit: false", "wood_type: oak"]);
    let states: serde_json::Map<String, serde_json::Value> = (0..100)
        .map(|i| (format!("state_{i}"), serde_json::json!(i)))
        .collect();
    let mut lines = Column::new(lines, &mut spare);
    spatial::append_block_states(&mut lines, &serde_json::to_string(&states).unwrap());
    let lines = lines.finish();
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

#[test]
fn visible_frames_between_diagnostic_ticks_leave_presentation_untouched() {
    use bevy::ecs::system::{IntoSystem, System};
    let mut world = World::new();
    let mut time = Time::<Real>::default();
    time.advance_by(Duration::from_millis(1));
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(time);
    world.insert_resource(ClientWorld::default());
    world.insert_resource(LocalPlayerFrameCarrier::default());
    world.insert_resource(DebugOverlayState::default());
    world.insert_resource(client_ui::test_support::mini_engine_presentation());
    let mut system = IntoSystem::into_system(publish_debug_overlay);
    system.initialize(&mut world);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    system.run((), &mut world).unwrap();
    world.resource_mut::<ButtonInput<KeyCode>>().clear();
    world.clear_trackers();
    world
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(1));
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<DebugOverlayState>().gathers, 1);
    assert!(
        !world
            .get_resource_ref::<UiPresentationRuntime>()
            .unwrap()
            .is_changed()
    );
}

#[test]
fn diagnostics_sample_once_per_tick_and_skip_hidden_frames() {
    let mut app = App::new();
    app.insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(Time::<Real>::default())
        .insert_resource(ClientWorld::default())
        .insert_resource(LocalPlayerFrameCarrier::default())
        .insert_resource(client_ui::test_support::mini_engine_presentation());
    configure(&mut app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    for _ in 0..1_000 {
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(Duration::from_millis(1));
        app.update();
    }
    assert_eq!(
        app.world().resource::<DebugOverlayState>().gathers,
        u64::from(world::TICKS_PER_SECOND) as usize + 1
    );
    assert_eq!(
        app.world().resource::<DebugOverlayState>().timing.fps,
        1_000.0
    );
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::F3);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    let gathered = app.world().resource::<DebugOverlayState>().gathers;
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(5));
    app.update();
    assert_eq!(
        app.world().resource::<DebugOverlayState>().gathers,
        gathered
    );
}

#[test]
fn diagnostic_formatting_and_unchanged_target_states_reuse_storage() {
    use crate::tests::alloc_count::thread_allocations;
    let mut states = spatial::BlockStates::default();
    let mut retained = DebugLines::default();
    let mut spare = [Vec::new(), Vec::new()];
    for _ in 0..2 {
        let mut lines = Lines::new(retained, &mut spare);
        spatial::append_coordinates(&mut lines.left, Vec3::new(1.25, 61.0, -8.0), Vec3::NEG_Z);
        states.append(
            &mut lines.right,
            r#"{"direction":2,"open_bit":false,"wood_type":"oak"}"#,
        );
        retained = lines.finish();
    }
    let before = thread_allocations();
    for _ in 0..100 {
        let mut lines = Lines::new(retained, &mut spare);
        spatial::append_coordinates(&mut lines.left, Vec3::new(1.25, 61.0, -8.0), Vec3::NEG_Z);
        states.append(
            &mut lines.right,
            r#"{"direction":2,"open_bit":false,"wood_type":"oak"}"#,
        );
        retained = lines.finish();
    }
    let allocations = thread_allocations() - before;
    assert_eq!(allocations, 0);
    assert_eq!(
        retained.right,
        ["direction: 2", "open_bit: false", "wood_type: oak"]
    );
}

#[test]
fn diagnostic_rows_reuse_tail_buffers_after_row_counts_change() {
    use crate::tests::alloc_count::thread_allocations;
    let mut retained = DebugLines::default();
    let mut spare = [Vec::new(), Vec::new()];
    for _ in 0..2 {
        for count in [10, 2, 10] {
            let mut lines = Lines::new(retained, &mut spare);
            for row in 0..count {
                lines.right.push(format_args!("Property {row}: true"));
            }
            retained = lines.finish();
        }
    }
    let before = thread_allocations();
    for count in [2, 10, 2, 10] {
        let mut lines = Lines::new(retained, &mut spare);
        for row in 0..count {
            lines.right.push(format_args!("Property {row}: true"));
        }
        retained = lines.finish();
    }
    let allocations = thread_allocations() - before;
    assert_eq!(allocations, 0);
    assert_eq!(retained.right.len(), 10);
}

#[test]
fn warmed_diagnostic_publication_allocates_nothing_when_values_stay_equal() {
    use crate::tests::alloc_count::thread_allocations;
    use bevy::ecs::system::{IntoSystem, System};

    let mut world = World::new();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(Time::<Real>::default());
    world.insert_resource(ClientWorld::default());
    world.insert_resource(LocalPlayerFrameCarrier::default());
    world.insert_resource(DebugOverlayState::default());
    world.insert_resource(client_ui::test_support::mini_engine_presentation());
    let mut system = IntoSystem::into_system(publish_debug_overlay);
    system.initialize(&mut world);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    system.run((), &mut world).unwrap();
    world.resource_mut::<ButtonInput<KeyCode>>().clear();
    world
        .resource_mut::<Time<Real>>()
        .advance_by(world::TICK_DURATION);
    system.run((), &mut world).unwrap();
    world.clear_trackers();
    let before = thread_allocations();
    for _ in 0..10 {
        world
            .resource_mut::<Time<Real>>()
            .advance_by(world::TICK_DURATION);
        system.run((), &mut world).unwrap();
    }
    let allocations = thread_allocations() - before;
    assert_eq!(allocations, 0);
    assert!(
        !world
            .get_resource_ref::<UiPresentationRuntime>()
            .unwrap()
            .is_changed()
    );
}

#[test]
fn gpu_diagnostics_rank_passes_without_allocating_or_discarding_zero_samples() {
    use crate::tests::alloc_count::thread_allocations;
    let passes = [
        (RuntimeStage::GpuFrame, Duration::from_millis(20)),
        (RuntimeStage::GpuUi, Duration::ZERO),
        (RuntimeStage::GpuHand, Duration::from_millis(2)),
        (RuntimeStage::GpuOpaque, Duration::from_millis(8)),
        (RuntimeStage::GpuTransparent, Duration::from_millis(5)),
    ];
    let before = thread_allocations();
    let top = top_gpu_passes(passes.into_iter());
    let allocations = thread_allocations() - before;
    assert_eq!(allocations, 0);
    assert_eq!(top, [Some(passes[3]), Some(passes[4]), Some(passes[2])]);
    assert_eq!(
        top_gpu_passes([passes[0], passes[1]].into_iter()),
        [Some(passes[1]), None, None]
    );
}

#[test]
fn covering_screens_skip_expensive_diagnostic_gathers() {
    use client_ui::menu::{MenuScreen, MenuView};
    use client_ui::ui_runtime::{UiRuntime, presentation::LoadingStage};
    let mut app = App::new();
    app.insert_resource(ButtonInput::<KeyCode>::default())
        .insert_resource(Time::<Real>::default())
        .insert_resource(ClientWorld::default())
        .insert_resource(LocalPlayerFrameCarrier::default())
        .insert_resource(PlayerRuntime::new(1))
        .insert_resource(UiRuntime::new(1))
        .insert_resource(client_ui::test_support::mini_engine_presentation());
    configure(&mut app);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    let gathered = app.world().resource::<DebugOverlayState>().gathers;
    assert_eq!(gathered, 1);
    let mut menu = MenuView::new(true, "Player".into());
    menu.screen = MenuScreen::Pause;
    menu.over_world = true;
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .set_menu_view(Some(menu));
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(world::TICK_DURATION);
    app.update();
    assert_eq!(
        app.world().resource::<DebugOverlayState>().gathers,
        gathered
    );
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .set_menu_view(None);
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .set_loading_stage(Some(LoadingStage::Connecting));
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(world::TICK_DURATION);
    app.update();
    assert_eq!(
        app.world().resource::<DebugOverlayState>().gathers,
        gathered
    );
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .set_loading_stage(None);
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(world::TICK_DURATION);
    app.update();
    assert_eq!(
        app.world().resource::<DebugOverlayState>().gathers,
        gathered + 1
    );
}

#[test]
fn first_unchanged_diagnostic_tick_allocates_nothing() {
    use bevy::ecs::system::{IntoSystem, System};
    let mut world = World::new();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(Time::<Real>::default());
    world.insert_resource(ClientWorld::default());
    world.insert_resource(LocalPlayerFrameCarrier::default());
    world.insert_resource(DebugOverlayState::default());
    world.insert_resource(client_ui::test_support::mini_engine_presentation());
    let mut system = IntoSystem::into_system(publish_debug_overlay);
    system.initialize(&mut world);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    system.run((), &mut world).unwrap();
    world.resource_mut::<ButtonInput<KeyCode>>().clear();
    world.clear_trackers();
    world
        .resource_mut::<Time<Real>>()
        .advance_by(world::TICK_DURATION);
    let before = crate::tests::alloc_count::thread_allocations();
    system.run((), &mut world).unwrap();
    let allocations = crate::tests::alloc_count::thread_allocations() - before;
    assert_eq!(allocations, 0);
    assert!(
        !world
            .get_resource_ref::<UiPresentationRuntime>()
            .unwrap()
            .is_changed()
    );
}

#[test]
fn ui_frame_counters_wait_for_the_frame_statistics_window() {
    use bevy::ecs::system::{IntoSystem, System};
    let mut world = World::new();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(Time::<Real>::default());
    world.insert_resource(ClientWorld::default());
    world.insert_resource(LocalPlayerFrameCarrier::default());
    world.insert_resource(DebugOverlayState::default());
    world.insert_resource(client_ui::test_support::mini_engine_presentation());
    world.insert_resource(UiRenderStatsResource::default());
    let mut system = IntoSystem::into_system(publish_debug_overlay);
    system.initialize(&mut world);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    system.run((), &mut world).unwrap();
    world.resource_mut::<ButtonInput<KeyCode>>().clear();
    world.clear_trackers();
    world.resource::<UiRenderStatsResource>().update(|stats| {
        stats.draw_calls = 17;
        stats.uploaded_vertices = 99;
    });
    world
        .resource_mut::<Time<Real>>()
        .advance_by(world::TICK_DURATION);
    system.run((), &mut world).unwrap();
    assert!(
        !world
            .get_resource_ref::<UiPresentationRuntime>()
            .unwrap()
            .is_changed()
    );
    world
        .resource_mut::<Time<Real>>()
        .advance_by(FRAME_TIMING_WINDOW - world::TICK_DURATION);
    system.run((), &mut world).unwrap();
    assert!(
        world
            .resource::<DebugOverlayState>()
            .ui_line
            .starts_with("UI: 17 draws / 99 vertices")
    );
}

#[test]
fn first_eligible_publication_samples_frame_counters() {
    use bevy::ecs::system::{IntoSystem, System};
    use client_ui::{
        menu::{MenuScreen, MenuView},
        ui_runtime::UiRuntime,
    };
    let mut world = World::new();
    world.insert_resource(ButtonInput::<KeyCode>::default());
    world.insert_resource(Time::<Real>::default());
    world.insert_resource(ClientWorld::default());
    world.insert_resource(LocalPlayerFrameCarrier::default());
    world.insert_resource(DebugOverlayState::default());
    world.insert_resource(PlayerRuntime::new(1));
    world.insert_resource(UiRuntime::new(1));
    world.insert_resource(UiRenderStatsResource::default());
    world.insert_resource(client_ui::test_support::mini_engine_presentation());
    let mut menu = MenuView::new(true, "Player".into());
    menu.screen = MenuScreen::Pause;
    menu.over_world = true;
    world
        .resource_mut::<UiPresentationRuntime>()
        .set_menu_view(Some(menu));
    let mut system = IntoSystem::into_system(publish_debug_overlay);
    system.initialize(&mut world);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F3);
    system.run((), &mut world).unwrap();
    assert_eq!(world.resource::<DebugOverlayState>().gathers, 0);
    world.resource_mut::<ButtonInput<KeyCode>>().clear();
    world
        .resource::<UiRenderStatsResource>()
        .update(|stats| stats.draw_calls = 17);
    world
        .resource_mut::<UiPresentationRuntime>()
        .set_menu_view(None);
    world
        .resource_mut::<Time<Real>>()
        .advance_by(world::TICK_DURATION);
    system.run((), &mut world).unwrap();
    let state = world.resource::<DebugOverlayState>();
    assert_eq!(state.gathers, 1);
    assert!(state.ui_line.starts_with("UI: 17 draws"));
}
