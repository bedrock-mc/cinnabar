use super::*;
use client_presentation::camera::{CameraSettingsAuthority, ServerCameraView, motion_blur};
use developer_control::camera::{Easing, Keyframe};
use render::motion_blur::CameraMotionBlur;
use std::time::Duration;

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
        .init_resource::<crate::local_player::LocalViewPose>()
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
    let player = *app.world().resource::<crate::local_player::LocalViewPose>();
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
        *app.world().resource::<crate::local_player::LocalViewPose>(),
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
