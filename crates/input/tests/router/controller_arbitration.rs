#[test]
fn held_reconnect_cannot_take_input_mode_from_still_held_keyboard() {
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
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let keyboard_owned = router.finalize().unwrap();
    assert_eq!(
        keyboard_owned.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(keyboard_owned.movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            disconnected_controllers: vec![7],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let reconnect = router.finalize().unwrap();
    assert_eq!(
        reconnect.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(reconnect.movement, [0.0, 1.0]);
    assert_eq!(reconnect.phases[Action::Jump as usize], Default::default());

    let neutral_controller = ControllerFrame {
        device_id: 7,
        activity_sequence: 4,
        ..ControllerFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            controllers: vec![neutral_controller.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    let neutral_rearm = router.finalize().unwrap();
    assert_eq!(
        neutral_rearm.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(neutral_rearm.movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            controllers: vec![neutral_controller],
            ..DeviceFrame::default()
        })
        .unwrap();
    let unchanged_neutral = router.finalize().unwrap();
    assert_eq!(
        unchanged_neutral.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(unchanged_neutral.movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 5,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let fresh_controller_activity = router.finalize().unwrap();
    assert_eq!(
        fresh_controller_activity.input_mode,
        semantic_input::InputMode::GamePad
    );
    assert!(
        fresh_controller_activity.phases[Action::Jump as usize].pressed,
        "fresh post-neutral controller activity becomes eligible"
    );
}

#[test]
fn quarantined_controller_axis_requires_deadzone_neutrality_across_sign_change() {
    let mut router = SemanticInputRouter::default();
    let positive = ControllerFrame {
        device_id: 7,
        activity_sequence: 1,
        axes: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ..ControllerFrame::default()
    };
    router
        .route(DeviceFrame {
            controllers: vec![positive.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [1.0, 0.0]);

    router.replace_authority(NonZeroU64::new(2).unwrap());
    router
        .route(DeviceFrame {
            controllers: vec![positive],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 0.0]);

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 2,
                axes: [-1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let sign_changed = router.finalize().unwrap();
    assert_eq!(sign_changed.movement, [0.0, 0.0]);
    assert!(sign_changed.phases.iter().all(|phase| !phase.held));

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                axes: [0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let deadzone_neutral = router.finalize().unwrap();
    assert_eq!(deadzone_neutral.movement, [0.0, 0.0]);
    assert!(deadzone_neutral.phases.iter().all(|phase| !phase.held));

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 4,
                buttons: vec![0],
                axes: [0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(
        router.finalize().unwrap().phases[Action::Jump as usize].pressed,
        "a stick inside the configured deadzone must rearm the controller"
    );
}

#[test]
fn deadzone_neutral_reconnect_cannot_take_input_mode_from_still_held_keyboard() {
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
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let keyboard_owned = router.finalize().unwrap();
    assert_eq!(
        keyboard_owned.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(keyboard_owned.movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            disconnected_controllers: vec![7],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                axes: [0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let reconnect = router.finalize().unwrap();
    assert_eq!(
        reconnect.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(reconnect.movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 4,
                buttons: vec![0],
                axes: [0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let fresh_button = router.finalize().unwrap();
    assert_eq!(fresh_button.input_mode, semantic_input::InputMode::GamePad);
    assert!(
        fresh_button.phases[Action::Jump as usize].pressed,
        "a sub-deadzone reconnect must rearm for a later button press"
    );
}

#[test]
fn post_reconnect_sub_deadzone_drift_cannot_take_mode_from_held_keyboard() {
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
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            disconnected_controllers: vec![7],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let neutral_reconnect = router.finalize().unwrap();
    assert_eq!(
        neutral_reconnect.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(neutral_reconnect.movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 4,
                axes: [0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let drift = router.finalize().unwrap();
    assert_eq!(drift.input_mode, semantic_input::InputMode::KeyboardMouse);
    assert_eq!(drift.movement, [0.0, 1.0]);
    assert!(drift.phases[Action::MoveForward as usize].held);
}

#[test]
fn connected_sub_deadzone_drift_cannot_take_mode_from_held_keyboard() {
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
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 1.0]);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 3,
                axes: [0.05, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let drift = router.finalize().unwrap();
    assert_eq!(drift.input_mode, semantic_input::InputMode::KeyboardMouse);
    assert_eq!(drift.movement, [0.0, 1.0]);
    assert!(drift.phases[Action::MoveForward as usize].held);
}

#[test]
fn authority_quarantine_uses_merged_multi_controller_radial_deadzone() {
    let mut router = SemanticInputRouter::default();
    let controllers = vec![
        ControllerFrame {
            device_id: 7,
            activity_sequence: 1,
            axes: [0.12, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ..ControllerFrame::default()
        },
        ControllerFrame {
            device_id: 8,
            activity_sequence: 1,
            axes: [0.0, 0.12, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            ..ControllerFrame::default()
        },
    ];
    router
        .route(DeviceFrame {
            controllers: controllers.clone(),
            ..DeviceFrame::default()
        })
        .unwrap();
    let held = router.finalize().unwrap();
    assert!(held.movement[0] > 0.0);
    assert!(held.movement[1] > 0.0);

    router.replace_authority(NonZeroU64::new(2).unwrap());
    router
        .route(DeviceFrame {
            controllers: controllers.clone(),
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 0.0]);

    router
        .route(DeviceFrame {
            controllers,
            ..DeviceFrame::default()
        })
        .unwrap();
    let unchanged = router.finalize().unwrap();
    assert_eq!(unchanged.movement, [0.0, 0.0]);
    assert!(unchanged.phases.iter().all(|phase| !phase.held));
}

#[test]
fn lowering_move_deadzone_quarantines_pending_axis_under_replacement_settings() {
    let mut router = SemanticInputRouter::default();
    router
        .replace_bindings(settings_with_deadzones(0.15, 0.15))
        .unwrap();
    let controller = ControllerFrame {
        device_id: 7,
        activity_sequence: 1,
        axes: [0.10, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ..ControllerFrame::default()
    };
    router
        .route(DeviceFrame {
            controllers: vec![controller.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 0.0]);

    router
        .route(DeviceFrame {
            controllers: vec![controller.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    router
        .replace_bindings(settings_with_deadzones(0.05, 0.15))
        .unwrap();
    let replacement = router.finalize().unwrap();
    assert_eq!(replacement.movement, [0.0, 0.0]);
    assert!(replacement.phases.iter().all(|phase| !phase.pressed));

    router
        .route(DeviceFrame {
            controllers: vec![controller],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().movement, [0.0, 0.0]);
}

#[test]
fn raising_move_deadzone_releases_axis_and_allows_fresh_button_activity() {
    let mut router = SemanticInputRouter::default();
    router
        .replace_bindings(settings_with_deadzones(0.05, 0.15))
        .unwrap();
    let controller = ControllerFrame {
        device_id: 7,
        activity_sequence: 1,
        axes: [0.10, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        ..ControllerFrame::default()
    };
    router
        .route(DeviceFrame {
            controllers: vec![controller.clone()],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().movement[0] > 0.0);

    router
        .route(DeviceFrame {
            controllers: vec![controller],
            ..DeviceFrame::default()
        })
        .unwrap();
    router
        .replace_bindings(settings_with_deadzones(0.15, 0.15))
        .unwrap();
    let replacement = router.finalize().unwrap();
    assert_eq!(replacement.movement, [0.0, 0.0]);
    assert_eq!(
        replacement.release_reasons[Action::MoveRight as usize],
        Some(ReleaseReason::BindingChanged)
    );

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 2,
                buttons: vec![0],
                axes: [0.10, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let button = router.finalize().unwrap();
    assert_eq!(button.input_mode, semantic_input::InputMode::GamePad);
    assert!(button.phases[Action::Jump as usize].pressed);
}

#[test]
fn held_reconnect_is_quarantined_after_more_than_four_controller_ids_churn() {
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
    router.finalize().unwrap();

    for (offset, device_id) in (8_u32..=12).enumerate() {
        let activity_sequence = offset as u64 + 2;
        router
            .route(DeviceFrame {
                controllers: vec![ControllerFrame {
                    device_id,
                    activity_sequence,
                    ..ControllerFrame::default()
                }],
                ..DeviceFrame::default()
            })
            .unwrap();
        router.finalize().unwrap();
        router
            .route(DeviceFrame {
                disconnected_controllers: vec![device_id],
                ..DeviceFrame::default()
            })
            .unwrap();
        router.finalize().unwrap();
    }

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 7,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let evicted_id_reconnect = router.finalize().unwrap();
    assert_eq!(
        evicted_id_reconnect.phases[Action::Jump as usize],
        Default::default()
    );
}

#[test]
fn repeated_max_controller_churn_is_bounded_and_cannot_leave_stale_input() {
    let mut router = SemanticInputRouter::default();
    let controllers = |generation: u32, activity_sequence: u64| {
        (0..MAX_CONTROLLERS)
            .map(|slot| ControllerFrame {
                device_id: generation * MAX_CONTROLLERS as u32 + slot as u32,
                activity_sequence,
                axes: [1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0],
                buttons: (0..32).collect(),
            })
            .collect::<Vec<_>>()
    };

    let mut previous_ids = Vec::new();
    for generation in 0..6_u32 {
        let current = controllers(generation, u64::from(generation) + 1);
        router
            .route(DeviceFrame {
                disconnected_controllers: previous_ids,
                controllers: current.clone(),
                ..DeviceFrame::default()
            })
            .unwrap();
        let snapshot = router.finalize().unwrap();
        assert!(snapshot.movement.iter().all(|axis| axis.is_finite()));
        assert!(snapshot.look_delta.iter().all(|axis| axis.is_finite()));
        if generation > 0 {
            assert_eq!(snapshot.movement, [0.0, 0.0]);
            assert_eq!(snapshot.look_delta, [0.0, 0.0]);
            assert!(snapshot.phases.iter().all(|phase| !phase.held));
        }
        previous_ids = current
            .iter()
            .map(|controller| controller.device_id)
            .collect();
    }

    let current_generation = 5_u32;
    let neutral = (0..MAX_CONTROLLERS)
        .map(|slot| ControllerFrame {
            device_id: current_generation * MAX_CONTROLLERS as u32 + slot as u32,
            activity_sequence: 7,
            ..ControllerFrame::default()
        })
        .collect::<Vec<_>>();
    router
        .route(DeviceFrame {
            controllers: neutral,
            ..DeviceFrame::default()
        })
        .unwrap();
    let neutral_snapshot = router.finalize().unwrap();
    assert_eq!(neutral_snapshot.movement, [0.0, 0.0]);
    assert!(neutral_snapshot.phases.iter().all(|phase| !phase.held));

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: current_generation * MAX_CONTROLLERS as u32,
                activity_sequence: 8,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Jump as usize].pressed);
}

#[test]
fn repeated_focus_loss_controller_churn_compacts_without_panicking_or_stale_input() {
    let mut router = SemanticInputRouter::default();
    let controllers = |generation: u32, activity_sequence: u64| {
        (0..MAX_CONTROLLERS)
            .map(|slot| ControllerFrame {
                device_id: generation * MAX_CONTROLLERS as u32 + slot as u32,
                activity_sequence,
                axes: [1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0],
                buttons: (0..32).collect(),
            })
            .collect::<Vec<_>>()
    };

    router
        .route(DeviceFrame {
            controllers: controllers(0, 1),
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();

    for generation in 1..=8_u32 {
        router
            .route(DeviceFrame {
                controllers: controllers(generation, u64::from(generation) + 1),
                window_focus_lost: true,
                ..DeviceFrame::default()
            })
            .unwrap();
        let snapshot = router.finalize().unwrap();
        assert_eq!(snapshot.movement, [0.0, 0.0]);
        assert_eq!(snapshot.look_delta, [0.0, 0.0]);
        assert!(snapshot.phases.iter().all(|phase| !phase.held));
    }

    router.route(DeviceFrame::default()).unwrap();
    let neutral = router.finalize().unwrap();
    assert_eq!(neutral.movement, [0.0, 0.0]);
    assert!(neutral.phases.iter().all(|phase| !phase.held));

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 99,
                activity_sequence: 10,
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
                device_id: 99,
                activity_sequence: 11,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 99,
                activity_sequence: 12,
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Jump as usize].pressed);
}
