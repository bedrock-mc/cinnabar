use super::*;
use protocol::{ClientInputLock, ClientInputLocks};
use semantic_input::{Action, ActionPhase};

/// Runs input through the gameplay coordinator with a grounded player and loaded floor.
struct Harness {
    physics: LocalPhysicsController,
    ticker: MovementTicker,
    locals: super::super::LocomotionState,
    effects: super::super::LocalMovementEffectTimeline,
    speed: super::super::LocalMovementSpeedAuthority,
}

impl Harness {
    /// Starts a fresh session without consuming a physics tick.
    fn new() -> Self {
        let (physics, ticker) = walked_physics(0);
        Self {
            physics,
            ticker,
            locals: Default::default(),
            effects: Default::default(),
            speed: Default::default(),
        }
    }

    /// Advances one render frame and returns only a newly completed sample.
    fn step(&mut self, frame: super::super::PhysicsFrameInput) -> Option<PhysicsMovementSample> {
        self.physics.revoke_input_requests(frame.facts.input_locks);
        self.locals.revoke_input_requests(frame.facts.input_locks);
        let previous = self.ticker.completed_tick();
        assert!(self.locals.advance(
            frame,
            &mut self.physics,
            &mut self.ticker,
            &mut self.effects,
            &mut self.speed,
            &VersionedFloor(1),
        ));
        (self.ticker.completed_tick() > previous).then(|| {
            self.physics
                .sample_at(self.ticker.completed_tick())
                .unwrap()
                .clone()
        })
    }
}

/// Supplies an idle frame with one server lock and an explicit sub-tick duration.
fn frame(lock: Option<ClientInputLock>, millis: u64) -> super::super::PhysicsFrameInput {
    let mut frame = speed_frame(Default::default());
    frame.delta = Duration::from_millis(millis);
    frame.movement = [0.0; 2];
    frame.raw_movement = [0.0; 2];
    frame.analogue_movement = [0.0; 2];
    frame.movement_buttons = Default::default();
    frame.facts.input_locks = ClientInputLocks::from_bits(lock.map_or(0, |lock| lock as u32));
    frame
}

/// Checks simulation and encoded jump requests, including retained raw button edges.
fn assert_no_jump(sample: &PhysicsMovementSample) {
    assert!(!sample.jumping);
    assert!(!sample.processed.jump_initiated);
    assert!(!sample.jump_repeated);
    assert_eq!(sample.input.jump, ActionPhase::default());
    let flags = super::super::encoding::input_flags(sample, Default::default());
    let jump_flags = PlayerInputFlags::JUMPING
        | PlayerInputFlags::JUMP_DOWN
        | PlayerInputFlags::WANT_UP
        | PlayerInputFlags::START_JUMPING
        | PlayerInputFlags::JUMP_CURRENT_RAW
        | PlayerInputFlags::JUMP_PRESSED_RAW
        | PlayerInputFlags::JUMP_RELEASED_RAW;
    assert_eq!(flags.bits() & jump_flags.bits(), 0);
}

#[test]
fn server_input_locks_cancel_sub_tick_jump_requests() {
    for lock in [ClientInputLock::Jump, ClientInputLock::Movement] {
        let mut harness = Harness::new();
        let mut press = frame(None, 10);
        press.jump.pressed = true;
        assert!(harness.step(press).is_none());
        assert_no_jump(&harness.step(frame(Some(lock), 40)).unwrap());
        assert_no_jump(&harness.step(frame(None, 50)).unwrap());
        let mut fresh = frame(None, 50);
        fresh.jump = ActionPhase {
            pressed: true,
            held: true,
            released: false,
        };
        assert!(harness.step(fresh).unwrap().processed.jump_initiated);
    }
}

#[test]
fn server_input_locks_allow_jump_releases() {
    let mut harness = Harness::new();
    let mut press = frame(None, 10);
    press.jump.pressed = true;
    assert!(harness.step(press).is_none());
    let mut release = frame(Some(ClientInputLock::Jump), 40);
    release.jump.released = true;
    let sample = harness.step(release).unwrap();
    assert!(!sample.processed.jump_initiated);
    assert!(!sample.input.jump.pressed);
    assert!(sample.input.jump.released);
}

#[test]
fn server_input_locks_allow_sneak_releases() {
    let mut harness = Harness::new();
    let mut press = frame(None, 10);
    press.sneak.pressed = true;
    assert!(harness.step(press).is_none());
    let mut release = frame(Some(ClientInputLock::Sneak), 40);
    release.sneak.released = true;
    let sample = harness.step(release).unwrap();
    assert!(!sample.sneaking);
    assert!(!sample.input.sneak.pressed);
    assert!(sample.input.sneak.released);
}

