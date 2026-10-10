use std::time::Duration;

use crate::app::{ClientFrameSet, configure_client_frame_schedule};
use crate::semantic_controls::{
    SemanticInputSnapshot, collect_raw_input, finalize_semantic_input_after_ui_authority,
    route_semantic_input,
};
use crate::settings_runtime::RuntimeSettings;
use bevy::{
    anti_alias::fxaa::Fxaa,
    core_pipeline::tonemapping::Tonemapping,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, WindowResolution},
};
use client_presentation::local_player::LocalViewPose;
use semantic_input::{Action, PerspectiveMode};
use sim::{
    Aabb, CollisionQuery, CollisionWorld, LenientCollisionBoxes, LenientSkipCounts,
    Vec3 as SimVec3, WorldQueryError,
};
use ui::UserSettings;
use world::ChunkKey;
use {
    crate::camera::{self, FlyCameraPlugin},
    client_presentation::camera::{
        AutoFly, CameraSettingsAuthority, CameraSettingsError, FlyCamera, PITCH_LIMIT,
    },
};

mod cursor_changes;
use cursor_changes::track_test_focus;
mod front_input;
mod projection;

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

/// Feeds the camera a pre-resolved lenient result, standing in for the palette
/// adapter's per-cell skipping so a boom test can mix a real wall with skips.
struct LenientCameraFixture {
    result: LenientCollisionBoxes,
}

impl CollisionWorld for LenientCameraFixture {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(self.result.value.clone()))
    }

    fn collision_boxes_camera_lenient(
        &self,
        _query: Aabb,
    ) -> Result<LenientCollisionBoxes, WorldQueryError> {
        Ok(self.result.clone())
    }
}

#[test]
fn third_person_boom_traces_eight_corners_and_stops_before_solid_geometry() {
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let world = CameraCollisionFixture {
        boxes: vec![Aabb::new(
            SimVec3::new(-1.0, 1.0, 2.0),
            SimVec3::new(1.0, 3.0, 3.0),
        )],
        unavailable: false,
    };

    let pose = client_presentation::camera::collision_safe_perspective_pose(
        subject,
        Quat::IDENTITY,
        PerspectiveMode::ThirdPersonBack,
        &world,
    );

    assert!(pose.translation.abs_diff_eq(
        Vec3::new(
            0.0,
            2.0,
            4.02_f32.sqrt() - render_api::CAMERA_NEAR_PLANE_BLOCKS,
        ),
        1.0e-5,
    ));
}

