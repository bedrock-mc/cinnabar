use super::*;
use semantic_input::{
    Action, ActionSnapshot, DeviceFrame, KeyboardMouseFrame, PerspectiveMode, SemanticInputRouter,
};

#[derive(Resource)]
struct Routed(ActionSnapshot);

#[derive(Resource)]
struct WindowWidth(u32);

#[allow(clippy::too_many_arguments)]
fn drive(
    routed: Res<Routed>,
    width: Option<Res<WindowWidth>>,
    auto: Res<AutoFly>,
    settings: ResMut<CameraSettingsAuthority>,
    time: Res<Time>,
    smoother: ResMut<look::LookSmoother>,
    view: ResMut<LocalViewPose>,
    server: Option<ResMut<ServerCameraView>>,
) {
    update_look(
        (0.0, None),
        width.map_or(1280, |width| width.0),
        crate::observations::InputObservation(Some(&routed.0)),
        Default::default(),
        auto,
        settings,
        time,
        smoother,
        view,
        server,
    );
}

#[test]
fn death_view_discards_pending_cinematic_look_and_ignores_new_input() {
    let mut configured = ui::UserSettings::default();
    configured.video.cinematic_camera = true;
    let mut settings = CameraSettingsAuthority::default();
    settings.replace(1, &configured).unwrap();
    settings.set_death_view(Some(Quat::IDENTITY));
    let initial = Quat::from_euler(EulerRot::YXZ, 0.4, 0.2, 0.0);
    let mut smoother = look::LookSmoother::default();
    let delta = std::time::Duration::from_secs_f32(1.0 / 60.0);
    assert_ne!(
        smoother.filter(Vec2::new(80.0, 20.0), delta.as_secs_f32()),
        Vec2::ZERO
    );
    let mut time = Time::<()>::default();
    time.advance_by(delta);
    let mut app = App::new();
    app.insert_resource(settings)
        .insert_resource(LocalViewPose::new(Vec3::Y * 70.0, initial))
        .insert_resource(AutoFly::new(false))
        .insert_resource(time)
        .insert_resource(smoother)
        .add_systems(Update, drive);
    let mut router = SemanticInputRouter::default();
    router.route(DeviceFrame::default()).unwrap();
    app.insert_resource(Routed(router.finalize().unwrap()));
    app.update();
    assert_eq!(app.world().resource::<LocalViewPose>().rotation(), initial);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [80.0, 20.0],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    app.insert_resource(Routed(router.finalize().unwrap()));
    app.update();
    assert_eq!(app.world().resource::<LocalViewPose>().rotation(), initial);

    app.world_mut()
        .resource_mut::<CameraSettingsAuthority>()
        .set_death_view(None);
    router.route(DeviceFrame::default()).unwrap();
    app.insert_resource(Routed(router.finalize().unwrap()));
    app.update();
    assert_eq!(app.world().resource::<LocalViewPose>().rotation(), initial);
}