#[test]
fn server_input_locks_allow_mod_jump_releases() {
    for lock in [
        ClientInputLock::Jump,
        ClientInputLock::Movement,
        ClientInputLock::Camera,
    ] {
        let mut harness = Harness::new();
        harness.physics.set_jump_pulse_scope(Some((7, 0)));
        harness.physics.request_jump_pulse();
        assert!(harness.step(frame(None, 50)).unwrap().input.jump.held);
        let released = harness.step(frame(Some(lock), 50)).unwrap().input.jump;
        assert!(released.released);
        assert!(!released.held);
        assert!(!released.pressed);
    }
}

#[test]
fn server_input_locks_cancel_mod_jump_pulses_before_and_during_locked_ticks() {
    for lock in [ClientInputLock::Jump, ClientInputLock::Movement] {
        for millis in [10, 50] {
            let mut harness = Harness::new();
            harness.physics.set_jump_pulse_scope(Some((7, 0)));
            harness.physics.request_jump_pulse();
            if let Some(sample) = harness.step(frame(Some(lock), millis)) {
                assert_no_jump(&sample);
            }
            assert_no_jump(&harness.step(frame(None, 50 - millis % 50)).unwrap());
        }
    }
}

#[test]
fn server_input_locks_cancel_sub_tick_sneak_edges() {
    for lock in [ClientInputLock::Sneak, ClientInputLock::Movement] {
        let mut harness = Harness::new();
        let mut press = frame(None, 10);
        press.sneak.pressed = true;
        assert!(harness.step(press).is_none());
        let sample = harness.step(frame(Some(lock), 40)).unwrap();
        assert_eq!(sample.input.sneak, ActionPhase::default());
        assert!(!sample.input.sneak_down);
        assert!(!sample.sneaking);
        assert_eq!(
            harness.step(frame(None, 50)).unwrap().input.sneak,
            ActionPhase::default()
        );
    }
}

#[test]
fn server_input_locks_do_not_defer_locked_sneak_toggle_presses() {
    let mut harness = Harness::new();
    let mut locked = frame(Some(ClientInputLock::Sneak), 50);
    locked.toggle_sneak = true;
    locked.sneak = ActionPhase {
        pressed: true,
        held: true,
        released: false,
    };
    assert!(!harness.step(locked).unwrap().sneaking);
    let mut unlocked = frame(None, 50);
    unlocked.toggle_sneak = true;
    assert!(!harness.step(unlocked).unwrap().sneaking);
}

#[test]
fn server_input_locks_do_not_defer_locked_sprint_toggle_presses() {
    for lock in [
        ClientInputLock::Movement,
        ClientInputLock::LateralMovement,
        ClientInputLock::MoveForward,
    ] {
        let mut harness = Harness::new();
        let mut locked = frame(Some(lock), 50);
        locked.movement = [0.0, 1.0];
        locked.toggle_sprint = true;
        locked.sprint = ActionPhase {
            pressed: true,
            held: true,
            released: false,
        };
        assert!(!harness.step(locked).unwrap().input.sprint_down);
        let mut unlocked = frame(None, 50);
        unlocked.movement = [0.0, 1.0];
        unlocked.toggle_sprint = true;
        assert!(!harness.step(unlocked).unwrap().input.sprint_down);
    }
}