#[test]
fn third_person_boom_handles_compound_wall_corner_ceiling_floor_transitions_before_hit() {
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let cases = [
        (
            "wall",
            Quat::IDENTITY,
            vec![Aabb::new(
                SimVec3::new(-1.0, 1.0, 2.0),
                SimVec3::new(1.0, 3.0, 3.0),
            )],
            4.02_f32.sqrt(),
        ),
        (
            "corner",
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_4),
            vec![
                Aabb::new(SimVec3::new(1.5, 1.0, -1.0), SimVec3::new(2.0, 3.0, 4.0)),
                Aabb::new(SimVec3::new(-1.0, 1.0, 1.5), SimVec3::new(4.0, 3.0, 2.0)),
            ],
            3.95_f32.sqrt(),
        ),
        (
            "ceiling",
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_4),
            vec![Aabb::new(
                SimVec3::new(-1.0, 4.0, -1.0),
                SimVec3::new(1.0, 4.5, 5.0),
            )],
            7.25_f32.sqrt(),
        ),
        (
            "floor",
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_4),
            vec![Aabb::new(
                SimVec3::new(-1.0, 0.5, -1.0),
                SimVec3::new(1.0, 1.0, 5.0),
            )],
            1.65_f32.sqrt(),
        ),
    ];

    for (label, rotation, boxes, contact_distance) in cases {
        let blocked_world = CameraCollisionFixture {
            boxes,
            unavailable: false,
        };
        let blocked = client_presentation::camera::collision_safe_perspective_pose(
            subject,
            rotation,
            PerspectiveMode::ThirdPersonBack,
            &blocked_world,
        );
        let blocked_distance = blocked.translation.distance(subject);
        let expected = contact_distance - render_api::CAMERA_NEAR_PLANE_BLOCKS;
        assert!(
            (blocked_distance - expected).abs() <= 1.0e-4,
            "{label} boom distance {blocked_distance} did not stop at pre-hit {expected}",
        );

        let clear = client_presentation::camera::collision_safe_perspective_pose(
            subject,
            rotation,
            PerspectiveMode::ThirdPersonBack,
            &CameraCollisionFixture::default(),
        );
        assert!(
            (clear.translation.distance(subject)
                - client_presentation::camera::THIRD_PERSON_RADIUS_BLOCKS)
                .abs()
                <= 1.0e-5,
            "{label} boom did not restore after collision space cleared",
        );
        let blocked_again = client_presentation::camera::collision_safe_perspective_pose(
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
fn third_person_boom_retains_its_reach_when_the_boom_region_is_unloaded() {
    // The old contract collapsed the boom onto the subject on any query error;
    // that glued the camera to the model near chunk edges. An unloaded region
    // must now be skipped, leaving the full preset boom rather than collapsing.
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let world = CameraCollisionFixture {
        unavailable: true,
        ..default()
    };

    let pose = client_presentation::camera::collision_safe_perspective_pose(
        subject,
        Quat::IDENTITY,
        PerspectiveMode::ThirdPersonBack,
        &world,
    );

    assert!(
        (pose.translation.distance(subject)
            - client_presentation::camera::THIRD_PERSON_RADIUS_BLOCKS)
            .abs()
            <= 1.0e-5,
        "unloaded boom region must not collapse the camera onto the subject",
    );
}

#[test]
fn third_person_boom_stops_at_a_real_wall_beside_a_skipped_cell() {
    // A boom region holding both an unknown/unloaded cell and a registered wall
    // must stop at the wall, never at 0 (collapse) and never at the full reach.
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let world = LenientCameraFixture {
        result: LenientCollisionBoxes {
            value: vec![Aabb::new(
                SimVec3::new(-1.0, 1.0, 2.0),
                SimVec3::new(1.0, 3.0, 3.0),
            )],
            skipped: LenientSkipCounts {
                unknown_runtime_id: 3,
                unloaded_chunk: 2,
            },
        },
    };

    let pose = client_presentation::camera::collision_safe_perspective_pose(
        subject,
        Quat::IDENTITY,
        PerspectiveMode::ThirdPersonBack,
        &world,
    );

    assert!(pose.translation.abs_diff_eq(
        Vec3::new(
            0.0,
            2.0,
            4.02_f32.sqrt() - render_api::CAMERA_NEAR_PLANE_BLOCKS
        ),
        1.0e-5,
    ));
}

#[test]
fn third_person_boom_keeps_full_reach_over_a_clear_loaded_region() {
    let subject = Vec3::new(0.0, 2.0, 0.0);
    let world = LenientCameraFixture {
        result: LenientCollisionBoxes::default(),
    };

    let pose = client_presentation::camera::collision_safe_perspective_pose(
        subject,
        Quat::IDENTITY,
        PerspectiveMode::ThirdPersonBack,
        &world,
    );

    assert!(
        (pose.translation.distance(subject)
            - client_presentation::camera::THIRD_PERSON_RADIUS_BLOCKS)
            .abs()
            <= 1.0e-5
    );
}

#[test]
fn missing_world_stream_falls_back_to_eye_in_third_person() {
    let eye = Vec3::new(4.0, 70.0, -3.0);
    let rotation = Quat::from_rotation_y(0.4);
    let pose = client_presentation::camera::unavailable_world_perspective_pose(
        eye,
        rotation,
        PerspectiveMode::ThirdPersonBack,
    );
    assert_eq!(pose.translation, eye);
    assert!(pose.rotation.abs_diff_eq(rotation, 1.0e-6));
}

#[test]
fn auto_fly_path_repeats_and_stays_within_the_loaded_radius() {
    assert_eq!(
        client_presentation::camera::auto_fly_offset(0.0),
        Vec3::ZERO
    );
    assert!(
        client_presentation::camera::auto_fly_offset(
            client_presentation::camera::AUTO_FLY_PERIOD_SECONDS
        )
        .abs_diff_eq(Vec3::ZERO, 0.001)
    );

    for sample in 0..=2_000 {
        let seconds =
            client_presentation::camera::AUTO_FLY_PERIOD_SECONDS * sample as f32 / 2_000.0;
        let offset = client_presentation::camera::auto_fly_offset(seconds);
        assert!(
            offset.xz().length()
                <= client_presentation::camera::AUTO_FLY_MAX_HORIZONTAL_BLOCKS + 0.001
        );
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
        let seconds =
            client_presentation::camera::AUTO_FLY_PERIOD_SECONDS * sample as f32 / 2_000.0;
        let position = anchor + client_presentation::camera::auto_fly_offset(seconds);
        assert!(position.distance(target) < 16.0 * 16.0);
        let rotation = client_presentation::camera::look_at_target(position, target);
        let forward = rotation * Vec3::NEG_Z;
        assert!(forward.dot((target - position).normalize()) > 0.999);
    }
}

#[test]
fn stable_presentation_pause_resumes_only_an_enabled_auto_fly_path() {
    let mut enabled = AutoFly::new(true);
    enabled.pause_for_stable_presentation();
    assert!(!enabled.enabled());
    enabled.resume_after_stable_presentation();
    assert!(enabled.enabled());

    let mut disabled = AutoFly::new(false);
    disabled.pause_for_stable_presentation();
    disabled.resume_after_stable_presentation();
    assert!(!disabled.enabled());
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

    let (yaw, pitch) = client_presentation::camera::look_angles(
        0.5,
        0.25,
        Vec2::new(10.0, -20.0),
        Vec2::splat(0.01),
    );
    assert!((yaw - 0.4).abs() < 1.0e-6);
    assert!((pitch - 0.45).abs() < 1.0e-6);

    let (_, up) =
        client_presentation::camera::look_angles(0.0, 0.0, Vec2::new(0.0, -1_000_000.0), Vec2::ONE);
    let (_, down) =
        client_presentation::camera::look_angles(0.0, 0.0, Vec2::new(0.0, 1_000_000.0), Vec2::ONE);
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

    assert!(client_presentation::camera::input_is_active(
        &focused, &captured
    ));
    assert!(!client_presentation::camera::input_is_active(
        &unfocused, &captured
    ));
    assert!(!client_presentation::camera::input_is_active(
        &focused, &visible
    ));
    assert!(!client_presentation::camera::input_is_active(
        &focused, &released
    ));
}

fn capture_test_app(
    focused: bool,
    grab_mode: CursorGrabMode,
    visible: bool,
    capture_on_start: bool,
) -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    app.init_resource::<client_presentation::camera::CursorFocus>()
        .add_systems(PreUpdate, track_test_focus)
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(AutoFly::with_startup_capture(false, capture_on_start))
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
fn candidate_startup_capture_locks_input_without_enabling_auto_fly() {
    let (mut app, window) = capture_test_app(true, CursorGrabMode::None, true, true);

    app.update();

    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::Locked);
    assert!(!cursor.visible);
    assert!(!app.world().resource::<AutoFly>().enabled());
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
fn consent_popup_releases_a_captured_cursor_whatever_the_scene_asks() {
    let (mut app, window) = capture_test_app(true, CursorGrabMode::Locked, false, true);
    app.insert_resource(crate::server_experiences::input::ConsentInput(true));

    app.update();

    let cursor = app.world().get::<CursorOptions>(window).unwrap();
    assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    assert!(cursor.visible);
}

#[test]
fn production_schedule_consumes_recapture_click_until_physical_release() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
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
            grab_mode: CursorGrabMode::None,
            visible: true,
            ..default()
        },
        PrimaryWindow,
    ));
    app.update();

    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert_eq!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Attack),
        Default::default()
    );

    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    assert_eq!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Attack),
        Default::default(),
        "the click that captured the cursor must remain quarantined while physically held"
    );

    {
        let mut mouse = app.world_mut().resource_mut::<ButtonInput<MouseButton>>();
        mouse.release(MouseButton::Left);
        mouse.clear();
    }
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Attack)
            .pressed
    );
}

