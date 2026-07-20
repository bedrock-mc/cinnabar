use std::time::Duration;

use crate::app::{ClientFrameSet, configure_client_frame_schedule};
use crate::camera::{
    self, AutoFly, CameraSettingsAuthority, CameraSettingsError, FlyCamera, FlyCameraPlugin,
    PITCH_LIMIT,
};
use crate::local_player::LocalViewPose;
use crate::semantic_controls::{
    collect_raw_input, finalize_semantic_input_after_ui_authority, route_semantic_input,
};
use crate::settings_runtime::RuntimeSettings;
use bevy::{
    anti_alias::fxaa::Fxaa,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowResolution},
};
use semantic_input::PerspectiveMode;
use sim::{Aabb, CollisionQuery, CollisionWorld, Vec3 as SimVec3, WorldQueryError};
use ui::UserSettings;
use world::ChunkKey;

#[derive(Default)]
struct CameraCollisionFixture {
    boxes: Vec<Aabb>,
    unavailable: bool,
}

impl CollisionWorld for CameraCollisionFixture {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        if self.unavailable {
            return Err(WorldQueryError::UnloadedChunk(ChunkKey::new(0, 0, 0)));
        }
        Ok(CollisionQuery::synthetic(self.boxes.clone()))
    }
}

#[test]
fn third_person_boom_sweeps_a_radius_point_and_stops_before_solid_geometry() {
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let world = CameraCollisionFixture {
        boxes: vec![Aabb::new(
            SimVec3::new(-1.0, 1.0, 2.0),
            SimVec3::new(1.0, 3.0, 3.0),
        )],
        unavailable: false,
    };

    let pose = camera::collision_safe_perspective_pose(
        subject,
        Quat::IDENTITY,
        PerspectiveMode::ThirdPersonBack,
        &world,
    );

    assert!(pose.translation.abs_diff_eq(
        Vec3::new(
            0.0,
            2.0,
            1.8 - camera::THIRD_PERSON_COLLISION_EPSILON_BLOCKS,
        ),
        1.0e-5,
    ));
}

#[test]
fn third_person_boom_handles_compound_wall_corner_ceiling_floor_transitions_before_hit() {
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let diagonal = std::f32::consts::FRAC_1_SQRT_2;
    let cases = [
        (
            "wall",
            Quat::IDENTITY,
            vec![Aabb::new(
                SimVec3::new(-1.0, 1.0, 2.0),
                SimVec3::new(1.0, 3.0, 3.0),
            )],
            1.8,
        ),
        (
            "corner",
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
            vec![
                Aabb::new(SimVec3::new(1.5, 1.0, -1.0), SimVec3::new(2.0, 3.0, 4.0)),
                Aabb::new(SimVec3::new(-1.0, 1.0, 1.5), SimVec3::new(4.0, 3.0, 2.0)),
            ],
            1.3 / diagonal,
        ),
        (
            "ceiling",
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
            vec![Aabb::new(
                SimVec3::new(-1.0, 4.0, -1.0),
                SimVec3::new(1.0, 4.5, 5.0),
            )],
            1.8 / diagonal,
        ),
        (
            "floor",
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_4),
            vec![Aabb::new(
                SimVec3::new(-1.0, 0.5, -1.0),
                SimVec3::new(1.0, 1.0, 5.0),
            )],
            0.8 / diagonal,
        ),
    ];

    for (label, rotation, boxes, contact_distance) in cases {
        let blocked_world = CameraCollisionFixture {
            boxes,
            unavailable: false,
        };
        let blocked = camera::collision_safe_perspective_pose(
            subject,
            rotation,
            PerspectiveMode::ThirdPersonBack,
            &blocked_world,
        );
        let blocked_distance = blocked.translation.distance(subject);
        let expected = contact_distance - camera::THIRD_PERSON_COLLISION_EPSILON_BLOCKS;
        assert!(
            (blocked_distance - expected).abs() <= 1.0e-4,
            "{label} boom distance {blocked_distance} did not stop at pre-hit {expected}",
        );

        let clear = camera::collision_safe_perspective_pose(
            subject,
            rotation,
            PerspectiveMode::ThirdPersonBack,
            &CameraCollisionFixture::default(),
        );
        assert!(
            (clear.translation.distance(subject) - camera::THIRD_PERSON_RADIUS_BLOCKS).abs()
                <= 1.0e-5,
            "{label} boom did not restore after collision space cleared",
        );
        let blocked_again = camera::collision_safe_perspective_pose(
            subject,
            rotation,
            PerspectiveMode::ThirdPersonBack,
            &blocked_world,
        );
        assert!(
            blocked_again
                .translation
                .abs_diff_eq(blocked.translation, 1.0e-5)
        );
    }
}