#[test]
fn freelook_orbits_without_turning_gameplay_and_restores_on_release_or_focus_loss() {
    for focus_loss in [false, true] {
        let mut router = SemanticInputRouter::default();
        let mut app = App::new();
        let mut settings = CameraSettingsAuthority::default();
        settings.cycle_perspective();
        settings.cycle_perspective();
        let initial = Quat::from_rotation_y(0.4);
        app.insert_resource(settings)
            .insert_resource(LocalViewPose::new(Vec3::Y * 70.0, initial))
            .insert_resource(AutoFly::new(false))
            .init_resource::<Time>()
            .init_resource::<look::LookSmoother>()
            .add_systems(Update, drive);
        router
            .route(DeviceFrame {
                keyboard_mouse: Some(KeyboardMouseFrame {
                    activity_sequence: 1,
                    keys: vec![0x09, 0x1a],
                    mouse_motion: [80.0, 20.0],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
        let snapshot = router.finalize().unwrap();
        assert!(snapshot.phases[Action::Freelook as usize].held);
        assert!(snapshot.movement[1].abs() > 0.0);
        app.insert_resource(Routed(snapshot));
        app.update();
        let view = *app.world().resource::<LocalViewPose>();
        assert_eq!(view.rotation(), initial);
        assert!(view.camera_rotation().angle_between(initial) > 0.1);
        assert_eq!(
            app.world()
                .resource::<CameraSettingsAuthority>()
                .perspective(),
            PerspectiveMode::ThirdPersonBack
        );
        router
            .route(DeviceFrame {
                window_focus_lost: focus_loss,
                ..Default::default()
            })
            .unwrap();
        app.insert_resource(Routed(router.finalize().unwrap()));
        app.update();
        let view = *app.world().resource::<LocalViewPose>();
        assert_eq!(view.rotation(), initial);
        assert_eq!(view.camera_rotation(), initial);
        assert_eq!(
            app.world()
                .resource::<CameraSettingsAuthority>()
                .perspective(),
            PerspectiveMode::ThirdPersonFront
        );
    }
}

#[test]
fn session_reset_clears_camera_orbit_without_turning_the_actor() {
    let mut view = LocalViewPose::default();
    view.set_freelook(true);
    view.set_look_rotation(Quat::from_rotation_y(1.0));
    let mut settings = CameraSettingsAuthority::default();
    settings.freelook = true;
    crate::local_player::reset_local_player_session(
        simulation_clock::identity::SessionGeneration::new(1),
        2,
        [0.0, 70.0, 0.0],
        &mut settings,
        &mut view,
        &mut LocalAvatarPresentation::default(),
    );
    assert_eq!(view.camera_rotation(), view.rotation());
    assert_eq!(settings.perspective(), PerspectiveMode::FirstPerson);
}

/// Quaternion publications cross the signed boundary without losing the camera's yaw history.
#[test]
fn view_heading_keeps_wrap_crossings_and_freelook_turns() {
    let mut view = LocalViewPose::default();
    for yaw in [120.0_f32, 240.0, 360.0, 480.0] {
        view.set_rotation(Quat::from_rotation_y(-yaw.to_radians()));
        assert!((view.camera_look()[1] - yaw).abs() < 1e-4);
    }
    view.set_freelook(true);
    view.turn_look_rotation(
        Quat::from_rotation_y(-1080.0_f32.to_radians()),
        -600.0_f32.to_radians(),
    );
    assert!((view.camera_look()[1] - 1080.0).abs() < 1e-4);
    view.set_freelook(false);
    assert!((view.camera_look()[1] - 1200.0).abs() < 1e-4);
    let before = view;
    view.set_look_rotation(Quat::from_xyzw(0.0, 0.0, 0.0, 0.0));
    view.turn_look_rotation(Quat::IDENTITY, f32::NAN);
    assert_eq!(view, before);
}

/// Complete turn history does not round the orientation used for small camera input.
#[test]
fn hand_yaw_history_does_not_reduce_camera_orientation_precision() {
    let mut view = LocalViewPose::default();
    view.turn_look_rotation(Quat::IDENTITY, 10_000.0 * std::f32::consts::TAU);
    let rotation = Quat::from_rotation_y(0.0001);
    view.turn_look_rotation(rotation, 0.0001);
    assert!((view.look_rotation() * Vec3::NEG_Z).abs_diff_eq(rotation * Vec3::NEG_Z, 1e-7));
}

#[test]
fn routed_look_uses_follow_orbit_angles_before_clamping_the_actor() {
    use protocol::{CameraEvent, CameraInstructionEvent, CameraPreset, CameraSetInstruction};
    let context = server_view::ViewContext {
        base: Transform::IDENTITY,
        subject: Transform::IDENTITY,
        base_fov: 90.0,
        actors: &|_| None,
    };
    let mut server = ServerCameraView::default();
    server.apply(
        1,
        &CameraEvent::Presets(
            [CameraPreset {
                name: "minecraft:follow_orbit".into(),
                ..Default::default()
            }]
            .into(),
        ),
        &context,
    );
    server.apply(
        2,
        &CameraEvent::Instruction(Box::new(CameraInstructionEvent {
            set: Some(CameraSetInstruction {
                preset_id: 0,
                ease: None,
                position: None,
                rotation_degrees: None,
                facing_position: None,
                view_offset: None,
                entity_offset: None,
                default_preset: None,
                remove_ignore_starting_values: false,
            }),
            ..Default::default()
        })),
        &context,
    );
    let initial = server.pose_override(&context).unwrap().rotation;
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [10.0, 10.0],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let mut app = App::new();
    app.insert_resource(server)
        .insert_resource(CameraSettingsAuthority::default())
        .insert_resource(LocalViewPose::new(
            Vec3::ZERO,
            Quat::from_rotation_x(-PITCH_LIMIT),
        ))
        .insert_resource(AutoFly::new(false))
        .insert_resource(Routed(router.finalize().unwrap()))
        .init_resource::<Time>()
        .init_resource::<look::LookSmoother>()
        .add_systems(Update, drive);
    app.update();
    let rotation = app
        .world()
        .resource::<ServerCameraView>()
        .pose_override(&context)
        .unwrap()
        .rotation;
    assert!(rotation.angle_between(initial) > 0.01);
    assert!(
        app.world()
            .resource::<LocalViewPose>()
            .rotation()
            .dot(rotation)
            .abs()
            > 0.99999
    );
    let (_, pitch, _) = rotation.to_euler(EulerRot::YXZ);
    assert!(pitch < -45.0_f32.to_radians());
}

/// Mouse look turns vanilla's per-count degrees for the window's pixel width and sensitivity.
#[test]
fn mouse_look_turns_vanilla_degrees_for_window_width_and_sensitivity() {
    let turned = |width: u32, sensitivity: Option<f32>, counts: f32| {
        let mut router = SemanticInputRouter::default();
        router
            .route(DeviceFrame {
                keyboard_mouse: Some(KeyboardMouseFrame {
                    activity_sequence: 1,
                    mouse_motion: [-counts, 0.0],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
        let mut settings = CameraSettingsAuthority::default();
        if let Some(sensitivity) = sensitivity {
            let mut user = ui::UserSettings::default();
            user.controls.mouse_sensitivity = sensitivity;
            settings.replace(1, &user).unwrap();
        }
        let mut app = App::new();
        app.insert_resource(settings)
            .insert_resource(LocalViewPose::new(Vec3::ZERO, Quat::IDENTITY))
            .insert_resource(AutoFly::new(false))
            .insert_resource(Routed(router.finalize().unwrap()))
            .insert_resource(WindowWidth(width))
            .init_resource::<Time>()
            .init_resource::<look::LookSmoother>()
            .add_systems(Update, drive);
        app.update();
        -app.world().resource::<LocalViewPose>().camera_look()[1]
    };
    let expected = |width, game| look::mouse_turn_degrees(Vec2::new(30.0, 0.0), width, game).x;
    assert!((turned(1920, None, 30.0) - expected(1920, 0.628)).abs() < 1e-4);
    assert!((turned(2560, None, 30.0) - expected(2560, 0.628)).abs() < 1e-4);
    assert!(turned(2560, None, 30.0) < turned(1920, None, 30.0));
    let mut game = look::GameSensitivity::default();
    game.set_sensitivity(0.8);
    assert!((turned(1920, Some(0.8), 30.0) - expected(1920, game.value())).abs() < 1e-4);
    assert_eq!(turned(1920, Some(0.5), 30.0), turned(1920, None, 30.0));
    for counts in [
        -semantic_input::MAX_LOOK_DELTA_PER_FRAME,
        semantic_input::MAX_LOOK_DELTA_PER_FRAME,
    ] {
        let expected = look::mouse_turn_degrees(Vec2::new(counts, 0.0), 1920, 0.628).x;
        assert!(expected.abs() > 180.0);
        let actual = turned(1920, None, counts);
        assert!(
            (actual - expected).abs() < 1e-4,
            "{counts} routed counts turn {actual} degrees, expected {expected}"
        );
    }
}

#[test]
fn view_scale_reduces_calibrated_mouse_look_and_restores_without_changing_sensitivity() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [-30.0, 0.0],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let mut app = App::new();
    app.insert_resource(CameraSettingsAuthority::default())
        .insert_resource(LocalViewPose::new(Vec3::ZERO, Quat::IDENTITY))
        .insert_resource(AutoFly::new(false))
        .insert_resource(Routed(router.finalize().unwrap()))
        .insert_resource(WindowWidth(1920))
        .init_resource::<Time>()
        .init_resource::<look::LookSmoother>()
        .add_systems(Update, drive);
    let sensitivity = app
        .world()
        .resource::<CameraSettingsAuthority>()
        .game_sensitivity();
    let ordinary = look::mouse_turn_degrees(Vec2::new(30.0, 0.0), 1920, sensitivity).x;
    for scale in [0.25, 1.0] {
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .set_view_scale(scale, scale);
        *app.world_mut().resource_mut::<LocalViewPose>() =
            LocalViewPose::new(Vec3::ZERO, Quat::IDENTITY);
        app.update();
        let (yaw, _, _) = app
            .world()
            .resource::<LocalViewPose>()
            .rotation()
            .to_euler(EulerRot::YXZ);
        assert!((yaw.to_degrees() - ordinary * scale).abs() < 1e-4);
        assert_eq!(
            app.world()
                .resource::<CameraSettingsAuthority>()
                .game_sensitivity(),
            sensitivity
        );
    }
}

/// A native view ending cannot discard a still-admitted user freelook heading.
#[test]
fn detached_release_preserves_only_admitted_held_user_freelook() {
    for (user_held, admitted) in [(true, true), (false, true), (true, false)] {
        let mut settings = super::CameraSettingsAuthority::default();
        let mut view = crate::local_player::LocalViewPose::default();
        let body = view.camera_rotation();
        settings.apply_detached_view(
            &mut view,
            Some(super::DetachedView::Orbit { distance: 4.0 }),
            false,
        );
        settings.freelook = admitted;
        let heading = bevy::prelude::Quat::from_rotation_y(0.8);
        view.set_look_rotation(heading);
        settings.apply_detached_view(&mut view, None, user_held);
        assert_eq!(
            view.camera_rotation(),
            if user_held && admitted { heading } else { body }
        );
        assert!(settings.detached_view().is_none());
    }
}