#[test]
fn production_schedule_preserves_locked_cursor_attack_hold_across_frames() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
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

    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    let pressed = app
        .world()
        .resource::<SemanticInputSnapshot>()
        .phase(Action::Attack);
    assert!(pressed.pressed);
    assert!(pressed.held);

    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
    app.update();
    let held = app
        .world()
        .resource::<SemanticInputSnapshot>()
        .phase(Action::Attack);
    assert!(!held.pressed);
    assert!(held.held, "captured mining/attack must remain held");
}

#[test]
fn plugin_spawns_camera_and_auto_fly_uses_delta_seconds() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
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
    assert_eq!(msaa.samples(), ui::DEFAULT_ANTI_ALIASING_SAMPLES);
    let smaa = app
        .world_mut()
        .query_filtered::<&bevy::anti_alias::smaa::Smaa, (With<Camera3d>, With<FlyCamera>)>()
        .iter(app.world())
        .count();
    assert_eq!(smaa, 0);
    let camera3d = app
        .world_mut()
        .query_filtered::<&Camera3d, With<FlyCamera>>()
        .single(app.world())
        .unwrap();
    assert!(
        bevy::render::render_resource::TextureUsages::from(camera3d.depth_texture_usages)
            .contains(bevy::render::render_resource::TextureUsages::TEXTURE_BINDING)
    );
    let fxaa = app
        .world_mut()
        .query_filtered::<&Fxaa, (With<Camera3d>, With<FlyCamera>)>()
        .iter(app.world())
        .count();
    assert_eq!(fxaa, 0);
    let start = app.world().resource::<LocalViewPose>().eye_translation();
    assert!(app.world().resource::<AutoFly>().enabled());

    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(0.5));
    app.update();

    let end = app.world().resource::<LocalViewPose>().eye_translation();
    let expected = start + client_presentation::camera::auto_fly_offset(0.5);
    assert!(end.abs_diff_eq(expected, 1.0e-4));
}

