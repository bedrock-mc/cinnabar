use {
    super::*,
    client_presentation::camera::{CameraSettingsAuthority, PITCH_LIMIT},
    client_presentation::local_player::LocalViewPose,
};

#[test]
fn front_orbit_follows_pitch_and_yaw_without_rolling_at_the_pitch_limits() {
    let subject = Vec3::new(4.0, 20.0, -3.0);
    for yaw in [-2.6, -0.5, 0.0, 0.8, 2.6] {
        for pitch in [-PITCH_LIMIT, -0.7, 0.0, 0.7, PITCH_LIMIT] {
            let rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
            let front =
                camera::perspective_pose(subject, rotation, PerspectiveMode::ThirdPersonFront);
            let back =
                camera::perspective_pose(subject, rotation, PerspectiveMode::ThirdPersonBack);
            assert!(front.translation.is_finite() && front.rotation.is_finite());
            assert!(
                (front.translation + back.translation).abs_diff_eq(subject * 2.0, 1.0e-5),
                "reverse orbit must retain elevation for yaw={yaw}, pitch={pitch}"
            );
            let to_subject = (subject - front.translation).normalize();
            assert!((front.rotation * Vec3::NEG_Z).dot(to_subject) > 0.999);
            assert!(
                (front.rotation * Vec3::X).y.abs() < 1.0e-4,
                "the camera's right axis must stay horizontal"
            );
            assert!((front.rotation * Vec3::Y).y > 0.0);
        }
    }
}

#[test]
fn routed_mouse_turns_keep_actor_direction_through_the_f5_cycle() {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1));
    configure_client_frame_schedule(&mut app);
    app.init_resource::<Time>()
        .add_plugins(FlyCameraPlugin::default())
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

    let initial_yaw = 0.6;
    let initial_pitch = 0.2;
    let rotation = Quat::from_euler(EulerRot::YXZ, initial_yaw, initial_pitch, 0.0);
    for (index, perspective) in [
        PerspectiveMode::FirstPerson,
        PerspectiveMode::ThirdPersonBack,
        PerspectiveMode::ThirdPersonFront,
        PerspectiveMode::FirstPerson,
    ]
    .into_iter()
    .enumerate()
    {
        if index != 0 {
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::F5);
        }
        for delta in [Vec2::new(12.0, -4.0), Vec2::new(-12.0, 4.0)] {
            app.world_mut()
                .resource_mut::<LocalViewPose>()
                .set_rotation(rotation);
            app.world_mut()
                .resource_mut::<AccumulatedMouseMotion>()
                .delta = delta;
            app.update();

            assert_eq!(
                app.world()
                    .resource::<CameraSettingsAuthority>()
                    .perspective(),
                perspective
            );
            let (yaw, pitch, _) = app
                .world()
                .resource::<LocalViewPose>()
                .rotation()
                .to_euler(EulerRot::YXZ);
            assert!(
                (yaw - initial_yaw) * delta.x < 0.0,
                "{perspective:?} reversed actor yaw for {delta:?}"
            );
            assert!(
                (pitch - initial_pitch) * delta.y < 0.0,
                "{perspective:?} reversed actor pitch for {delta:?}"
            );
            app.world_mut()
                .resource_mut::<AccumulatedMouseMotion>()
                .delta = Vec2::ZERO;
            let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
            keys.release(KeyCode::F5);
            keys.clear();
            app.update();
        }
    }
}
