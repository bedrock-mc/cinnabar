use core::num::NonZeroU64;

use semantic_input::{
    Action, ActionBinding, AxisDirection, BindingError, ControlSettings, ControllerFrame,
    DeviceFrame, FrameError, InputChord, InputContext, KeyboardMouseFrame, MAX_CONTROLLERS,
    MAX_LOOK_DELTA_PER_FRAME, MAX_TOUCH_CONTROLS, ModifierChord, PhysicalControl, ReleaseReason,
    RouterError, SemanticInputRouter, TouchAxis, TouchControl, TouchControlKind,
    TouchControlLayout, TouchLayoutError,
};

fn empty_chord(control: PhysicalControl) -> InputChord {
    InputChord {
        control,
        modifiers: ModifierChord::default(),
    }
}

fn touch_button(hit_id: u16) -> TouchControl {
    TouchControl {
        hit_id,
        kind: TouchControlKind::Button,
    }
}

fn assert_global_activity_contract(seed: DeviceFrame, source: fn(u64) -> DeviceFrame) {
    let mut router = SemanticInputRouter::default();
    router.route(seed).unwrap();
    router.finalize().unwrap();
    router.route(source(1)).unwrap();
    router.finalize().unwrap();
    for actual in [2, 10] {
        assert_eq!(
            router.route(source(actual)),
            Err(RouterError::NonMonotonicActivitySequence {
                previous: 10,
                actual,
            })
        );
    }
    router.route(source(11)).unwrap();
    assert_eq!(router.finalize().unwrap().frame_sequence, 3);
}

#[test]
fn keyboard_jump_has_physical_edges_and_held_state() {
    let mut router = SemanticInputRouter::default();
    let mut keyboard = KeyboardMouseFrame {
        activity_sequence: 1,
        ..KeyboardMouseFrame::default()
    };
    keyboard.keys.push(0x2c); // USB HID Space

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            ..DeviceFrame::default()
        })
        .unwrap();
    let pressed = router.finalize().unwrap();
    assert!(pressed.phases[Action::Jump as usize].pressed);
    assert!(pressed.phases[Action::Jump as usize].held);
    assert!(!pressed.phases[Action::Jump as usize].released);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            ..DeviceFrame::default()
        })
        .unwrap();
    let repeated = router.finalize().unwrap();
    assert!(!repeated.phases[Action::Jump as usize].pressed);
    assert!(repeated.phases[Action::Jump as usize].held);

    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::Jump as usize].released);
    assert!(!released.phases[Action::Jump as usize].held);
}

#[test]
fn route_and_finalize_enforce_exactly_one_pending_frame_transactionally() {
    let mut router = SemanticInputRouter::default();
    assert_eq!(router.finalize(), Err(RouterError::MissingPendingFrame));

    router.route(DeviceFrame::default()).unwrap();
    assert_eq!(
        router.route(DeviceFrame::default()),
        Err(RouterError::PendingFrameAlreadyRouted)
    );
    let first = router.finalize().unwrap();
    assert_eq!(first.frame_sequence, 1);
    assert_eq!(router.finalize(), Err(RouterError::MissingPendingFrame));

    router.route(DeviceFrame::default()).unwrap();
    assert_eq!(router.finalize().unwrap().frame_sequence, 2);
}

#[test]
fn malformed_device_frames_are_rejected_without_consuming_the_slot() {
    let mut router = SemanticInputRouter::default();
    let controllers = (0..=MAX_CONTROLLERS)
        .map(|device_id| ControllerFrame {
            device_id: device_id as u32,
            ..ControllerFrame::default()
        })
        .collect();
    assert_eq!(
        router.route(DeviceFrame {
            controllers,
            ..DeviceFrame::default()
        }),
        Err(RouterError::InvalidFrame(FrameError::TooManyControllers {
            actual: MAX_CONTROLLERS + 1,
            maximum: MAX_CONTROLLERS,
        }))
    );

    let duplicate = ControllerFrame {
        device_id: 7,
        ..ControllerFrame::default()
    };
    assert_eq!(
        router.route(DeviceFrame {
            controllers: vec![duplicate.clone(), duplicate],
            ..DeviceFrame::default()
        }),
        Err(RouterError::InvalidFrame(FrameError::DuplicateDeviceId(7)))
    );

    router.route(DeviceFrame::default()).unwrap();
    assert_eq!(router.finalize().unwrap().frame_sequence, 1);
}

