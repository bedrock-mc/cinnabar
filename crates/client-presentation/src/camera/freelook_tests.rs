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
        auto,
        settings,
        time,
        smoother,
        view,
        server,
    );
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
        1,
        2,
        [0.0, 70.0, 0.0],
        &mut settings,
        &mut view,
        &mut LocalAvatarPresentation::default(),
    );
    assert_eq!(view.camera_rotation(), view.rotation());
    assert_eq!(settings.perspective(), PerspectiveMode::FirstPerson);
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
    let turned = |width: u32, sensitivity: Option<f32>| {
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
        let (yaw, _, _) = app
            .world()
            .resource::<LocalViewPose>()
            .rotation()
            .to_euler(EulerRot::YXZ);
        yaw.to_degrees()
    };
    let expected = |width, game| look::mouse_turn_degrees(Vec2::new(30.0, 0.0), width, game).x;
    assert!((turned(1920, None) - expected(1920, 0.628)).abs() < 1e-4);
    assert!((turned(2560, None) - expected(2560, 0.628)).abs() < 1e-4);
    assert!(turned(2560, None) < turned(1920, None));
    let mut game = look::GameSensitivity::default();
    game.set_sensitivity(0.8);
    assert!((turned(1920, Some(0.8)) - expected(1920, game.value())).abs() < 1e-4);
    assert_eq!(turned(1920, Some(0.5)), turned(1920, None));
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
