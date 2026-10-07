/// A complete physical tap retains both edges without pretending the button is still held.
#[test]
fn a_subframe_jump_tap_preserves_both_raw_edges() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                key_edges: semantic_input::ButtonEdges {
                    pressed: vec![0x2c],
                    released: vec![0x2c],
                },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let input = router.finalize().unwrap();
    let jump = input.phases[Action::Jump as usize];
    assert!(jump.pressed && jump.released);
    assert!(!jump.held);
}

/// Opposing and diagonal keyboard buttons remain independent of their normalized vector.
#[test]
fn digital_directions_survive_vector_cancellation() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                keys: vec![0x1a, 0x16, 0x07],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let input = router.finalize().unwrap();
    assert_eq!(input.movement, [1.0, 0.0]);
    assert!(
        input.movement_buttons.forward
            && input.movement_buttons.backward
            && input.movement_buttons.right
    );
    assert!(!input.movement_buttons.left);
}

/// Analogue stick movement never fabricates a digital direction button.
#[test]
fn analogue_movement_has_no_digital_directions() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                activity_sequence: 1,
                axes: [1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    let input = router.finalize().unwrap();
    assert!(input.movement[0] > 0.0 && input.movement[1] > 0.0);
    assert_eq!(
        input.movement_buttons,
        semantic_input::MovementButtons::default()
    );
}

/// A controller tap ending within one frame can select its device and publish both edges.
#[test]
fn a_subframe_controller_tap_preserves_both_raw_edges() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                activity_sequence: 1,
                button_edges: semantic_input::ButtonEdges {
                    pressed: vec![0],
                    released: vec![0],
                },
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    let input = router.finalize().unwrap();
    assert_eq!(input.input_mode, semantic_input::InputMode::GamePad);
    let jump = input.phases[Action::Jump as usize];
    assert!(jump.pressed && jump.released);
    assert!(!jump.held);
}

/// A completed tap is already released and cannot quarantine a new authority's fresh press.
#[test]
fn finalized_tap_does_not_quarantine_the_next_authority_press() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                key_edges: semantic_input::ButtonEdges {
                    pressed: vec![0x2c],
                    released: vec![0x2c],
                },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let tap = router.finalize().unwrap();
    assert!(tap.phases[Action::Jump as usize].released);
    router.replace_authority(NonZeroU64::new(2).unwrap());
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 2,
                keys: vec![0x2c],
                key_edges: semantic_input::ButtonEdges {
                    pressed: vec![0x2c],
                    released: vec![],
                },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let next = router.finalize().unwrap();
    assert_eq!(next.authority_generation, NonZeroU64::new(2).unwrap());
    assert!(next.phases[Action::Jump as usize].pressed);
    assert!(next.phases[Action::Jump as usize].held);
}

/// A pending tap belongs to the old authority and must not leak into its replacement.
#[test]
fn pending_tap_is_quarantined_when_authority_changes_before_finalize() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                activity_sequence: 1,
                key_edges: semantic_input::ButtonEdges {
                    pressed: vec![0x2c],
                    released: vec![0x2c],
                },
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    router.replace_authority(NonZeroU64::new(2).unwrap());
    let next = router.finalize().unwrap();
    assert_eq!(next.phases[Action::Jump as usize], Default::default());
}
