// Player-list held-action bindings: press/hold/release across every device
// family plus context disjointness with the UI Tab bindings.

fn keyboard_tab() -> KeyboardMouseFrame {
    KeyboardMouseFrame {
        activity_sequence: 1,
        keys: vec![0x2b], // USB HID Tab
        ..KeyboardMouseFrame::default()
    }
}

#[test]
fn keyboard_tab_holds_the_player_list_through_the_full_phase_cycle() {
    let mut router = SemanticInputRouter::default();

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard_tab()),
            ..DeviceFrame::default()
        })
        .unwrap();
    let pressed = router.finalize().unwrap();
    assert!(pressed.phases[Action::PlayerList as usize].pressed);
    assert!(pressed.phases[Action::PlayerList as usize].held);
    assert!(!pressed.phases[Action::PlayerList as usize].released);

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard_tab()),
            ..DeviceFrame::default()
        })
        .unwrap();
    let repeated = router.finalize().unwrap();
    assert!(!repeated.phases[Action::PlayerList as usize].pressed);
    assert!(repeated.phases[Action::PlayerList as usize].held);

    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::PlayerList as usize].released);
    assert!(!released.phases[Action::PlayerList as usize].held);
}

#[test]
fn gamepad_right_thumb_holds_the_player_list_with_clean_release_reasons() {
    let mut router = SemanticInputRouter::default();
    let controller = || ControllerFrame {
        device_id: 1,
        activity_sequence: 1,
        buttons: vec![9], // RightThumb
        ..ControllerFrame::default()
    };

    router
        .route(DeviceFrame {
            controllers: vec![controller()],
            ..DeviceFrame::default()
        })
        .unwrap();
    let pressed = router.finalize().unwrap();
    assert!(pressed.phases[Action::PlayerList as usize].pressed);
    assert!(pressed.phases[Action::PlayerList as usize].held);
    assert_eq!(
        pressed.release_reasons[Action::PlayerList as usize],
        None
    );

    router
        .route(DeviceFrame {
            controllers: vec![controller()],
            ..DeviceFrame::default()
        })
        .unwrap();
    let repeated = router.finalize().unwrap();
    assert!(repeated.phases[Action::PlayerList as usize].held);
    assert!(!repeated.phases[Action::PlayerList as usize].pressed);

    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::PlayerList as usize].released);
    assert!(!released.phases[Action::PlayerList as usize].held);
}

#[test]
fn touch_player_list_control_holds_and_releases() {
    let mut router = SemanticInputRouter::default();
    let contact = || semantic_input::TouchContact {
        contact_id: 1,
        activity_sequence: 1,
        position: [0.5, 0.5],
        delta: [0.0, 0.0],
        hit_id: Some(semantic_input::touch::PLAYER_LIST),
    };

    router
        .route(DeviceFrame {
            touches: vec![contact()],
            ..DeviceFrame::default()
        })
        .unwrap();
    let pressed = router.finalize().unwrap();
    assert!(pressed.phases[Action::PlayerList as usize].pressed);
    assert!(pressed.phases[Action::PlayerList as usize].held);

    router
        .route(DeviceFrame {
            touches: vec![contact()],
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::PlayerList as usize].held);

    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::PlayerList as usize].released);
    assert!(!released.phases[Action::PlayerList as usize].held);
}

#[test]
fn tab_resolves_by_context_player_list_in_gameplay_and_ui_tab_next_when_focused() {
    let mut router = SemanticInputRouter::default();

    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard_tab()),
            ..DeviceFrame::default()
        })
        .unwrap();
    let gameplay = router.finalize().unwrap();
    assert!(gameplay.phases[Action::PlayerList as usize].held);
    assert!(!gameplay.phases[Action::UiTabNext as usize].pressed);

    let mut focused = SemanticInputRouter::default();
    focused.set_context(InputContext::UiFocused);
    focused
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard_tab()),
            ..DeviceFrame::default()
        })
        .unwrap();
    let ui = focused.finalize().unwrap();
    assert!(ui.phases[Action::UiTabNext as usize].pressed);
    assert!(!ui.phases[Action::PlayerList as usize].held);
}

#[test]
fn focus_loss_releases_a_held_player_list_with_a_documented_reason() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            keyboard_mouse: Some(keyboard_tab()),
            ..DeviceFrame::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[Action::PlayerList as usize].held);

    router.release_all(ReleaseReason::WindowFocusLost);
    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::PlayerList as usize].released);
    assert_eq!(
        released.release_reasons[Action::PlayerList as usize],
        Some(ReleaseReason::WindowFocusLost)
    );
}
