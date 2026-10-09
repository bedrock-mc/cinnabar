use super::*;
use bevy::camera::CameraProjection;

/// Builds the app camera adapter with a primary window of the requested size.
fn camera_app(width: u32, height: u32) -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default());
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: WindowResolution::new(width, height),
                focused: true,
                ..default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.update();
    (app, window)
}

/// Reads the live fly-camera projection after the scheduled presentation update.
fn projection(app: &mut App) -> PerspectiveProjection {
    let projection = app
        .world_mut()
        .query_filtered::<&Projection, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    let Projection::Perspective(perspective) = projection else {
        panic!("fly camera projection is not perspective");
    };
    perspective.clone()
}

#[test]
fn native_full_viewport_fov_sets_vertical_angle_and_aspect_only_scales_horizontal_axis() {
    // Vanilla FOV uses normalized viewport fractions; bx::mtxProjRh
    // stores cot(FOV/2) on Y and cot(FOV/2)/aspect on X.
    for degrees in [30.0_f32, 60.0, 90.0, 110.0, 120.0] {
        for aspect in [4.0 / 3.0, 16.0 / 9.0, 21.0 / 9.0, 9.0 / 16.0] {
            let projection = PerspectiveProjection {
                fov: camera::projection_fov_radians(degrees),
                aspect_ratio: aspect,
                ..default()
            };
            let clip = projection.get_clip_from_view();
            let y_scale = 1.0 / (degrees.to_radians() * 0.5).tan();
            assert!((projection.fov - degrees.to_radians()).abs() < 1.0e-6);
            assert!((clip.y_axis.y - y_scale).abs() < 1.0e-6);
            assert!((clip.x_axis.x - y_scale / aspect).abs() < 1.0e-6);
        }
    }
}

#[test]
fn replacing_user_settings_preserves_110_degree_vertical_fov() {
    let (mut app, _) = camera_app(1600, 900);
    let mut settings = UserSettings::default();
    settings.video.horizontal_fov_degrees = 110.0;
    settings.video.fov_effects_scale = 0.0;
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(settings);
    app.update();

    let perspective = projection(&mut app);
    assert_eq!(
        app.world()
            .resource::<CameraSettingsAuthority>()
            .generation(),
        1
    );
    assert!((perspective.fov - 110.0_f32.to_radians()).abs() < 1.0e-6);
    let horizontal = 2.0 * (1.0 / perspective.get_clip_from_view().x_axis.x).atan();
    assert!((horizontal.to_degrees() - 137.0).abs() < 0.2);
}

#[test]
fn projection_fov_is_finite_and_bounded_for_bad_inputs() {
    for degrees in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 999.0] {
        let vertical = camera::projection_fov_radians(degrees);
        assert!(vertical.is_finite());
        assert!(vertical > 0.0 && vertical < std::f32::consts::PI);
    }
}

#[test]
fn plugin_spawns_camera_with_configured_default_vertical_fov() {
    let (mut app, _) = camera_app(1600, 900);
    let expected = UserSettings::default().video.horizontal_fov_degrees;
    assert!((projection(&mut app).fov - expected.to_radians()).abs() <= 1.0e-6);
}

#[test]
fn resizing_the_window_preserves_vertical_fov_and_updates_horizontal_projection() {
    let (mut app, window) = camera_app(1600, 900);
    let before = projection(&mut app);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1200, 900);
    app.update();
    let after = projection(&mut app);

    assert_eq!(after.fov, before.fov);
    assert!((before.aspect_ratio - 16.0 / 9.0).abs() < 1.0e-6);
    assert!((after.aspect_ratio - 4.0 / 3.0).abs() < 1.0e-6);
    let before_clip = before.get_clip_from_view();
    let after_clip = after.get_clip_from_view();
    assert_eq!(before_clip.y_axis.y, after_clip.y_axis.y);
    assert!(after_clip.x_axis.x > before_clip.x_axis.x);
}

#[test]
fn zero_height_window_preserves_fov_with_a_finite_projection() {
    let (mut app, window) = camera_app(1600, 900);
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1600, 0);
    app.update();

    let perspective = projection(&mut app);
    let expected_fov = UserSettings::default()
        .video
        .horizontal_fov_degrees
        .to_radians();
    assert!((perspective.fov - expected_fov).abs() < 1.0e-6);
    assert!(perspective.aspect_ratio.is_finite() && perspective.aspect_ratio > 0.0);
    assert!(perspective.get_clip_from_view().is_finite());
}

