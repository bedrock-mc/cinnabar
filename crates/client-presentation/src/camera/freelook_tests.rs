use super::*;
use semantic_input::{
    Action, ActionSnapshot, DeviceFrame, KeyboardMouseFrame, PerspectiveMode, SemanticInputRouter,
};

#[derive(Resource)]
struct Routed(ActionSnapshot);

fn drive(
    routed: Res<Routed>,
    auto: Res<AutoFly>,
    settings: ResMut<CameraSettingsAuthority>,
    time: Res<Time>,
    smoother: ResMut<look::LookSmoother>,
    view: ResMut<LocalViewPose>,
    server: Option<ResMut<ServerCameraView>>,
) {
    update_look(
        (0.0, None),
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