#[test]
fn third_person_boom_falls_back_to_the_subject_when_collision_space_is_unloaded() {
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let world = CameraCollisionFixture {
        unavailable: true,
        ..default()
    };

    let pose = camera::collision_safe_perspective_pose(
        subject,
        Quat::IDENTITY,
        PerspectiveMode::ThirdPersonBack,
        &world,
    );

    assert_eq!(pose.translation, subject);
}

#[test]
fn missing_world_stream_falls_back_to_eye_in_third_person() {
    let eye = Vec3::new(4.0, 70.0, -3.0);
    let rotation = Quat::from_rotation_y(0.4);
    let pose =
        camera::unavailable_world_perspective_pose(eye, rotation, PerspectiveMode::ThirdPersonBack);
    assert_eq!(pose.translation, eye);
    assert!(pose.rotation.abs_diff_eq(rotation, 1.0e-6));
}

#[test]
fn front_camera_uses_positive_horizontal_look_instead_of_pitched_forward() {
    let subject = Vec3::new(4.0, 20.0, -3.0);
    let pitched = Quat::from_euler(EulerRot::YXZ, 0.0, 45.0_f32.to_radians(), 0.0);

    let pose = camera::perspective_pose(subject, pitched, PerspectiveMode::ThirdPersonFront);

    assert!(
        pose.translation
            .abs_diff_eq(Vec3::new(4.0, 20.0, -7.0), 1.0e-5)
    );
    assert!((pose.rotation * Vec3::NEG_Z).dot(Vec3::Z) > 0.999);
}

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
    assert_eq!(PITCH_LIMIT, 89.9_f32.to_radians());

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
    let msaa = app
        .world_mut()
        .query_filtered::<&Msaa, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    assert_eq!(*msaa, Msaa::Off);
    let fxaa = app
        .world_mut()
        .query_filtered::<&Fxaa, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    assert!(fxaa.enabled);
    let start = app.world().resource::<LocalViewPose>().eye_translation();
    assert!(app.world().resource::<AutoFly>().enabled());

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(0.5));
    app.update();

    let end = app.world().resource::<LocalViewPose>().eye_translation();
    let expected = start + camera::auto_fly_offset(0.5);
    assert!(end.abs_diff_eq(expected, 1.0e-4));
}

#[test]
fn horizontal_fov_converts_to_aspect_correct_vertical_fov() {
    let horizontal = 90.0_f32.to_radians();
    let sixteen_nine = camera::horizontal_fov_to_vertical(horizontal, 16.0 / 9.0);
    let four_three = camera::horizontal_fov_to_vertical(horizontal, 4.0 / 3.0);
    assert!(four_three > sixteen_nine);
}

#[test]
fn perspective_cycle_matches_bedrock_settings_order() {
    assert_eq!(
        camera::next_perspective(PerspectiveMode::FirstPerson),
        PerspectiveMode::ThirdPersonBack
    );
    assert_eq!(
        camera::next_perspective(PerspectiveMode::ThirdPersonBack),
        PerspectiveMode::ThirdPersonFront
    );
    assert_eq!(
        camera::next_perspective(PerspectiveMode::ThirdPersonFront),
        PerspectiveMode::FirstPerson
    );
}

#[test]
fn front_perspective_inverts_horizontal_orbit_input_only() {
    let delta = Vec2::new(8.0, -3.0);
    assert_eq!(
        camera::perspective_look_delta(delta, PerspectiveMode::FirstPerson),
        delta
    );
    assert_eq!(
        camera::perspective_look_delta(delta, PerspectiveMode::ThirdPersonBack),
        delta
    );
    assert_eq!(
        camera::perspective_look_delta(delta, PerspectiveMode::ThirdPersonFront),
        Vec2::new(-8.0, -3.0)
    );
}

#[test]
fn perspective_poses_orbit_four_blocks_and_face_the_subject() {
    let subject = Vec3::new(4.0, 70.0, -2.0);
    let rotation = Quat::from_euler(EulerRot::YXZ, 0.7, -0.3, 0.0);
    let forward = rotation * Vec3::NEG_Z;

    let first = camera::perspective_pose(subject, rotation, PerspectiveMode::FirstPerson);
    assert!(first.translation.abs_diff_eq(subject, 1.0e-6));
    assert!(first.rotation.abs_diff_eq(rotation, 1.0e-6));

    let back = camera::perspective_pose(subject, rotation, PerspectiveMode::ThirdPersonBack);
    assert!((back.translation.distance(subject) - 4.0).abs() < 1.0e-5);
    assert!((back.translation - (subject - forward * 4.0)).length() < 1.0e-5);
    assert!((back.rotation * Vec3::NEG_Z).dot((subject - back.translation).normalize()) > 0.999);

    let front = camera::perspective_pose(subject, rotation, PerspectiveMode::ThirdPersonFront);
    let horizontal_forward = Vec3::new(forward.x, 0.0, forward.z).normalize();
    assert!((front.translation.distance(subject) - 4.0).abs() < 1.0e-5);
    assert!((front.translation - (subject + horizontal_forward * 4.0)).length() < 1.0e-5);
    assert!((front.rotation * Vec3::NEG_Z).dot((subject - front.translation).normalize()) > 0.999);
}

