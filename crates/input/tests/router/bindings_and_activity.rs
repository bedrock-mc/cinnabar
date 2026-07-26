#[test]
fn removing_look_bindings_disables_look_delta_and_direction_phases() {
    let bindings = ControlSettings::default()
        .bindings()
        .iter()
        .copied()
        .filter(|binding| {
            !matches!(
                binding.action,
                Action::LookUp | Action::LookDown | Action::LookLeft | Action::LookRight
            )
        })
        .collect();
    let settings = ControlSettings::new(bindings, 1.0, 1.0, 1.0, false, false, 0.15, 0.15).unwrap();
    let mut router = SemanticInputRouter::default();
    router.replace_bindings(settings).unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [8.0, -4.0],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.look_delta, [0.0, 0.0]);
    for action in [
        Action::LookUp,
        Action::LookDown,
        Action::LookLeft,
        Action::LookRight,
    ] {
        assert_eq!(snapshot.phases[action as usize], Default::default());
    }

    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 2,
                position: [0.75, 0.75],
                delta: [0.25, 0.0],
                hit_id: Some(semantic_input::touch::LOOK_RIGHT),
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let touch_snapshot = router.finalize().unwrap();
    assert_eq!(touch_snapshot.look_delta, [0.0, 0.0]);
}

#[test]
fn remapped_mouse_axis_controls_look_direction() {
    let mut bindings = ControlSettings::default().bindings().to_vec();
    let binding = bindings
        .iter_mut()
        .find(|binding| {
            binding.context == InputContext::Gameplay
                && binding.chord.control
                    == PhysicalControl::MouseAxis(semantic_input::MouseAxis::XPositive)
        })
        .unwrap();
    binding.action = Action::LookLeft;
    let settings = ControlSettings::new(bindings, 1.0, 1.0, 1.0, false, false, 0.15, 0.15).unwrap();
    let mut router = SemanticInputRouter::default();
    router.replace_bindings(settings).unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [8.0, 0.0],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.look_delta, [-8.0, 0.0]);
    assert!(snapshot.phases[Action::LookLeft as usize].held);
    assert!(!snapshot.phases[Action::LookRight as usize].held);
}

#[test]
fn opposing_mapped_look_controls_keep_both_digital_phases() {
    let settings = ControlSettings::new(
        vec![
            ActionBinding {
                action: Action::LookLeft,
                context: InputContext::Gameplay,
                chord: empty_chord(PhysicalControl::KeyboardUsage(0x04)),
            },
            ActionBinding {
                action: Action::LookRight,
                context: InputContext::Gameplay,
                chord: empty_chord(PhysicalControl::KeyboardUsage(0x07)),
            },
        ],
        1.0,
        1.0,
        1.0,
        false,
        false,
        0.1,
        0.1,
    )
    .unwrap();
    let mut router = SemanticInputRouter::default();
    router.replace_bindings(settings).unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                keys: vec![0x04, 0x07],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.look_delta, [0.0, 0.0]);
    assert!(snapshot.phases[Action::LookLeft as usize].held);
    assert!(snapshot.phases[Action::LookRight as usize].held);
}

#[test]
fn default_touch_drag_is_gated_by_typed_look_bindings() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.75, 0.75],
                delta: [0.25, 0.0],
                hit_id: Some(semantic_input::touch::LOOK_RIGHT),
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert!(snapshot.look_delta[0] > 0.0);
    assert!(snapshot.phases[Action::LookRight as usize].held);
}

