#[test]
fn consuming_a_control_preserves_other_input_and_rearms_after_release() {
    for control in [
        PhysicalControl::KeyboardUsage(0x2c),
        PhysicalControl::MouseButton(2),
    ] {
        let action = match control {
            PhysicalControl::KeyboardUsage(_) => Action::Jump,
            _ => Action::Use,
        };
        let mut router = SemanticInputRouter::default();
        let keyboard = KeyboardMouseFrame {
            activity_sequence: 1,
            keys: vec![0x1a, 0x2c],
            mouse_buttons: vec![1, 2],
            mouse_motion: [3.0, 2.0],
            ..Default::default()
        };
        let mut frame = DeviceFrame {
            keyboard_mouse: Some(keyboard.clone()),
            ..Default::default()
        };
        router.route(frame.clone()).unwrap();
        router.consume_control(control);
        for _ in 0..2 {
            let sample = router.finalize().unwrap();
            assert_eq!(sample.movement, [0.0, 1.0]);
            assert!(sample.phases[Action::Attack as usize].held);
            assert!(!sample.phases[action as usize].held);
            assert!(!sample.phases[action as usize].pressed);
            assert_ne!(sample.look_delta, [0.0; 2]);
            router.route(frame.clone()).unwrap();
        }
        router.finalize().unwrap();
        let mut released = keyboard;
        released.activity_sequence = 2;
        match control {
            PhysicalControl::KeyboardUsage(code) => released.keys.retain(|key| *key != code),
            PhysicalControl::MouseButton(code) => {
                released.mouse_buttons.retain(|button| *button != code)
            }
            _ => unreachable!(),
        }
        router
            .route(DeviceFrame {
                keyboard_mouse: Some(released),
                ..Default::default()
            })
            .unwrap();
        router.finalize().unwrap();
        frame.keyboard_mouse.as_mut().unwrap().activity_sequence = 3;
        router.route(frame).unwrap();
        let sample = router.finalize().unwrap();
        assert!(sample.phases[action as usize].pressed);
        assert!(sample.phases[action as usize].held);
    }
}
