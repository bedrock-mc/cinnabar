use client_presentation::camera::{CameraSettingsAuthority, ServerCameraView, motion_blur};
use developer_control::camera::{Easing, Keyframe};
use render::motion_blur::CameraMotionBlur;
use std::time::Duration;
use {super::*, client_presentation::camera::FlyCamera};

fn frame(t: f32, x: f32) -> Keyframe {
    Keyframe {
        t,
        position: [x, 64.0, 0.0],
        yaw: x * 10.0,
        pitch: 0.0,
        fov: None,
        easing: None,
    }
}

fn app(path: CameraPath) -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<client_presentation::local_player::LocalViewPose>()
        .init_resource::<ServerCameraView>()
        .init_resource::<CameraSettingsAuthority>()
        .add_systems(
            Update,
            motion_blur::apply_camera_motion_blur
                .after(ClientFrameSet::Camera)
                .before(ClientFrameSet::Interaction),
        );
    configure(&mut app);
    let mut settings = ui::UserSettings::default();
    settings.video.motion_blur = ui::MotionBlurQuality::High;
    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .replace(1, &settings)
        .unwrap();
    let camera = app
        .world_mut()
        .spawn((FlyCamera::default(), Camera3d::default()))
        .id();
    start(app.world_mut(), path).unwrap();
    app.update();
    (app, camera)
}

fn advance(app: &mut App, seconds: f32) {
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(seconds));
    app.update();
}

fn blur(app: &App, camera: Entity) -> CameraMotionBlur {
    *app.world().get::<CameraMotionBlur>(camera).unwrap()
}

#[test]
fn scripted_camera_step_resets_motion_blur_on_the_rendered_cut_frame() {
    let mut path = CameraPath {
        keyframes: vec![frame(0.0, 0.0), frame(1.0, 1.0), frame(2.0, 2.0)],
        easing: Easing::Linear,
        looping: false,
        hide_hand: false,
    };
    path.keyframes[1].easing = Some(Easing::Step);
    let (mut app, camera) = app(path);
    advance(&mut app, 0.75);
    let before = blur(&app, camera);
    assert!(before.exposure_seconds > 0.0);
    assert_eq!(
        app.world().get::<Transform>(camera).unwrap().translation.x,
        0.0
    );
    let player = *app
        .world()
        .resource::<client_presentation::local_player::LocalViewPose>();
    advance(&mut app, 0.5);
    let cut = blur(&app, camera);
    assert_eq!(
        app.world().get::<Transform>(camera).unwrap().translation.x,
        1.25
    );
    assert_eq!(cut.exposure_seconds, 0.0);
    assert_ne!(cut.reset_epoch, before.reset_epoch);
    let mut expected_player = player;
    expected_player.reanchor_camera();
    assert_eq!(
        *app.world()
            .resource::<client_presentation::local_player::LocalViewPose>(),
        expected_player
    );
    advance(&mut app, 0.125);
    assert!(blur(&app, camera).exposure_seconds > 0.0);
    assert_eq!(blur(&app, camera).reset_epoch, cut.reset_epoch);
}

#[test]
fn scripted_camera_loop_resets_motion_blur_only_for_discontinuous_wraps() {
    for continuous in [false, true] {
        let path = CameraPath {
            keyframes: vec![
                frame(0.0, 0.0),
                frame(1.0, 1.0),
                frame(2.0, if continuous { 0.0 } else { 2.0 }),
            ],
            easing: Easing::Linear,
            looping: true,
            hide_hand: false,
        };
        let (mut app, camera) = app(path);
        advance(&mut app, 1.875);
        let before = blur(&app, camera);
        assert!(before.exposure_seconds > 0.0);
        advance(&mut app, 0.25);
        let wrapped = blur(&app, camera);
        assert_eq!(
            app.world().get::<Transform>(camera).unwrap().translation.x,
            0.125
        );
        assert_eq!(wrapped.exposure_seconds == 0.0, !continuous);
        assert_eq!(wrapped.reset_epoch == before.reset_epoch, continuous);
        advance(&mut app, 0.125);
        assert!(blur(&app, camera).exposure_seconds > 0.0);
        assert_eq!(blur(&app, camera).reset_epoch, wrapped.reset_epoch);
    }
}