#[test]
fn axes_are_finite_and_bounded_without_erasing_analogue_magnitude() {
    let mut router = SemanticInputRouter::default();
    let controller = ControllerFrame {
        device_id: 1,
        activity_sequence: 1,
        axes: [0.6, 0.8, 1_000_000.0, -1_000_000.0, 0.0, 0.0, 0.0, 0.0],
        ..ControllerFrame::default()
    };
    router
        .route(DeviceFrame {
            controllers: vec![controller],
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert!((snapshot.movement[0] - 0.6).abs() < 0.000_001);
    assert!((snapshot.movement[1] - 0.8).abs() < 0.000_001);
    assert!(snapshot.look_delta.iter().all(|value| value.is_finite()));
    assert!(snapshot.look_delta[0].hypot(snapshot.look_delta[1]) <= MAX_LOOK_DELTA_PER_FRAME);
}

#[test]
fn extreme_finite_stick_axes_clamp_without_collapsing_to_zero() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 1,
                activity_sequence: 1,
                axes: [f32::MAX, f32::MAX, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let movement = router.finalize().unwrap().movement;
    assert!(movement.iter().all(|axis| axis.is_finite()));
    assert!((movement[0].hypot(movement[1]) - 1.0).abs() < 0.000_001);
}

#[test]
fn extreme_finite_mouse_motion_clamps_without_collapsing_to_zero() {
    let mut router = SemanticInputRouter::default();
    let mut settings = ControlSettings::default();
    settings.mouse_sensitivity = 10.0;
    router.replace_bindings(settings).unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [f32::MAX, 0.0],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let look = router.finalize().unwrap().look_delta;
    assert!(look.iter().all(|axis| axis.is_finite()));
    assert_eq!(look, [MAX_LOOK_DELTA_PER_FRAME, 0.0]);
}

#[test]
fn extreme_finite_touch_drag_clamps_without_non_finite_output() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.75, 0.5],
                delta: [f32::MAX, 0.0],
                hit_id: Some(semantic_input::touch::LOOK_RIGHT),
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let look = router.finalize().unwrap().look_delta;
    assert!(look.iter().all(|axis| axis.is_finite()));
    assert_eq!(look, [MAX_LOOK_DELTA_PER_FRAME, 0.0]);
}

#[test]
fn ui_preview_is_read_only_and_rejects_gameplay_actions() {
    let mut router = SemanticInputRouter::default();
    let mut keyboard = KeyboardMouseFrame {
        activity_sequence: 1,
        ..KeyboardMouseFrame::default()
    };
    keyboard.keys.push(0x29); // Escape
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            ..DeviceFrame::default()
        })
        .unwrap();

    let first = router.preview_ui_phase(Action::Menu).unwrap();
    let second = router.preview_ui_phase(Action::Menu).unwrap();
    assert_eq!(first, second);
    assert!(first.pressed);
    assert_eq!(
        router.preview_ui_phase(Action::Jump),
        Err(RouterError::GameplayActionPreview(Action::Jump))
    );
    assert!(router.finalize().unwrap().phases[Action::Menu as usize].pressed);
}

#[test]
fn release_reasons_use_the_documented_priority_and_finalize_barrier() {
    let mut router = SemanticInputRouter::default();
    let mut keyboard = KeyboardMouseFrame {
        activity_sequence: 1,
        ..KeyboardMouseFrame::default()
    };
    keyboard.keys.push(0x2c);
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();

    router.release_all(ReleaseReason::WindowFocusLost);
    router.release_all(ReleaseReason::AuthorityChanged);
    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert_eq!(
        released.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::AuthorityChanged)
    );
    assert!(released.phases[Action::Jump as usize].released);
}

#[test]
fn authority_and_context_changes_publish_only_at_finalize() {
    let mut router = SemanticInputRouter::default();
    let generation = NonZeroU64::new(9).unwrap();
    router.replace_authority(generation);
    router.set_context(InputContext::UiFocused);
    router.route(DeviceFrame::default()).unwrap();
    let snapshot = router.finalize().unwrap();
    assert_eq!(snapshot.authority_generation, generation);
}

