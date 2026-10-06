use super::*;
use semantic_input::{ActionSnapshot, DeviceFrame, KeyboardMouseFrame, SemanticInputRouter};

#[derive(Resource)]
struct Routed(ActionSnapshot);

fn drive(
    routed: Res<Routed>,
    auto: Res<AutoFly>,
    settings: ResMut<CameraSettingsAuthority>,
    time: Res<Time>,
    smoother: ResMut<look::LookSmoother>,
    view: ResMut<LocalViewPose>,
) {
    update_look(
        (0.0, None),
        crate::observations::InputObservation(Some(&routed.0)),
        auto,
        settings,
        time,
        smoother,
        view,
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
    let mut settings = CameraSettingsAuthority {
        freelook: true,
        ..Default::default()
    };
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