#[test]
fn malformed_subject_rotation_cannot_poison_the_live_camera() {
    let mut view = LocalViewPose::default();
    let original = view.rotation();
    view.set_rotation(Quat::from_xyzw(0.0, 0.0, 0.0, 0.0));
    assert_eq!(view.rotation(), original);
    view.set_rotation(Quat::from_xyzw(f32::NAN, 0.0, 0.0, 1.0));
    assert_eq!(view.rotation(), original);
}

#[test]
fn settings_authority_rejects_stale_and_invalid_fov_updates_atomically() {
    let mut authority = CameraSettingsAuthority::default();
    let mut settings = UserSettings::default();
    settings.video.horizontal_fov_degrees = 82.0;
    settings.gameplay.default_perspective = PerspectiveMode::ThirdPersonBack;
    authority.replace(7, &settings).unwrap();
    assert_eq!(authority.generation(), 7);
    assert_eq!(authority.horizontal_fov_degrees(), 82.0);
    assert_eq!(authority.perspective(), PerspectiveMode::ThirdPersonBack);

    settings.video.horizontal_fov_degrees = f32::NAN;
    assert_eq!(
        authority.replace(8, &settings),
        Err(CameraSettingsError::NonFiniteFov)
    );
    assert_eq!(authority.generation(), 7);
    assert_eq!(authority.horizontal_fov_degrees(), 82.0);

    settings.video.horizontal_fov_degrees = 29.99;
    assert_eq!(
        authority.replace(8, &settings),
        Err(CameraSettingsError::FovOutOfRange)
    );
    assert_eq!(authority.generation(), 7);
    assert_eq!(authority.horizontal_fov_degrees(), 82.0);

    settings.video.horizontal_fov_degrees = 120.01;
    assert_eq!(
        authority.replace(8, &settings),
        Err(CameraSettingsError::FovOutOfRange)
    );
    assert_eq!(authority.generation(), 7);
    assert_eq!(authority.horizontal_fov_degrees(), 82.0);

    settings.video.horizontal_fov_degrees = 90.0;
    assert_eq!(
        authority.replace(7, &settings),
        Err(CameraSettingsError::StaleGeneration {
            previous: 7,
            actual: 7,
        })
    );
    assert_eq!(authority.horizontal_fov_degrees(), 82.0);

    settings.video.horizontal_fov_degrees = 30.0;
    authority.replace(8, &settings).unwrap();
    assert_eq!(authority.horizontal_fov_degrees(), 30.0);
    settings.video.horizontal_fov_degrees = 120.0;
    authority.replace(9, &settings).unwrap();
    assert_eq!(authority.horizontal_fov_degrees(), 120.0);
}

#[test]
fn captured_f5_cycles_perspective_without_moving_the_local_view() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default());
    app.add_systems(
        Update,
        (
            collect_raw_input.in_set(ClientFrameSet::RawInput),
            route_semantic_input.in_set(ClientFrameSet::SemanticSample),
            finalize_semantic_input_after_ui_authority.in_set(ClientFrameSet::SemanticFinalize),
        ),
    );
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..default()
        },
        PrimaryWindow,
    ));
    app.update();
    let subject = app.world().resource::<LocalViewPose>().eye_translation();

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::F5);
    app.update();

    assert_eq!(
        app.world()
            .resource::<CameraSettingsAuthority>()
            .perspective(),
        PerspectiveMode::ThirdPersonBack
    );
    assert_eq!(
        app.world().resource::<LocalViewPose>().eye_translation(),
        subject
    );
}