#[test]
fn scripted_fov_survives_gameplay_updates_and_release_restores_the_player_fov() {
    use bevy::window::{CursorOptions, PrimaryWindow, WindowResolution};
    let mut app = App::new();
    crate::app::configure_client_frame_schedule(&mut app);
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    app.init_resource::<Time>()
        .add_plugins(crate::camera::FlyCameraPlugin::default());
    configure(&mut app);
    app.world_mut().spawn((
        Window {
            resolution: WindowResolution::new(1280, 720),
            ..default()
        },
        CursorOptions::default(),
        PrimaryWindow,
    ));
    app.update();
    let mut path = CameraPath {
        keyframes: vec![frame(0.0, 0.0), frame(2.0, 2.0)],
        easing: Easing::Linear,
        looping: false,
        hide_hand: false,
    };
    path.keyframes[0].fov = Some(40.0);
    path.keyframes[1].fov = Some(80.0);
    start(app.world_mut(), path).unwrap();
    app.update();
    let read_fov = |app: &mut App| {
        let projection = app
            .world_mut()
            .query_filtered::<&Projection, With<FlyCamera>>()
            .single(app.world())
            .unwrap();
        let Projection::Perspective(perspective) = projection else {
            panic!("expected a perspective camera");
        };
        perspective.fov.to_degrees()
    };
    assert!((read_fov(&mut app) - 40.0).abs() < 0.001);
    advance(&mut app, 1.0);
    assert!((read_fov(&mut app) - 60.0).abs() < 0.001);
    release(app.world_mut()).unwrap();
    app.update();
    let expected = app
        .world()
        .resource::<CameraSettingsAuthority>()
        .horizontal_fov_degrees()
        * app
            .world()
            .resource::<client_presentation::camera::CameraFovState>()
            .modifier();
    assert!((read_fov(&mut app) - expected).abs() < 0.001);
}

#[test]
fn scripted_pose_survives_late_actor_camera_effects() {
    use client_presentation::camera::{FirstPersonHandMotion, ViewEffect, actor_effects};

    /// Exercises the production effect composer at the actor presentation boundary.
    fn actor_effect(
        settings: Res<CameraSettingsAuthority>,
        hand: Res<FirstPersonHandMotion>,
        server: Res<ServerCameraView>,
        cameras: Query<&mut Transform, With<FlyCamera>>,
    ) {
        actor_effects::apply_actor_damage_camera_rotation(
            settings, hand, server, None, 0.0, cameras,
        );
    }

    let path = CameraPath {
        keyframes: vec![frame(0.0, 0.0), frame(2.0, 2.0)],
        easing: Easing::Linear,
        looping: false,
        hide_hand: false,
    };
    let (mut app, camera) = app(path);
    app.insert_resource(FirstPersonHandMotion {
        bob: ViewEffect {
            translation: Vec3::new(0.1, -0.05, 0.0),
            roll_radians: 0.1,
            pitch_radians: 0.05,
        },
        ..Default::default()
    });
    crate::app::configure_client_frame_schedule(&mut app);
    app.add_systems(
        Update,
        actor_effect
            .in_set(actor_effects::ActorCameraEffects)
            .after(ClientFrameSet::ActorPreparation)
            .before(ClientFrameSet::UiPreparation),
    );
    advance(&mut app, 1.0);
    let pose = app.world().get::<Transform>(camera).unwrap();
    assert!(
        pose.translation
            .abs_diff_eq(Vec3::new(1.0, 64.0, 0.0), 1e-6)
    );
    assert!(
        pose.rotation
            .abs_diff_eq(bedrock_camera_rotation(10.0, 0.0), 1e-6)
    );
}