#[test]
fn settings_validation_is_bounded_and_atomic() {
    let chord = empty_chord(PhysicalControl::KeyboardUsage(0x04));
    let bindings = vec![
        ActionBinding {
            action: Action::MoveLeft,
            context: InputContext::Gameplay,
            chord,
        },
        ActionBinding {
            action: Action::MoveRight,
            context: InputContext::Gameplay,
            chord,
        },
    ];
    assert_eq!(
        ControlSettings::new(bindings, 1.0, 1.0, 1.0, false, false, 0.1, 0.1),
        Err(BindingError::Conflict {
            context: InputContext::Gameplay,
            chord,
            first: Action::MoveLeft,
            second: Action::MoveRight,
        })
    );

    assert!(matches!(
        ControlSettings::new(
            vec![ActionBinding {
                action: Action::Jump,
                context: InputContext::Gameplay,
                chord: empty_chord(PhysicalControl::GamepadAxis {
                    axis: 8,
                    direction: AxisDirection::Positive,
                }),
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
    ));
}

#[test]
fn ui_navigation_is_one_shot_and_does_not_retrigger_while_held() {
    let mut router = SemanticInputRouter::default();
    router.set_context(InputContext::UiFocused);
    let keyboard = KeyboardMouseFrame {
        activity_sequence: 1,
        keys: vec![0x52], // Up arrow
        ..KeyboardMouseFrame::default()
    };
    for expected_pressed in [true, false] {
        router
            .route(DeviceFrame {
                keyboard_mouse: Some(keyboard.clone()),
                ..DeviceFrame::default()
            })
            .unwrap();
        let phase = router.finalize().unwrap().phases[Action::UiUp as usize];
        assert_eq!(phase.pressed, expected_pressed);
        assert!(!phase.held);
        assert!(!phase.released);
    }
}

#[test]
fn controller_disconnect_does_not_release_keyboard_owned_actions() {
    let mut router = SemanticInputRouter::default();
    let keyboard = KeyboardMouseFrame {
        activity_sequence: 2,
        keys: vec![0x2c],
        ..KeyboardMouseFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            disconnected_controllers: vec![7],
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert!(snapshot.phases[Action::Jump as usize].held);
    assert!(!snapshot.phases[Action::Jump as usize].released);
    assert_eq!(snapshot.release_reasons[Action::Jump as usize], None);
}

#[test]
fn inverted_look_has_one_semantic_direction() {
    let settings = ControlSettings::new(
        ControlSettings::default().bindings().to_vec(),
        1.0,
        1.0,
        1.0,
        true,
        false,
        0.15,
        0.15,
    )
    .unwrap();
    let mut router = SemanticInputRouter::default();
    router.replace_bindings(settings).unwrap();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_motion: [0.0, 4.0],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert!(snapshot.phases[Action::LookUp as usize].held);
    assert!(!snapshot.phases[Action::LookDown as usize].held);
}

#[test]
fn binding_replacement_revalidates_public_numeric_fields_atomically() {
    let mut router = SemanticInputRouter::default();
    let keyboard = KeyboardMouseFrame {
        activity_sequence: 1,
        keys: vec![0x2c],
        ..KeyboardMouseFrame::default()
    };
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            ..DeviceFrame::default()
        })
        .unwrap();
    router.finalize().unwrap();

    let mut invalid = ControlSettings::default();
    invalid.mouse_sensitivity = f32::NAN;
    assert_eq!(
        router.replace_bindings(invalid),
        Err(BindingError::NonFiniteSensitivity)
    );

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard),
            ..DeviceFrame::default()
        })
        .unwrap();
    let snapshot = router.finalize().unwrap();
    assert!(snapshot.phases[Action::Jump as usize].held);
    assert!(!snapshot.phases[Action::Jump as usize].released);
    assert_eq!(snapshot.release_reasons[Action::Jump as usize], None);
}

#[test]
fn keyboard_key_samples_are_domain_bounded_and_unique() {
    let mut router = SemanticInputRouter::default();
    let oversized = (0..229).map(|index| 0x04 + (index % 228) as u16).collect();
    assert!(matches!(
        router.route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                keys: oversized,
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        }),
        Err(RouterError::InvalidFrame(_))
    ));
    assert!(matches!(
        router.route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                keys: vec![0x04, 0x04],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        }),
        Err(RouterError::InvalidFrame(_))
    ));
}

