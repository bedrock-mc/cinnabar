#[path = "../src/camera.rs"]
mod camera;

use std::{f32::consts::FRAC_PI_2, time::Duration};

use bevy::{
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow},
};
use camera::{AutoFly, FlyCamera, FlyCameraPlugin, PITCH_LIMIT};

#[test]
fn auto_fly_path_repeats_and_stays_within_the_loaded_radius() {
    assert_eq!(camera::auto_fly_offset(0.0), Vec3::ZERO);
    assert!(
        camera::auto_fly_offset(camera::AUTO_FLY_PERIOD_SECONDS).abs_diff_eq(Vec3::ZERO, 0.001)
    );

    for sample in 0..=2_000 {
        let seconds = camera::AUTO_FLY_PERIOD_SECONDS * sample as f32 / 2_000.0;
        let offset = camera::auto_fly_offset(seconds);
        assert!(offset.xz().length() <= camera::AUTO_FLY_MAX_HORIZONTAL_BLOCKS + 0.001);
        assert!(offset.y.abs() <= 8.001);
        assert!(offset.x.abs() < 16.0 * 16.0);
        assert!(offset.z.abs() < 16.0 * 16.0);
    }
}

#[test]
fn auto_fly_keeps_the_mutation_target_in_view() {
    let anchor = Vec3::new(100.5, 70.62, -30.5);
    let target = Vec3::new(104.5, 69.5, -30.5);
    let mut auto_fly = AutoFly::new(true);
    auto_fly.set_look_target(target);
    assert!(auto_fly.enabled());
    for sample in 0..=2_000 {
        let seconds = camera::AUTO_FLY_PERIOD_SECONDS * sample as f32 / 2_000.0;
        let position = anchor + camera::auto_fly_offset(seconds);
        assert!(position.distance(target) < 16.0 * 16.0);
        let rotation = camera::look_at_target(position, target);
        let forward = rotation * Vec3::NEG_Z;
        assert!(forward.dot((target - position).normalize()) > 0.999);
    }
}

fn axes_for(key: KeyCode) -> Vec3 {
    let mut keys = ButtonInput::default();
    keys.press(key);
    camera::movement_axes(&keys)
}

#[test]
fn direction_axes_map_wasd_space_and_both_shift_keys() {
    assert_eq!(axes_for(KeyCode::KeyW), Vec3::Z);
    assert_eq!(axes_for(KeyCode::KeyS), Vec3::NEG_Z);
    assert_eq!(axes_for(KeyCode::KeyA), Vec3::NEG_X);
    assert_eq!(axes_for(KeyCode::KeyD), Vec3::X);
    assert_eq!(axes_for(KeyCode::Space), Vec3::Y);
    assert_eq!(axes_for(KeyCode::ShiftLeft), Vec3::NEG_Y);
    assert_eq!(axes_for(KeyCode::ShiftRight), Vec3::NEG_Y);

    let mut keys = ButtonInput::default();
    keys.press(KeyCode::KeyW);
    keys.press(KeyCode::KeyS);
    keys.press(KeyCode::Space);
    keys.press(KeyCode::ShiftLeft);
    assert_eq!(camera::movement_axes(&keys), Vec3::ZERO);
}

#[test]
fn mouse_look_clamps_pitch_and_applies_pixel_delta_without_time() {
    assert_eq!(PITCH_LIMIT, FRAC_PI_2 - 0.01);

    let (yaw, pitch) = camera::look_angles(0.5, 0.25, Vec2::new(10.0, -20.0), Vec2::splat(0.01));
    assert!((yaw - 0.4).abs() < 1.0e-6);
    assert!((pitch - 0.45).abs() < 1.0e-6);

    let (_, up) = camera::look_angles(0.0, 0.0, Vec2::new(0.0, -1_000_000.0), Vec2::ONE);
    let (_, down) = camera::look_angles(0.0, 0.0, Vec2::new(0.0, 1_000_000.0), Vec2::ONE);
    assert_eq!(up, PITCH_LIMIT);
    assert_eq!(down, -PITCH_LIMIT);
}

