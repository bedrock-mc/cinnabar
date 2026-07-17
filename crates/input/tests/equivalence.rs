use semantic_input::{
    Action, ControllerFrame, DeviceFrame, InputMode, KeyboardMouseFrame, SemanticInputRouter,
    TouchContact,
};

fn semantic_projection(snapshot: &semantic_input::ActionSnapshot) -> ([f32; 2], bool, bool) {
    (
        snapshot.movement,
        snapshot.phases[Action::Jump as usize].pressed,
        snapshot.phases[Action::Jump as usize].held,
    )
}

#[test]
fn keyboard_controller_and_touch_scripts_are_semantically_equivalent() {
    let keyboard = DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            activity_sequence: 1,
            keys: vec![0x1a, 0x2c], // W + Space
            ..KeyboardMouseFrame::default()
        }),
        ..DeviceFrame::default()
    };
    let controller = DeviceFrame {
        controllers: vec![ControllerFrame {
            device_id: 1,
            activity_sequence: 1,
            axes: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            buttons: vec![0], // south face
        }],
        ..DeviceFrame::default()
    };
    let touch = DeviceFrame {
        touches: vec![
            TouchContact {
                contact_id: 1,
                activity_sequence: 1,
                position: [0.25, 0.5],
                delta: [0.0, 0.0],
                hit_id: None,
            },
            TouchContact {
                contact_id: 2,
                activity_sequence: 1,
                position: [0.75, 0.75],
                delta: [0.0, 0.0],
                hit_id: Some(semantic_input::touch::JUMP),
            },
        ],
        ..DeviceFrame::default()
    };

    let mut projections = Vec::new();
    let mut modes = Vec::new();
    for frame in [keyboard, controller, touch] {
        let mut router = SemanticInputRouter::default();
        router.route(frame).unwrap();
        let snapshot = router.finalize().unwrap();
        projections.push(semantic_projection(&snapshot));
        modes.push(snapshot.input_mode);
    }
    assert_eq!(projections[0], projections[1]);
    assert_eq!(projections[1], projections[2]);
    assert_eq!(
        modes,
        vec![
            InputMode::KeyboardMouse,
            InputMode::GamePad,
            InputMode::Touch
        ]
    );
}

#[test]
fn radial_deadzone_preserves_direction_and_remaps_magnitude() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 1,
                activity_sequence: 1,
                axes: [0.3, 0.4, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        })
        .unwrap();
    let movement = router.finalize().unwrap().movement;
    assert!(movement[0] > 0.0 && movement[1] > 0.0);
    assert!((movement[0] / movement[1] - 0.75).abs() < 0.000_1);
    assert!(movement[0].hypot(movement[1]) < 0.5);
}
