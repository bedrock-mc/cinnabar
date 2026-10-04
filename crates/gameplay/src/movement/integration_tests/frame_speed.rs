/// Samples one forward movement frame at the gameplay coordinator boundary.
fn speed_frame(sprint: semantic_input::ActionPhase) -> super::PhysicsFrameInput {
    super::PhysicsFrameInput {
        delta: Duration::from_millis(50),
        now: Duration::from_secs(1),
        active: true,
        movement: [0.0, 1.0],
        raw_movement: [0.0, 1.0],
        analogue_movement: [0.0, 1.0],
        yaw: 180.0,
        pitch: 0.0,
        camera_orientation: [0.0, 0.0, -1.0],
        input_mode: PlayerInputMode::Mouse,
        jump: Default::default(),
        sprint,
        sneak: Default::default(),
        toggle_sprint: false,
        toggle_sneak: false,
        facts: Default::default(),
        item_use_modifier: None,
    }
}

#[test]
fn gameplay_frame_applies_local_sprint_modifier_once() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut speed = super::LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    assert!(speed.apply(7, 1, 0, f64::from(0.12_f32), None));
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    for pressed in [true, false] {
        assert!(locals.advance(
            speed_frame(semantic_input::ActionPhase {
                pressed,
                held: true,
                released: false,
            }),
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        assert_eq!(
            speed.current(),
            Some(f64::from(0.12_f32 * sim::SPRINT_SPEED_MULTIPLIER as f32)),
            "the frame owner must apply the local sprint edge exactly once"
        );
    }
}

#[test]
fn gameplay_frame_adopts_server_sprint_without_double_boosting() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut expected = physics.clone();
    expected.advance(
        Duration::from_millis(50),
        MovementInput {
            sprinting: true,
            movement_speed: Some(f64::from(0.13_f32 / sim::SPRINT_SPEED_MULTIPLIER as f32)),
            ..forward_physics_input()
        },
        &VersionedFloor(1),
    );
    let mut speed = super::LocalMovementSpeedAuthority::default();
    speed.begin_session(7, 0);
    assert!(speed.apply(7, 1, 0, f64::from(0.13_f32), None));
    physics.apply_server_movement_flags(0, flags(|flags| flags.sprinting = Some(true)));
    assert!(super::LocomotionState::default().advance(
        speed_frame(Default::default()),
        &mut physics,
        &mut ticker,
        &mut super::LocalMovementEffectTimeline::default(),
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(speed.current(), Some(f64::from(0.13_f32)));
    assert!(physics.sample_at(101).unwrap().sprinting);
    assert_eq!(
        physics.state(),
        expected.state(),
        "the frame owner must cancel the simulator's multiplier for effective server speed"
    );
}