/// Samples real default bindings for one device, including its movement-vector carrier.
fn device_snapshot(mode: semantic_input::InputMode) -> semantic_input::ActionSnapshot {
    use semantic_input::{AxisDirection, DeviceFrame, PhysicalControl};
    let settings = semantic_input::ControlSettings::default();
    let mut device = DeviceFrame::default();
    for action in [
        Action::MoveForward,
        Action::MoveRight,
        Action::Jump,
        Action::Sneak,
    ] {
        let control = settings.bindings().iter().find_map(|binding| {
            (binding.context == semantic_input::InputContext::Gameplay && binding.action == action)
                .then_some(binding.chord.control)
                .filter(|control| {
                    matches!(
                        (mode, control),
                        (
                            semantic_input::InputMode::KeyboardMouse,
                            PhysicalControl::KeyboardUsage(_)
                        ) | (
                            semantic_input::InputMode::GamePad,
                            PhysicalControl::GamepadButton(_) | PhysicalControl::GamepadAxis { .. }
                        ) | (
                            semantic_input::InputMode::Touch,
                            PhysicalControl::TouchControl(_)
                        )
                    )
                })
        });
        match control {
            Some(PhysicalControl::KeyboardUsage(key)) => {
                let keyboard = device.keyboard_mouse.get_or_insert_default();
                keyboard.activity_sequence = 1;
                keyboard.keys.push(key);
            }
            Some(
                control @ (PhysicalControl::GamepadButton(_) | PhysicalControl::GamepadAxis { .. }),
            ) => {
                if device.controllers.is_empty() {
                    device.controllers.push(semantic_input::ControllerFrame {
                        activity_sequence: 1,
                        ..Default::default()
                    });
                }
                let controller = &mut device.controllers[0];
                match control {
                    PhysicalControl::GamepadButton(button) => controller.buttons.push(button),
                    PhysicalControl::GamepadAxis { axis, direction } => {
                        controller.axes[usize::from(axis)] = if direction == AxisDirection::Positive
                        {
                            1.0
                        } else {
                            -1.0
                        };
                    }
                    _ => unreachable!(),
                }
            }
            Some(PhysicalControl::TouchControl(hit_id)) => {
                device.touches.push(semantic_input::TouchContact {
                    contact_id: u64::from(hit_id),
                    activity_sequence: 1,
                    position: [0.0; 2],
                    delta: [0.0; 2],
                    hit_id: Some(hit_id),
                })
            }
            None if mode == semantic_input::InputMode::Touch => {}
            _ => panic!("missing default input for {mode:?} {action:?}"),
        }
    }
    if mode == semantic_input::InputMode::Touch {
        device.touches.push(semantic_input::TouchContact {
            contact_id: u64::from(semantic_input::touch::JOYSTICK),
            activity_sequence: 1,
            position: [0.0; 2],
            delta: [1.0; 2],
            hit_id: Some(semantic_input::touch::JOYSTICK),
        });
    }
    let mut router = semantic_input::SemanticInputRouter::default();
    router.route(device).unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.input_mode, mode);
    snapshot
}

/// Passes routed device controls through movement and both vehicle exit triggers.
fn assert_device_locks(snapshot: semantic_input::ActionSnapshot) {
    let mode = snapshot.input_mode;
    assert!(snapshot.movement.into_iter().all(|axis| axis > 0.0));
    assert!(snapshot.phases[Action::Jump as usize].held);
    assert!(snapshot.phases[Action::Sneak as usize].held);
    for lock in [
        None,
        Some(ClientInputLock::Movement),
        Some(ClientInputLock::LateralMovement),
        Some(ClientInputLock::MoveForward),
        Some(ClientInputLock::MoveRight),
        Some(ClientInputLock::Jump),
        Some(ClientInputLock::Sneak),
    ] {
        let mut harness = Harness::new();
        let mut frame = frame(lock, 50);
        frame.movement = snapshot.movement;
        frame.raw_movement = snapshot.raw_movement;
        frame.analogue_movement = snapshot.analogue_movement;
        frame.movement_buttons = snapshot.movement_buttons;
        frame.jump = snapshot.phases[Action::Jump as usize];
        frame.sneak = snapshot.phases[Action::Sneak as usize];
        frame.input_mode = match mode {
            semantic_input::InputMode::KeyboardMouse => PlayerInputMode::Mouse,
            semantic_input::InputMode::GamePad => PlayerInputMode::GamePad,
            semantic_input::InputMode::Touch => PlayerInputMode::Touch,
        };
        let locks = frame.facts.input_locks;
        let sample = harness.step(frame).unwrap();
        assert_eq!(sample.raw_move_vector, snapshot.raw_movement);
        assert_eq!(sample.analogue_move_vector, snapshot.analogue_movement);
        assert_eq!(
            sample.move_vector[0] > 0.0,
            !locks.blocks(ClientInputLock::MoveRight)
        );
        assert_eq!(
            sample.move_vector[1] > 0.0,
            !locks.blocks(ClientInputLock::MoveForward)
        );
        assert_eq!(sample.jumping, !locks.blocks(ClientInputLock::Jump));
        assert_eq!(sample.input.jump.held, !locks.blocks(ClientInputLock::Jump));
        assert_eq!(sample.sneaking, !locks.blocks(ClientInputLock::Sneak));
        assert_eq!(
            sample.input.sneak.held,
            !locks.blocks(ClientInputLock::Sneak)
        );
    }
    // A boat exits through Jump or Sneak, so both must be locked to keep the rider aboard.
    let both = ClientInputLock::Jump as u32 | ClientInputLock::Sneak as u32;
    for (ride, triggers) in [
        (super::super::RideKind::Boat, both),
        (super::super::RideKind::Other, ClientInputLock::Sneak as u32),
    ] {
        for locks in [
            triggers,
            ClientInputLock::Dismount as u32,
            ClientInputLock::Movement as u32,
        ] {
            let mut harness = Harness::new();
            let mut frame = frame(None, 50);
            frame.facts.input_locks = ClientInputLocks::from_bits(locks);
            frame.facts.ride = Some(ride);
            frame.facts.mount_runtime_id = Some(7);
            frame.jump = snapshot.phases[Action::Jump as usize];
            frame.sneak = snapshot.phases[Action::Sneak as usize];
            harness.locals.observe_dismount(&frame, 7);
            harness
                .locals
                .flush_dismount([0.0; 3], |_| panic!("locked device exited its vehicle"));
            frame.facts.input_locks = Default::default();
            harness.locals.observe_dismount(&frame, 7);
            let mut sent = false;
            harness.locals.flush_dismount([0.0; 3], |_| {
                sent = true;
                Ok(())
            });
            assert!(sent);
        }
    }
}