#[test]
fn mouse_button_samples_are_domain_bounded_and_unique() {
    let mut router = SemanticInputRouter::default();
    for buttons in [(0..9).map(|index| 1 + index % 8).collect(), vec![1, 1]] {
        assert!(matches!(
            router.route(DeviceFrame {
                keyboard_mouse: Some(KeyboardMouseFrame {
                    mouse_buttons: buttons,
                    ..KeyboardMouseFrame::default()
                }),
                ..DeviceFrame::default()
            }),
            Err(RouterError::InvalidFrame(_))
        ));
    }
}

#[test]
fn controller_button_samples_are_domain_bounded_and_unique() {
    let mut router = SemanticInputRouter::default();
    for buttons in [(0..33).map(|index| index % 32).collect(), vec![0, 0]] {
        assert!(matches!(
            router.route(DeviceFrame {
                controllers: vec![ControllerFrame {
                    device_id: 1,
                    buttons,
                    ..ControllerFrame::default()
                }],
                ..DeviceFrame::default()
            }),
            Err(RouterError::InvalidFrame(_))
        ));
    }
}

#[test]
fn disconnected_controller_samples_are_slot_bounded_and_unique() {
    let mut router = SemanticInputRouter::default();
    for disconnected_controllers in [vec![0, 1, 2, 3, 4], vec![7, 7]] {
        assert!(matches!(
            router.route(DeviceFrame {
                disconnected_controllers,
                ..DeviceFrame::default()
            }),
            Err(RouterError::InvalidFrame(_))
        ));
    }
    assert!(matches!(
        router.route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 7,
                ..ControllerFrame::default()
            }],
            disconnected_controllers: vec![7],
            ..DeviceFrame::default()
        }),
        Err(RouterError::InvalidFrame(_))
    ));
}

#[test]
fn touch_layout_is_bounded_nonzero_and_unique() {
    assert_eq!(
        TouchControlLayout::new(
            (1..=(MAX_TOUCH_CONTROLS + 1) as u16)
                .map(touch_button)
                .collect()
        ),
        Err(TouchLayoutError::TooManyControls {
            actual: MAX_TOUCH_CONTROLS + 1,
            maximum: MAX_TOUCH_CONTROLS,
        })
    );
    assert_eq!(
        TouchControlLayout::new(vec![touch_button(0)]),
        Err(TouchLayoutError::ZeroControlId)
    );
    assert_eq!(
        TouchControlLayout::new(vec![touch_button(41), touch_button(41)]),
        Err(TouchLayoutError::DuplicateControlId(41))
    );
}

#[test]
fn default_layout_rejects_arbitrary_touch_binding_and_frame_ids() {
    let arbitrary = ActionBinding {
        action: Action::Jump,
        context: InputContext::Gameplay,
        chord: empty_chord(PhysicalControl::TouchControl(999)),
    };
    assert_eq!(
        ControlSettings::new(vec![arbitrary], 1.0, 1.0, 1.0, false, false, 0.1, 0.1),
        Err(BindingError::UnknownPhysicalCode)
    );

    let mut router = SemanticInputRouter::default();
    assert!(matches!(
        router.route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.75, 0.75],
                delta: [0.0, 0.0],
                hit_id: Some(999),
            }],
            ..DeviceFrame::default()
        }),
        Err(RouterError::InvalidFrame(FrameError::UnknownTouchControl(
            999
        )))
    ));
}

#[test]
fn custom_touch_layout_is_shared_by_settings_and_frame_validation() {
    let layout = TouchControlLayout::new(vec![touch_button(55)]).unwrap();
    let settings = ControlSettings::new_with_touch_layout(
        vec![ActionBinding {
            action: Action::Jump,
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
    let mut router = SemanticInputRouter::with_settings_and_touch_layout(settings, layout).unwrap();
    router
        .route(DeviceFrame {
            touches: vec![semantic_input::TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.75, 0.75],
                delta: [0.0, 0.0],
                hit_id: Some(55),
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::Jump as usize].pressed);
}

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
