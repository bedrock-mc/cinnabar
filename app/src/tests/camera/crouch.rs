#[test]
fn crouch_eye_and_actor_feet_are_frozen_independently() {
    let mut standing = frozen_local_player_sample();
    standing.eye = standing.feet + Vec3::Y * protocol::PLAYER_NETWORK_OFFSET;
    standing.pose = perspective_pose(standing.eye, standing.rotation, standing.perspective);
    let mut crouched = standing.clone();
    crouched.eye.y -= 0.35;
    crouched.pose = perspective_pose(crouched.eye, crouched.rotation, crouched.perspective);
    let mut carrier = LocalPlayerFrameCarrier::default();
    carrier.publish(crouched.clone()).unwrap();
    let frame = carrier.snapshot().unwrap();
    assert_eq!(frame.eye(), crouched.eye);
    assert_eq!(frame.feet(), standing.feet);

    let mut interaction = InteractionOriginSnapshot::default();
    interaction.publish_from_local_player_frame(&carrier);
    assert_eq!(interaction.outbound_ray().unwrap().origin(), crouched.eye);

    let mut avatar = LocalAvatarPresentation::default();
    avatar.begin_session(standing.session_generation, 91);
    let mut visibility = LocalAvatarVisibilityCarrier::default();
    avatar.publish_visibility(frame, &mut visibility);
    let snapshot = visibility.snapshot().unwrap();
    assert_eq!(snapshot.eye(), crouched.eye);
    assert_eq!(snapshot.feet(), standing.feet);
    let mut scene = ActorRenderScene::default();
    let rendered = update_actor_render_scene(&mut scene, 1.0, None, vec![], Some(snapshot));
    assert_eq!(rendered.instances[0].position, standing.feet.to_array());

    let prior = carrier.clone();
    crouched.feet = Vec3::NAN;
    assert_eq!(
        carrier.publish(crouched),
        Err(client_presentation::local_player::LocalPlayerFrameError::NonFiniteFeet)
    );
    assert_eq!(carrier, prior);
}

#[test]
fn local_view_pose_moves_both_origins_but_accepts_a_separate_crouch_eye() {
    let feet = Vec3::new(4.0, 70.0, -2.0);
    let eye = feet + Vec3::Y * (protocol::PLAYER_NETWORK_OFFSET - 0.35);
    let mut view = LocalViewPose::default();
    view.set_subject_position(eye, feet);
    assert_eq!(view.eye_translation(), eye);
    assert_eq!(view.feet_translation(), feet);
    let displacement = Vec3::new(2.0, 3.0, 1.0);
    view.set_eye_translation(eye + displacement);
    assert_eq!(view.eye_translation(), eye + displacement);
    assert_eq!(view.feet_translation(), feet + displacement);
    let prior = view;
    view.set_subject_position(Vec3::NAN, feet);
    assert_eq!(view, prior);
    view.set_subject_position(eye, Vec3::NAN);
    assert_eq!(view, prior);
}

#[test]
fn both_shift_bindings_produce_held_sneak_and_release() {
    for shift in [0xe1, 0xe5] {
        let mut runtime = SemanticInputRuntime::default();
        let frame = |keys| DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                keys,
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        };
        let pressed = runtime.route_and_finalize(frame(vec![shift])).unwrap();
        assert!(pressed.phases[Action::Sneak as usize].pressed);
        assert!(pressed.phases[Action::Sneak as usize].held);
        let held = runtime.route_and_finalize(frame(vec![shift])).unwrap();
        assert!(!held.phases[Action::Sneak as usize].pressed);
        assert!(held.phases[Action::Sneak as usize].held);
        let released = runtime.route_and_finalize(frame(vec![])).unwrap();
        assert!(released.phases[Action::Sneak as usize].released);
        assert!(!released.phases[Action::Sneak as usize].held);
    }
}