#[test]
fn modified_keyboard_and_mouse_bindings_do_not_fire_unmodified() {
    let shift = ModifierChord {
        shift: true,
        ..ModifierChord::default()
    };
    let settings = ControlSettings::new(
        vec![
            ActionBinding {
                action: Action::Jump,
                context: InputContext::Gameplay,
                chord: InputChord {
                    control: PhysicalControl::KeyboardUsage(0x2c),
                    modifiers: shift,
                },
            },
            ActionBinding {
                action: Action::Attack,
                context: InputContext::Gameplay,
                chord: InputChord {
                    control: PhysicalControl::MouseButton(1),
                    modifiers: shift,
                },
            },
            ActionBinding {
                action: Action::LookRight,
                context: InputContext::Gameplay,
                chord: InputChord {
                    control: PhysicalControl::MouseAxis(semantic_input::MouseAxis::XPositive),
                    modifiers: shift,
                },
            },
        ],
        1.0,
        1.0,
        1.0,
        false,
        false,
        0.1,
        0.1,
    )
    .unwrap();
    let mut router = SemanticInputRouter::default();
    router.replace_bindings(settings).unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                keys: vec![0x2c],
                mouse_buttons: vec![1],
                mouse_motion: [4.0, 0.0],
                modifiers: ModifierChord::default(),
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert!(!snapshot.phases[Action::Jump as usize].pressed);
    assert!(!snapshot.phases[Action::Attack as usize].held);
    assert_eq!(snapshot.look_delta, [0.0, 0.0]);
}

#[test]
fn gamepad_and_touch_bindings_reject_unsupported_modifiers() {
    let modified = ModifierChord {
        control: true,
        ..ModifierChord::default()
    };
    for control in [
        PhysicalControl::GamepadButton(0),
        PhysicalControl::GamepadAxis {
            axis: 0,
            direction: AxisDirection::Positive,
        },
        PhysicalControl::TouchControl(semantic_input::touch::JUMP),
    ] {
        assert_eq!(
            ControlSettings::new(
                vec![ActionBinding {
                    action: Action::Jump,
                    context: InputContext::Gameplay,
                    chord: InputChord {
                        control,
                        modifiers: modified,
                    },
                }],
                1.0,
                1.0,
                1.0,
                false,
                false,
                0.1,
                0.1,
            ),
            Err(BindingError::UnknownPhysicalCode)
        );
    }
}

#[test]
fn held_escape_does_not_retrigger_a_one_shot_after_context_change() {
    let mut router = SemanticInputRouter::default();
    let held_escape = KeyboardMouseFrame {
        activity_sequence: 1,
        keys: vec![0x29],
        ..KeyboardMouseFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(held_escape.clone()),
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Menu as usize].pressed);

    router.set_context(InputContext::UiFocused);
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(held_escape),
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(!router.finalize().unwrap().phases[Action::Back as usize].pressed);

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
                keys: vec![0x29],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Back as usize].pressed);
}

#[test]
fn previewed_escape_edge_is_not_reassigned_after_same_frame_context_change() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                keys: vec![0x29],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.preview_ui_phase(Action::Menu).unwrap().pressed);
    router.set_context(InputContext::UiFocused);
    let snapshot = router.finalize().unwrap();
    assert!(!snapshot.phases[Action::Back as usize].pressed);
}

#[test]
fn activity_sequences_cannot_move_backward_within_or_across_sources() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 10,
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();

    assert_eq!(
        router.route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 9,
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        }),
        Err(RouterError::NonMonotonicActivitySequence {
            previous: 10,
            actual: 9,
        })
    );
    assert_eq!(
        router.route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 9,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        }),
        Err(RouterError::NonMonotonicActivitySequence {
            previous: 10,
            actual: 9,
        })
    );
}