#[test]
fn a_rig_committed_during_camera_input_changes_the_fov_in_the_same_frame() {
    fn commit_rig(mut settings: ResMut<CameraSettingsAuthority>) {
        settings.set_rig(Some(camera::CameraRig {
            offset: Vec3::new(0.5, 0.5, 3.0),
            roll_radians: 0.0,
            fov_delta_degrees: 10.0,
        }));
    }
    let (mut app, _) = camera_app(1280, 720);
    let base = app
        .world()
        .resource::<CameraSettingsAuthority>()
        .horizontal_fov_degrees();
    app.add_systems(Update, commit_rig.in_set(camera::FlyCameraUpdateSet));
    app.update();
    let expected = camera::projection_fov_radians(base + 10.0);
    assert!((projection(&mut app).fov - expected).abs() < 1.0e-6);
}

#[test]
fn death_fov_samples_this_frames_actor_clock_and_clears_on_recovery_and_session_retirement() {
    use crate::runtime::{network::ActorFramePartialTick, world::ClientWorld};
    use chunk_pipeline::WorldStream;
    use protocol::{
        ActorAttribute, ActorEvent, ActorKind, ActorSpawnEvent, HudEvent, UiEvent, WorldBootstrap,
        WorldEvent,
    };
    use std::sync::Arc;

    /// Advances the actor clock at the production actor preparation boundary.
    fn advance_death_clock(
        mut world: ResMut<ClientWorld>,
        mut partial: ResMut<ActorFramePartialTick>,
    ) {
        if let Some(stream) = &mut world.stream {
            stream.advance_actor_interpolation_frame(125);
        }
        partial.0 = 0.25;
    }

    let (mut app, _) = camera_app(1280, 720);
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::Spawn(ActorSpawnEvent {
                dimension: 0,
                unique_id: 1,
                runtime_id: 1,
                kind: ActorKind::Player {
                    uuid: [0; 16],
                    username: "Player".into(),
                },
                position: [0.0; 3],
                velocity: [0.0; 3],
                pitch: 0.0,
                yaw: 0.0,
                head_yaw: 0.0,
                body_yaw: 0.0,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([ActorAttribute {
                    name: "minecraft:health".into(),
                    min: 0.0,
                    max: client_world::DEFAULT_PLAYER_HEALTH,
                    current: 0.0,
                    default: None,
                    modifiers: Arc::from([]),
                }]),
                properties: Arc::from([]),
                links: Arc::from([]),
            })),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 0 })),
        )
        .unwrap();
    assert!(stream.authority().actor(1).unwrap().status.dead);
    app.insert_resource(ClientWorld {
        stream: Some(stream),
        ..Default::default()
    })
    .init_resource::<ActorFramePartialTick>()
    .add_systems(
        Update,
        advance_death_clock.in_set(ClientFrameSet::ActorPreparation),
    );
    configure_client_frame_schedule(&mut app);
    let mut settings = UserSettings::default();
    settings.video.fov_effects_scale = 0.0;
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(settings);

    app.update();
    assert!((projection(&mut app).fov.to_degrees() - 84.0).abs() < 1e-4);
    assert_eq!(
        app.world()
            .resource::<camera::CameraFovInputs>()
            .death_ticks,
        Some(125.25)
    );

    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            3,
            WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 7 })),
        )
        .unwrap();
    app.update();
    let base = app
        .world()
        .resource::<CameraSettingsAuthority>()
        .horizontal_fov_degrees();
    assert!((projection(&mut app).fov.to_degrees() - base).abs() < 1e-4);
    assert_eq!(
        app.world()
            .resource::<camera::CameraFovInputs>()
            .death_ticks,
        None
    );

    app.world_mut()
        .resource_mut::<ClientWorld>()
        .stream
        .as_mut()
        .unwrap()
        .submit(
            4,
            WorldEvent::Ui(UiEvent::Hud(HudEvent::Health { health: 0 })),
        )
        .unwrap();
    app.update();
    assert!((projection(&mut app).fov.to_degrees() - 84.0).abs() < 1e-4);
    app.world_mut().resource_mut::<ClientWorld>().stream = None;
    app.update();
    assert!((projection(&mut app).fov.to_degrees() - base).abs() < 1e-4);
    assert_eq!(
        app.world()
            .resource::<camera::CameraFovInputs>()
            .death_ticks,
        None
    );
}