#[test]
fn captured_f5_tap_between_frames_still_cycles_perspective_once() {
    let mut app = App::new();
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default());
    app.add_systems(
        Update,
        (
            collect_raw_input.in_set(ClientFrameSet::RawInput),
            route_semantic_input.in_set(ClientFrameSet::SemanticSample),
            finalize_semantic_input_after_ui_authority.in_set(ClientFrameSet::SemanticFinalize),
        ),
    );
    app.world_mut().spawn((
        Window {
            focused: true,
            ..default()
        },
        CursorOptions {
            grab_mode: CursorGrabMode::Locked,
            visible: false,
            ..default()
        },
        PrimaryWindow,
    ));
    app.update();

    {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::F5);
        keys.release(KeyCode::F5);
    }
    app.update();
    assert_eq!(
        app.world()
            .resource::<CameraSettingsAuthority>()
            .perspective(),
        PerspectiveMode::ThirdPersonBack
    );

    // Production's input lifecycle clears transient press/release flags at the
    // next frame boundary. This focused app injects ButtonInput directly, so
    // reproduce that boundary explicitly before proving the tap cannot repeat.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    assert_eq!(
        app.world()
            .resource::<CameraSettingsAuthority>()
            .perspective(),
        PerspectiveMode::ThirdPersonBack,
        "the synthetic one-frame tap must not repeat"
    );
}

#[test]
fn replacing_user_settings_updates_the_live_projection() {
    let mut app = App::new();
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default());
    app.world_mut().spawn((
        Window {
            resolution: WindowResolution::new(1600, 900),
            focused: true,
            ..default()
        },
        CursorOptions::default(),
        PrimaryWindow,
    ));
    app.update();

    let mut settings = UserSettings::default();
    settings.video.horizontal_fov_degrees = 82.0;
    app.world_mut()
        .resource_mut::<RuntimeSettings>()
        .replace_user_settings(settings);
    app.update();

    let projection = app
        .world_mut()
        .query_filtered::<&Projection, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap();
    let Projection::Perspective(perspective) = projection else {
        panic!("fly camera projection is not perspective");
    };
    assert_eq!(
        app.world()
            .resource::<CameraSettingsAuthority>()
            .generation(),
        1
    );
    let expected = camera::horizontal_fov_to_vertical(82.0_f32.to_radians(), 16.0 / 9.0);
    assert!((perspective.fov - expected).abs() < 1.0e-6);
}

#[test]
fn horizontal_fov_conversion_is_finite_and_bounded_for_bad_inputs() {
    for horizontal in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0, 999.0] {
        for aspect in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
            let vertical = camera::horizontal_fov_to_vertical(horizontal, aspect);
            assert!(vertical.is_finite());
            assert!(vertical > 0.0 && vertical < std::f32::consts::PI);
        }
    }
}

#[test]
fn plugin_spawns_camera_with_default_horizontal_fov() {
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
    let expected = camera::horizontal_fov_to_vertical(90.0_f32.to_radians(), 16.0 / 9.0);
    assert!(
        (perspective.fov - expected).abs() <= 1.0e-6,
        "vertical FOV = {} degrees, want aspect-correct 90-degree horizontal FOV",
        perspective.fov.to_degrees()
    );
}

#[test]
fn camera_vertical_fov_tracks_primary_window_aspect_changes() {
    let mut app = App::new();
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default());
    let window = app
        .world_mut()
        .spawn((
            Window {
                resolution: WindowResolution::new(1600, 900),
                focused: true,
                ..default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();

    app.update();
    let fov_16_9 = match app
        .world_mut()
        .query_filtered::<&Projection, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap()
    {
        Projection::Perspective(perspective) => perspective.fov,
        _ => panic!("fly camera projection is not perspective"),
    };
    assert!(
        (fov_16_9 - camera::horizontal_fov_to_vertical(90.0_f32.to_radians(), 16.0 / 9.0)).abs()
            < 1.0e-6
    );

    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .resolution
        .set_physical_resolution(1200, 900);
    app.update();

    let fov_4_3 = match app
        .world_mut()
        .query_filtered::<&Projection, (With<Camera3d>, With<FlyCamera>)>()
        .single(app.world())
        .unwrap()
    {
        Projection::Perspective(perspective) => perspective.fov,
        _ => panic!("fly camera projection is not perspective"),
    };
    assert!(
        (fov_4_3 - camera::horizontal_fov_to_vertical(90.0_f32.to_radians(), 4.0 / 3.0)).abs()
            < 1.0e-6
    );
    assert!(fov_4_3 > fov_16_9);
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
    let start = app.world().resource::<LocalViewPose>().eye_translation();
    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(0.5));
    app.update();

    let end = *app.world().resource::<LocalViewPose>();
    let expected = start + camera::auto_fly_offset(0.5);
    assert!(
        end.eye_translation().abs_diff_eq(expected, 1.0e-4),
        "auto-fly stayed at {:?} instead of advancing to {expected:?}",
        end.eye_translation(),
    );
    assert!(
        (end.rotation() * Vec3::NEG_Z).dot((target - end.eye_translation()).normalize()) > 0.999,
        "auto-fly did not keep the target in view while unfocused"
    );
}