#[test]
fn touch_look_uses_matching_typed_layout_entry_and_supports_remapping() {
    for (action, expected_x) in [(Action::LookRight, 512.0), (Action::LookLeft, -512.0)] {
        let layout = TouchControlLayout::new(vec![TouchControl {
            hit_id: 55,
            kind: TouchControlKind::LookAxis(TouchAxis::XPositive),
        }])
        .unwrap();
        let settings = ControlSettings::new_with_touch_layout(
            vec![ActionBinding {
                action,
                context: InputContext::Gameplay,
                chord: empty_chord(PhysicalControl::TouchControl(55)),
            }],
            1.0,
            1.0,
            1.0,
            false,
            false,
            0.1,
            0.1,
            &layout,
        )
        .unwrap();
        let mut router =
            SemanticInputRouter::with_settings_and_touch_layout(settings, layout).unwrap();
        router
            .route(DeviceFrame {
                touches: vec![semantic_input::TouchContact {
                    contact_id: 1,
                    activity_sequence: 1,
                    position: [0.75, 0.75],
                    delta: [0.25, 0.0],
                    hit_id: Some(55),
                }],
                ..DeviceFrame::default()
            })
            .unwrap();
        let snapshot = router.finalize().unwrap();
        assert_eq!(snapshot.look_delta, [expected_x, 0.0]);
        assert!(snapshot.phases[action as usize].held);
    }
}

#[test]
fn unrelated_touch_drag_and_removed_touch_binding_cannot_bypass_mapping() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.75, 0.75],
                delta: [0.25, 0.0],
                hit_id: None,
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().look_delta, [0.0, 0.0]);

    let bindings = ControlSettings::default()
        .bindings()
        .iter()
        .copied()
        .filter(|binding| {
            binding.chord.control
                != PhysicalControl::TouchControl(semantic_input::touch::LOOK_RIGHT)
        })
        .collect();
    let settings = ControlSettings::new(bindings, 1.0, 1.0, 1.0, false, false, 0.1, 0.1).unwrap();
    let mut router = SemanticInputRouter::default();
    router.replace_bindings(settings).unwrap();
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.75, 0.75],
                delta: [0.25, 0.0],
                hit_id: Some(semantic_input::touch::LOOK_RIGHT),
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert_eq!(router.finalize().unwrap().look_delta, [0.0, 0.0]);
}

#[test]
fn changed_source_must_advance_past_global_watermark_without_mutating_state() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 10,
                ..KeyboardMouseFrame::default()
            }),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 1,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let first = router.finalize().unwrap();
    assert_eq!(first.input_mode, semantic_input::InputMode::KeyboardMouse);

    assert_eq!(
        router.route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 2,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        }),
        Err(RouterError::NonMonotonicActivitySequence {
            previous: 10,
            actual: 2,
        })
    );

    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 11,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let accepted = router.finalize().unwrap();
    assert_eq!(accepted.frame_sequence, 2);
    assert_eq!(accepted.input_mode, semantic_input::InputMode::GamePad);
}

#[test]
fn global_watermark_rules_cover_keyboard_controller_and_touch_sources() {
    let keyboard = |activity_sequence| DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            activity_sequence,
            ..KeyboardMouseFrame::default()
        }),
        ..DeviceFrame::default()
    };
    assert_global_activity_contract(
        DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                ..KeyboardMouseFrame::default()
            }),
            controllers: vec![ControllerFrame {
                device_id: 99,
                activity_sequence: 10,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        },
        keyboard,
    );

    let controller = |activity_sequence| DeviceFrame {
        controllers: vec![ControllerFrame {
            device_id: 7,
            activity_sequence,
            ..ControllerFrame::default()
        }],
        ..DeviceFrame::default()
    };
    assert_global_activity_contract(
        DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 10,
                ..KeyboardMouseFrame::default()
            }),
            controllers: vec![ControllerFrame {
                device_id: 7,
                activity_sequence: 1,
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        },
        controller,
    );

    let touch = |activity_sequence| DeviceFrame {
        touches: vec![semantic_input::TouchContact {
            contact_id: 5,
            activity_sequence,
            position: [0.25, 0.25],
            delta: [0.0, 0.0],
            hit_id: None,
        }],
        ..DeviceFrame::default()
    };
    assert_global_activity_contract(
        DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 10,
                ..KeyboardMouseFrame::default()
            }),
            touches: vec![semantic_input::TouchContact {
                contact_id: 5,
                activity_sequence: 1,
                position: [0.25, 0.25],
                delta: [0.0, 0.0],
                hit_id: None,
            }],
            ..DeviceFrame::default()
        },
        touch,
    );
}