#[test]
fn standalone_camera_keeps_advancing_when_the_host_diagnostic_clock_changes() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
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
    let start = app.world().resource::<LocalViewPose>().eye_translation();
    let step = Duration::from_millis(250);
    let mut elapsed = Duration::ZERO;

    for real_clock_present in [false, true, false] {
        if real_clock_present {
            let mut real_clock = Time::<bevy::time::Real>::default();
            real_clock.advance_by(Duration::from_secs(10));
            app.insert_resource(real_clock);
        } else {
            app.world_mut().remove_resource::<Time<bevy::time::Real>>();
        }
        app.world_mut().resource_mut::<Time>().advance_by(step);
        elapsed += step;
        app.update();

        let actual = app.world().resource::<LocalViewPose>().eye_translation();
        let expected = start + client_presentation::camera::auto_fly_offset(elapsed.as_secs_f32());
        assert!(actual.abs_diff_eq(expected, 1.0e-4));
        assert!(
            app.world()
                .resource::<client_presentation::camera::ScreenOverlays>()
                .layers
                .is_empty()
        );
    }
}

#[test]
fn stable_presentation_pause_ignores_held_movement_and_look_input() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::new(true))
        .add_systems(
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
    let frozen = *app.world().resource::<LocalViewPose>();

    app.world_mut()
        .resource_mut::<AutoFly>()
        .pause_for_stable_presentation();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::KeyW);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::new(15.0, -4.0);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(0.5));
    app.update();

    assert_eq!(*app.world().resource::<LocalViewPose>(), frozen);
}

#[test]
fn perspective_cycle_matches_bedrock_settings_order() {
    assert_eq!(
        client_presentation::camera::next_perspective(PerspectiveMode::FirstPerson),
        PerspectiveMode::ThirdPersonBack
    );
    assert_eq!(
        client_presentation::camera::next_perspective(PerspectiveMode::ThirdPersonBack),
        PerspectiveMode::ThirdPersonFront
    );
    assert_eq!(
        client_presentation::camera::next_perspective(PerspectiveMode::ThirdPersonFront),
        PerspectiveMode::FirstPerson
    );
}

#[test]
fn perspective_poses_orbit_four_blocks_and_face_the_subject() {
    let subject = Vec3::new(4.0, 70.0, -2.0);
    let rotation = Quat::from_euler(EulerRot::YXZ, 0.7, -0.3, 0.0);
    let forward = rotation * Vec3::NEG_Z;
    let radius = client_presentation::camera::THIRD_PERSON_RADIUS_BLOCKS;

    let first = client_presentation::camera::perspective_pose(
        subject,
        rotation,
        PerspectiveMode::FirstPerson,
    );
    assert!(first.translation.abs_diff_eq(subject, 1.0e-6));
    assert!(first.rotation.abs_diff_eq(rotation, 1.0e-6));

    let back = client_presentation::camera::perspective_pose(
        subject,
        rotation,
        PerspectiveMode::ThirdPersonBack,
    );
    assert!((back.translation.distance(subject) - radius).abs() < 1.0e-5);
    assert!((back.translation - (subject - forward * radius)).length() < 1.0e-5);
    assert!((back.rotation * Vec3::NEG_Z).dot((subject - back.translation).normalize()) > 0.999);

    let front = client_presentation::camera::perspective_pose(
        subject,
        rotation,
        PerspectiveMode::ThirdPersonFront,
    );
    assert!((front.translation.distance(subject) - radius).abs() < 1.0e-5);
    assert!((front.translation - (subject + forward * radius)).length() < 1.0e-5);
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
fn review_unrelated_settings_keep_the_hotkey_perspective() {
    let mut authority = CameraSettingsAuthority::default();
    let mut settings = UserSettings::default();
    authority.replace(1, &settings).unwrap();
    authority.cycle_perspective();
    assert_eq!(authority.perspective(), PerspectiveMode::ThirdPersonBack);
    settings.video.horizontal_fov_degrees = 82.0;
    authority.replace(2, &settings).unwrap();
    assert_eq!(authority.perspective(), PerspectiveMode::ThirdPersonBack);
    settings.gameplay.default_perspective = PerspectiveMode::ThirdPersonFront;
    authority.replace(3, &settings).unwrap();
    assert_eq!(authority.perspective(), PerspectiveMode::ThirdPersonFront);
    authority.reset_perspective();
    authority.replace(4, &settings).unwrap();
    assert_eq!(authority.perspective(), PerspectiveMode::FirstPerson);
}

#[test]
fn captured_f5_cycles_perspective_without_moving_the_local_view() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
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
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
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
fn auto_fly_moves_and_rotates_while_unfocused_with_a_released_cursor() {
    let target = Vec3::new(4.5, 70.0, -3.5);
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
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
    let expected = start + client_presentation::camera::auto_fly_offset(0.5);
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
