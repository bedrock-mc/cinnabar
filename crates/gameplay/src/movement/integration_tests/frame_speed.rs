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
        always_sprint: false,
        toggle_sneak: false,
        facts: Default::default(),
        item_use_modifier: None,
        hold: None,
    }
}

#[test]
fn loading_and_spawn_search_share_stationary_ticks_but_differ_in_wire_admission() {
    let (mut physics, mut ticker) = walked_physics(0);
    let position = physics.network_position().unwrap();
    let mut locals = super::LocomotionState::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    ticker.begin_respawn_search();
    for withhold_input in [true, false] {
        let mut frame = speed_frame(semantic_input::ActionPhase {
            pressed: true,
            held: true,
            released: false,
        });
        frame.jump = semantic_input::ActionPhase {
            pressed: true,
            held: true,
            released: false,
        };
        frame.hold = Some(super::PhysicsFrameHold {
            registry: sim::CollisionRegistry::new().identity(),
            withhold_input,
        });
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        assert_eq!(physics.network_position(), Some(position));
        assert_eq!(ticker.has_unsent_inputs(), !withhold_input);
    }
    assert_eq!(ticker.completed_tick(), 102);
    let snapshots = ticker.pending_snapshots();
    assert_eq!(snapshots.len(), 1);
    assert_eq!(snapshots[0].tick, 102);
    assert_eq!(snapshots[0].delta, [0.0; 3]);
    assert_eq!(snapshots[0].move_vector, [0.0; 2]);
    assert!(locals.advance(
        speed_frame(Default::default()),
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    assert_eq!(ticker.completed_tick(), 103);
    assert_ne!(physics.network_position(), Some(position));
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

#[test]
fn always_sprint_packets_follow_processed_movement_and_stop_when_idle() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    for (index, (forward, sneak, blocked, enabled, expected)) in [
        (1.0, false, false, true, true),
        (0.0, false, false, true, false),
        (1.0, false, false, true, true),
        (1.0, true, false, true, false),
        (1.0, false, true, true, false),
        (1.0, false, false, true, true),
        (1.0, false, false, false, false),
        (-1.0, false, false, true, false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut frame = speed_frame(Default::default());
        frame.now += Duration::from_millis(index as u64 * 50);
        frame.movement = [0.0, forward];
        frame.raw_movement = frame.movement;
        frame.analogue_movement = frame.movement;
        frame.sneak.held = sneak;
        frame.facts.sprint_blocked = blocked;
        frame.always_sprint = enabled;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1)
        ));
        let packet = &ticker.outbox.back().unwrap().snapshot;
        assert_eq!(
            packet.flags.bits() & PlayerInputFlags::SPRINTING.bits() != 0,
            expected,
            "case {index}"
        );
        assert_eq!(
            packet.flags.bits() & PlayerInputFlags::SPRINT_DOWN.bits() != 0,
            expected,
            "case {index}"
        );
        if !expected {
            assert_eq!(
                packet.flags.bits() & PlayerInputFlags::START_SPRINTING.bits(),
                0,
                "case {index}"
            );
        }
    }
}
