#[test]
fn authority_release_publishes_neutral_and_quarantines_held_movement_until_neutral() {
    let mut router = SemanticInputRouter::default();
    let held = KeyboardMouseFrame {
        activity_sequence: 1,
        keys: vec![0x1a],
        ..KeyboardMouseFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(held.clone()),
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 1.0]);

    router.replace_authority(NonZeroU64::new(2).unwrap());
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(held.clone()),
            ..DeviceFrame::default()
        })
        .unwrap();
    let released = router.finalize().unwrap();
    assert_eq!(released.movement, [0.0, 0.0]);
    assert_eq!(released.look_delta, [0.0, 0.0]);
    assert!(released.phases[Action::MoveForward as usize].released);
    assert!(!released.phases[Action::MoveForward as usize].held);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(held),
            ..DeviceFrame::default()
        })
        .unwrap();
    let quarantined = router.finalize().unwrap();
    assert_eq!(quarantined.movement, [0.0, 0.0]);
    assert_eq!(
        quarantined.phases[Action::MoveForward as usize],
        Default::default()
    );

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 2,
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 3,
                keys: vec![0x1a],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let rearmed = router.finalize().unwrap();
    assert_eq!(rearmed.movement, [0.0, 1.0]);
    assert!(rearmed.phases[Action::MoveForward as usize].pressed);
}

#[test]
fn held_ui_accept_cannot_become_gameplay_jump_after_context_return() {
    let mut router = SemanticInputRouter::default();
    router.set_context(InputContext::UiFocused);
    let held_accept = ControllerFrame {
        device_id: 7,
        activity_sequence: 1,
        buttons: vec![0],
        ..ControllerFrame::default()
    };
    router
        .route(DeviceFrame {
            controllers: vec![held_accept.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::UiAccept as usize].pressed);

    router.set_context(InputContext::Gameplay);
    router
        .route(DeviceFrame {
            controllers: vec![held_accept.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    let transition = router.finalize().unwrap();
    assert_eq!(transition.movement, [0.0, 0.0]);
    assert_eq!(transition.phases[Action::Jump as usize], Default::default());

    router
        .route(DeviceFrame {
            controllers: vec![held_accept],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(
        router.finalize().unwrap().phases[Action::Jump as usize],
        Default::default()
    );

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 2,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Jump as usize].pressed);
}

#[test]
fn held_ui_cancel_cannot_become_gameplay_sneak_after_context_return() {
    let mut router = SemanticInputRouter::default();
    router.set_context(InputContext::UiFocused);
    let held_cancel = ControllerFrame {
        device_id: 7,
        activity_sequence: 1,
        buttons: vec![1],
        ..ControllerFrame::default()
    };
    router
        .route(DeviceFrame {
            controllers: vec![held_cancel.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::UiCancel as usize].pressed);

    router.set_context(InputContext::Gameplay);
    for _ in 0..2 {
        router
            .route(DeviceFrame {
                controllers: vec![held_cancel.clone()],
                ..DeviceFrame::default()
            })
            .unwrap();
        assert_eq!(
            router.finalize().unwrap().phases[Action::Sneak as usize],
            Default::default()
        );
    }
}

#[test]
fn focus_loss_is_a_neutral_barrier_before_physics_and_requires_rearm() {
    let mut router = SemanticInputRouter::default();
    let held = KeyboardMouseFrame {
        activity_sequence: 1,
        keys: vec![0x1a, 0x2c],
        ..KeyboardMouseFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(held.clone()),
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 2,
                ..KeyboardMouseFrame::default()
            }),
            window_focus_lost: true,
            ..DeviceFrame::default()
        })
        .unwrap();
    let unfocused = router.finalize().unwrap();
    assert_eq!(unfocused.movement, [0.0, 0.0]);
    assert!(unfocused.phases[Action::MoveForward as usize].released);
    assert!(unfocused.phases[Action::Jump as usize].released);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 3,
                ..held
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let still_held = router.finalize().unwrap();
    assert_eq!(still_held.movement, [0.0, 0.0]);
    assert!(!still_held.phases[Action::Jump as usize].pressed);
    assert!(!still_held.phases[Action::Jump as usize].held);
}

#[test]
fn focus_loss_frame_suppresses_its_own_mouse_motion() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [8.0, -4.0],
                ..KeyboardMouseFrame::default()
            }),
            window_focus_lost: true,
            ..DeviceFrame::default()
        })
        .unwrap();

    assert_eq!(router.finalize().unwrap().look_delta, [0.0, 0.0]);
}

#[test]
fn actual_controller_disconnect_releases_once_and_reconnect_waits_for_neutral() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 1,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Jump as usize].held);

    router
        .route(DeviceFrame {
            disconnected_controllers: vec![7],
            ..DeviceFrame::default()
        })
        .unwrap();
    let disconnected = router.finalize().unwrap();
    assert!(disconnected.phases[Action::Jump as usize].released);
    assert_eq!(
        disconnected.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::ControllerDisconnected)
    );

    router.route(DeviceFrame::default()).unwrap();
    let next = router.finalize().unwrap();
    assert!(!next.phases[Action::Jump as usize].released);
    assert_eq!(next.release_reasons[Action::Jump as usize], None);

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 2,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(
        router.finalize().unwrap().phases[Action::Jump as usize],
        Default::default()
    );
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 4,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Jump as usize].pressed);
}

#[test]
fn inactive_mode_controller_disconnect_still_quarantines_reconnect_until_neutral() {
    let mut router = SemanticInputRouter::default();
    let keyboard = KeyboardMouseFrame {
        activity_sequence: 2,
        keys: vec![0x1a],
        ..KeyboardMouseFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 1,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(
        router.finalize().unwrap().input_mode,
        semantic_input::InputMode::KeyboardMouse
    );

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            disconnected_controllers: vec![7],
            ..DeviceFrame::default()
        })
        .unwrap();
    let keyboard_owned = router.finalize().unwrap();
    assert_eq!(keyboard_owned.movement, [0.0, 1.0]);
    assert!(!keyboard_owned.phases[Action::MoveForward as usize].released);
    assert_eq!(
        keyboard_owned.release_reasons[Action::MoveForward as usize],
        None
    );

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(
        router.finalize().unwrap().phases[Action::Jump as usize],
        Default::default()
    );
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 4,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 5,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Jump as usize].pressed);
}
