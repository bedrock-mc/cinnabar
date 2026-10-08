#[test]
fn tab_roster_is_momentary_gameplay_input_and_preserves_ui_navigation() {
    let mut router = SemanticInputRouter::default();
    let tab = |activity_sequence| DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            activity_sequence,
            keys: vec![0x2b],
            ..Default::default()
        }),
        ..Default::default()
    };
    router.route(tab(1)).unwrap();
    let first = router.finalize().unwrap();
    assert!(first.phases[Action::PlayerList as usize].pressed);
    assert!(first.phases[Action::PlayerList as usize].held);
    assert!(!first.phases[Action::UiTabNext as usize].held);
    router.route(tab(1)).unwrap();
    let held = router.finalize().unwrap();
    assert!(held.phases[Action::PlayerList as usize].held);
    assert!(!held.phases[Action::PlayerList as usize].pressed);
    router.route(DeviceFrame::default()).unwrap();
    let released = router.finalize().unwrap();
    assert!(released.phases[Action::PlayerList as usize].released);
    assert!(!released.phases[Action::PlayerList as usize].held);
    router.set_context(InputContext::UiFocused);
    router.route(tab(2)).unwrap();
    let menu = router.finalize().unwrap();
    assert!(!menu.phases[Action::PlayerList as usize].held);
    assert!(menu.phases[Action::UiTabNext as usize].pressed);
    router.set_context(InputContext::Gameplay);
    router.route(DeviceFrame::default()).unwrap();
    router.finalize().unwrap();
    router.route(tab(3)).unwrap();
    router.finalize().unwrap();
    router
        .route(DeviceFrame {
            window_focus_lost: true,
            ..Default::default()
        })
        .unwrap();
    assert!(!router.finalize().unwrap().phases[Action::PlayerList as usize].held);
}
