/// Samples one forward movement frame at the gameplay coordinator boundary.
fn speed_frame(sprint: semantic_input::ActionPhase) -> super::PhysicsFrameInput {
    super::PhysicsFrameInput {
        delta: Duration::from_millis(50),
        now: Duration::from_secs(1),
        active: true,
        input_authority_released: false,
        resolved_movement: false,
        movement: [0.0, 1.0],
        raw_movement: [0.0, 1.0],
        analogue_movement: [0.0, 1.0],
        movement_buttons: semantic_input::MovementButtons {
            forward: true,
            ..Default::default()
        },
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
    ticker.begin_respawn_search(&mut physics);
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
    assert!(speed.apply(7, 1, 0, attribute(f64::from(0.12_f32), 0.12, None)));
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

/// Gliding keeps one sprint speed between fixed ticks, including with a Speed effect.
#[test]
fn gliding_sprint_speed_stays_stable_between_physics_ticks_and_after_replay() {
    for base_speed in [sim::DEFAULT_MOVEMENT_SPEED as f32, 0.14_f32] {
        let position = [0.0, 100.0, 0.0];
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position(position, 100, false);
        let mut ticker = MovementTicker::default();
        ticker.reset(7, 100, position);
        ticker.set_source(MovementSource::Physics);
        let mut locals = super::LocomotionState::default();
        let mut effects = super::LocalMovementEffectTimeline::default();
        let mut speed = super::LocalMovementSpeedAuthority::default();
        speed.begin_session(7, 0);
        assert!(speed.apply(7, 1, 0, attribute(f64::from(base_speed), base_speed, None)));
        let expected = f64::from(base_speed * sim::SPRINT_SPEED_MULTIPLIER as f32);
        for index in 0..12 {
            let mut frame = speed_frame(semantic_input::ActionPhase {
                held: true,
                ..Default::default()
            });
            frame.facts.elytra_ready = true;
            frame.jump.pressed = index == 0;
            frame.jump.held = index == 0;
            frame.delta = Duration::from_millis(25);
            frame.now += frame.delta * index;
            assert!(locals.advance(
                frame,
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
            assert_eq!(speed.current(), Some(expected), "render frame {index}");
            if index > 0 {
                assert_eq!(physics.mode(), sim::MovementMode::Gliding);
                assert_eq!(physics.latest_sneak_sprint(), Some((false, true)));
            }
        }
        reconcile_timeline_rewind(&mut ticker, &mut physics, 102, &VersionedFloor(1)).unwrap();
        assert_eq!(physics.mode(), sim::MovementMode::Gliding);
        assert_eq!(physics.latest_sneak_sprint(), Some((false, true)));
        for snapshot in ticker.pending_snapshots() {
            assert!(
                physics
                    .sample_at(snapshot.tick)
                    .unwrap()
                    .processed
                    .sprinting
            );
            assert_eq!(
                snapshot.flags.bits() & PlayerInputFlags::STOP_SPRINTING.bits(),
                0
            );
        }
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
    assert!(speed.apply(7, 1, 0, attribute(f64::from(0.13_f32), 0.1, None)));
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
        ticker.pending_snapshots()[0].flags.bits() & PlayerInputFlags::START_SPRINTING.bits(),
        0,
        "a server-authored sprint is not a local start action"
    );
    assert_eq!(
        physics.state(),
        expected.state(),
        "the frame owner must cancel the simulator's multiplier for effective server speed"
    );
}

/// A held sprint can restart after metadata clears it between completed ticks.
#[test]
fn server_sprint_stop_preserves_the_held_key_restart_action() {
    for toggle_sprint in [false, true] {
        let (mut physics, mut ticker) = walked_physics(0);
        let mut locals = super::LocomotionState::default();
        let mut effects = super::LocalMovementEffectTimeline::default();
        let mut speed = super::LocalMovementSpeedAuthority::default();
        let held = semantic_input::ActionPhase {
            held: true,
            ..Default::default()
        };
        for pressed in [true, false] {
            assert!(locals.advance(
                super::PhysicsFrameInput {
                    toggle_sprint,
                    ..speed_frame(semantic_input::ActionPhase { pressed, ..held })
                },
                &mut physics,
                &mut ticker,
                &mut effects,
                &mut speed,
                &VersionedFloor(1),
            ));
        }
        assert!(physics.sample_at(102).unwrap().processed.sprinting);
        physics.apply_server_movement_flags(0, flags(|flags| flags.sprinting = Some(false)));
        let mut tickless = speed_frame(held);
        tickless.toggle_sprint = toggle_sprint;
        tickless.delta = Duration::ZERO;
        assert!(locals.advance(
            tickless,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        let mut catch_up = speed_frame(held);
        catch_up.toggle_sprint = toggle_sprint;
        catch_up.delta = Duration::from_millis(100);
        assert!(locals.advance(
            catch_up,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        let start = PlayerInputFlags::START_SPRINTING.bits();
        let packets = ticker.pending_snapshots();
        assert_ne!(packets[2].flags.bits() & start, 0);
        assert_eq!(packets[3].flags.bits() & start, 0);
        assert!(physics.sample_at(103).unwrap().processed.sprinting);
        reconcile_timeline_rewind(&mut ticker, &mut physics, 101, &VersionedFloor(1)).unwrap();
        let replayed = ticker.pending_snapshots();
        assert_ne!(replayed[1].flags.bits() & start, 0);
        assert_eq!(replayed[2].flags.bits() & start, 0);
    }
}

#[test]
fn always_sprint_keeps_request_flags_while_actor_sprint_is_gated() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    for (index, (forward, sneak, blocked, enabled, expected)) in [
        (1.0, false, false, true, true),
        (0.0, false, false, true, false),
        (1.0, false, false, true, true),
        (1.0, true, false, true, true),
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
            enabled,
            "case {index}"
        );
        assert_eq!(
            packet.flags.bits() & PlayerInputFlags::SPRINT_DOWN.bits() != 0,
            enabled,
            "case {index}"
        );
        assert_eq!(
            physics.sample_at(packet.tick).unwrap().processed.sprinting,
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

/// Suspended input drops toggled sneak on keyboard but persistent controls keep it.
#[test]
fn suspended_input_keeps_toggled_sneak_only_under_persistent_controls() {
    let has = |flags: PlayerInputFlags, flag: PlayerInputFlags| flags.bits() & flag.bits() != 0;
    for (mode, kept) in [
        (PlayerInputMode::Mouse, false),
        (PlayerInputMode::GamePad, true),
    ] {
        let (mut physics, mut ticker) = walked_physics(0);
        let mut locals = super::LocomotionState::default();
        let mut speed = super::LocalMovementSpeedAuthority::default();
        let mut effects = super::LocalMovementEffectTimeline::default();
        let mut toggle = speed_frame(Default::default());
        toggle.input_mode = mode;
        toggle.toggle_sneak = true;
        toggle.sneak = semantic_input::ActionPhase {
            pressed: true,
            held: true,
            released: false,
        };
        assert!(locals.advance(
            toggle,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        let mut suspended = speed_frame(Default::default());
        suspended.active = false;
        suspended.toggle_sneak = true;
        assert!(locals.advance(
            suspended,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        let flags = ticker.pending_snapshots().last().unwrap().flags;
        assert_eq!(has(flags, PlayerInputFlags::SNEAK_DOWN), kept, "{mode:?}");
        assert_eq!(
            has(flags, PlayerInputFlags::STOP_SNEAKING),
            !kept,
            "{mode:?}"
        );
        assert_eq!(
            has(flags, PlayerInputFlags::PERSIST_SNEAK),
            kept,
            "{mode:?}"
        );
    }
}

/// Yaw wraps to vanilla's [-180, 180) and head yaw keeps the unwrapped camera angle.
#[test]
fn outbound_yaw_and_head_yaw_use_vanilla_ranges() {
    for (degrees, yaw) in [
        (270.0, -90.0),
        (180.0, -180.0),
        (-180.0, -180.0),
        (0.0, 0.0),
        (540.0, -180.0),
    ] {
        assert_eq!(super::wire_yaw(degrees), yaw, "{degrees}");
    }
    for (yaw, head) in [(135.0, -225.0), (90.0, 90.0), (-170.0, -170.0)] {
        assert_eq!(super::wire_head_yaw(yaw), head, "{yaw}");
    }
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut frame = speed_frame(Default::default());
    frame.yaw = 135.0;
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    let snapshot = *ticker.pending_snapshots().last().unwrap();
    assert_eq!((snapshot.yaw, snapshot.head_yaw), (135.0, -225.0));
}

/// A one-second render stall runs two ticks and loses the rest instead of catching up.
#[test]
fn stalled_render_frame_runs_two_ticks_and_leaves_no_backlog() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let start = ticker.completed_tick();
    for (delta, ticks) in [(Duration::from_secs(1), 2), (Duration::ZERO, 2)] {
        let mut frame = speed_frame(Default::default());
        frame.delta = delta;
        assert!(locals.advance(
            frame,
            &mut physics,
            &mut ticker,
            &mut effects,
            &mut speed,
            &VersionedFloor(1),
        ));
        assert_eq!(ticker.completed_tick(), start + ticks);
    }
    assert_eq!(physics.tick_alpha(), 0.0);
    assert_eq!(physics.dropped_tick_count(), 0);
}

/// The default item-use slowdown sends the forward axis scaled by 0.35 squared.
#[test]
fn item_use_slowdown_scales_the_wire_move_vector_by_its_square() {
    let (mut physics, mut ticker) = walked_physics(0);
    let mut locals = super::LocomotionState::default();
    let mut speed = super::LocalMovementSpeedAuthority::default();
    let mut effects = super::LocalMovementEffectTimeline::default();
    let mut frame = speed_frame(Default::default());
    frame.item_use_modifier = Some(0.35);
    assert!(locals.advance(
        frame,
        &mut physics,
        &mut ticker,
        &mut effects,
        &mut speed,
        &VersionedFloor(1),
    ));
    let snapshot = *ticker.pending_snapshots().last().unwrap();
    assert_eq!(snapshot.move_vector, [0.0, 0.35_f32 * 0.35_f32]);
}