#[test]
fn input_requires_focus_and_a_locked_hidden_cursor() {
    let focused = Window {
        focused: true,
        ..default()
    };
    let unfocused = Window {
        focused: false,
        ..default()
    };
    let captured = CursorOptions {
        grab_mode: CursorGrabMode::Locked,
        visible: false,
        ..default()
    };
    let visible = CursorOptions {
        grab_mode: CursorGrabMode::Locked,
        visible: true,
        ..default()
    };
    let released = CursorOptions::default();

    assert!(camera::input_is_active(&focused, &captured));
    assert!(!camera::input_is_active(&unfocused, &captured));
    assert!(!camera::input_is_active(&focused, &visible));
    assert!(!camera::input_is_active(&focused, &released));
}

fn capture_test_app(
    focused: bool,
    grab_mode: CursorGrabMode,
    visible: bool,
    auto_fly: bool,
) -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(AutoFly::new(auto_fly))
        .add_systems(Update, camera::update_cursor_capture);

    let entity = app
        .world_mut()
        .spawn((
            Window {
                focused,
                ..default()
            },
            CursorOptions {
                grab_mode,
                visible,
                ..default()
            },
            PrimaryWindow,
        ))
        .id();
    (app, entity)
}

#[test]
fn focus_loss_releases_cursor_clears_input_and_beats_auto_capture() {
    let (mut app, window) = capture_test_app(false, CursorGrabMode::Locked, false, true);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::new(15.0, -4.0);

    app.update();

    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .get_pressed()
            .next()
            .is_none()
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .get_pressed()
            .next()
            .is_none()
    );
    assert_eq!(
        app.world().resource::<AccumulatedMouseMotion>().delta,
        Vec2::ZERO
    );
}

#[test]
fn escape_releases_and_clears_pressed_movement_before_recapture() {
    let (mut app, window) = capture_test_app(true, CursorGrabMode::Locked, false, false);
    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::KeyW);
        keys.press(KeyCode::Escape);
    }

    app.update();

    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .get_pressed()
            .next()
            .is_none()
    );
}

#[test]
fn left_click_recaptures_with_locked_invisible_cursor() {
    let (mut app, window) = capture_test_app(true, CursorGrabMode::None, true, false);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    app.update();

    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
}

#[test]
fn plugin_spawns_camera_and_auto_fly_uses_delta_seconds() {
    let mut app = App::new();
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::new(true));
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        CursorOptions::default(),
        PrimaryWindow,
    ));

    app.update();
    let tonemapping = app
        .world_mut()
        .query_filtered::<&Tonemapping, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    assert_eq!(*tonemapping, Tonemapping::None);
    let start = app
        .world_mut()
        .query_filtered::<&Transform, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap()
        .translation;
    assert!(app.world().resource::<AutoFly>().enabled());

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(0.5));
    app.update();

    let end = app
        .world_mut()
        .query_filtered::<&Transform, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap()
        .translation;
    let expected = start + camera::auto_fly_offset(0.5);
    assert!(end.abs_diff_eq(expected, 1.0e-4));
}

#[test]
fn plugin_spawns_camera_with_120_degree_vertical_fov() {
    let mut app = App::new();
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default());
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        CursorOptions::default(),
        PrimaryWindow,
    ));

    app.update();
    let projection = app
        .world_mut()
        .query_filtered::<&Projection, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    let Projection::Perspective(perspective) = projection else {
        panic!("fly camera projection is not perspective");
    };
    let expected = 120.0_f32.to_radians();
    assert!(
        (perspective.fov - expected).abs() <= 1.0e-6,
        "vertical FOV = {} degrees, want 120",
        perspective.fov.to_degrees()
    );
}

#[test]
fn auto_fly_moves_and_rotates_while_unfocused_with_a_released_cursor() {
    let target = Vec3::new(4.5, 70.0, -3.5);
    let mut app = App::new();
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::new(true));
    app.world_mut()
        .resource_mut::<AutoFly>()
        .set_look_target(target);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: false,
                ..default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();

    app.update();
    let start = app
        .world_mut()
        .query_filtered::<&Transform, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap()
        .translation;
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(0.5));
    app.update();

    let end = *app
        .world_mut()
        .query_filtered::<&Transform, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    let expected = start + camera::auto_fly_offset(0.5);
    assert!(
        end.translation.abs_diff_eq(expected, 1.0e-4),
        "auto-fly stayed at {:?} instead of advancing to {expected:?}",
        end.translation,
    );
    assert!(
        (end.rotation * Vec3::NEG_Z).dot((target - end.translation).normalize()) > 0.999,
        "auto-fly did not keep the target in view while unfocused"
    );
}