#[test]
fn server_input_locks_cover_keyboard_movement_jump_sneak_and_dismount() {
    assert_device_locks(device_snapshot(semantic_input::InputMode::KeyboardMouse));
}

#[test]
fn server_input_locks_cover_gamepad_movement_jump_sneak_and_dismount() {
    assert_device_locks(device_snapshot(semantic_input::InputMode::GamePad));
}

#[test]
fn server_input_locks_cover_touch_movement_jump_sneak_and_dismount() {
    assert_device_locks(device_snapshot(semantic_input::InputMode::Touch));
}

#[test]
fn server_input_locks_cover_rebound_mouse_movement_jump_sneak_and_dismount() {
    let settings = semantic_input::ControlSettings::default();
    let actions = [
        Action::MoveForward,
        Action::MoveRight,
        Action::Jump,
        Action::Sneak,
    ];
    let mut bindings = settings.bindings().to_vec();
    let buttons: Vec<_> = actions
        .iter()
        .enumerate()
        .map(|(index, action)| {
            let button = u8::try_from(index + 1).unwrap();
            bindings.push(semantic_input::ActionBinding {
                action: *action,
                context: semantic_input::InputContext::Gameplay,
                chord: semantic_input::InputChord {
                    control: semantic_input::PhysicalControl::MouseButton(button),
                    modifiers: Default::default(),
                },
            });
            button
        })
        .collect();
    let settings = semantic_input::ControlSettings::new(
        bindings,
        settings.mouse_sensitivity,
        settings.gamepad_look_sensitivity,
        settings.touch_look_sensitivity,
        settings.invert_mouse_y,
        settings.invert_gamepad_y,
        settings.gamepad_move_deadzone,
        settings.gamepad_look_deadzone,
    )
    .unwrap();
    let mut router = semantic_input::SemanticInputRouter::with_settings_and_touch_layout(
        settings,
        semantic_input::TouchControlLayout::default(),
    )
    .unwrap();
    router
        .route(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_buttons: buttons,
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert_device_locks(router.finalize().unwrap());
}

#[test]
fn server_input_locks_suppress_held_auto_jump_repetition() {
    for locked in [false, true] {
        let mut harness = Harness::new();
        let mut takeoffs = 0;
        for _ in 0..24 {
            let mut frame = frame(locked.then_some(ClientInputLock::Jump), 50);
            frame.jump.held = true;
            let sample = harness.step(frame).unwrap();
            takeoffs += usize::from(sample.processed.jump_initiated);
            if locked {
                assert_no_jump(&sample);
            }
        }
        assert_eq!(takeoffs >= 2, !locked);
    }
}

#[test]
fn server_input_locks_cancel_a_dismount_waiting_for_transport_capacity() {
    let mut harness = Harness::new();
    let mut frame = frame(None, 50);
    frame.facts.ride = Some(super::super::RideKind::Other);
    frame.facts.mount_runtime_id = Some(7);
    frame.sneak.pressed = true;
    harness.locals.observe_dismount(&frame, 7);
    let mut attempted = false;
    harness.locals.flush_dismount([0.0; 3], |_| {
        attempted = true;
        Err(protocol::BatchSendError::Full)
    });
    assert!(attempted);
    frame.sneak = Default::default();
    frame.facts.input_locks = ClientInputLocks::from_bits(ClientInputLock::Dismount as u32);
    harness
        .locals
        .revoke_input_requests(frame.facts.input_locks);
    harness.locals.observe_dismount(&frame, 7);
    harness
        .locals
        .flush_dismount([0.0; 3], |_| panic!("deferred dismount survived its lock"));
    frame.facts.input_locks = Default::default();
    harness.locals.observe_dismount(&frame, 7);
    harness
        .locals
        .flush_dismount([0.0; 3], |_| panic!("unlock replayed an old dismount"));
}
