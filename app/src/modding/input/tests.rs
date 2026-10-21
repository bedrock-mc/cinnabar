use super::*;

#[test]
fn packet_pointer_interruptions_emit_one_cancel_and_require_a_fresh_press() {
    for (open, focused, absorbed, cursor) in [
        (false, true, false, true),
        (true, false, false, true),
        (true, true, true, true),
        (true, true, false, false),
    ] {
        let mut physical = PhysicalControls {
            left_held: true,
            ..Default::default()
        };
        assert_eq!(
            physical.packet_pointer_sample(true, true, false, true, true),
            (true, false)
        );
        assert_eq!(
            physical.packet_pointer_sample(true, true, false, true, false),
            (true, false)
        );
        assert_eq!(
            physical.packet_pointer_sample(open, focused, absorbed, cursor, false),
            (false, true)
        );
        assert_eq!(
            physical.packet_pointer_sample(open, focused, absorbed, cursor, false),
            (false, false),
            "an interruption is an edge"
        );
        assert_eq!(
            physical.packet_pointer_sample(true, true, false, true, false),
            (false, false),
            "holding across refocus or re-entry cannot resume capture"
        );
        assert_eq!(
            physical.packet_pointer_sample(true, true, false, true, true),
            (true, false)
        );
    }
}

#[test]
fn packet_pointer_release_is_a_commit_sample_instead_of_a_cancellation() {
    let mut physical = PhysicalControls {
        left_held: true,
        ..Default::default()
    };
    assert_eq!(
        physical.packet_pointer_sample(true, true, false, true, true),
        (true, false)
    );
    physical.left_held = false;
    assert_eq!(
        physical.packet_pointer_sample(true, true, false, true, false),
        (false, false)
    );
    assert_eq!(
        physical.packet_pointer_sample(false, false, true, false, false),
        (false, false),
        "losing the window after release does not cancel a completed gesture"
    );
}

#[test]
fn shift_snapshot_uses_the_focused_physical_key_owner() {
    let mut physical = PhysicalControls::default();
    physical.track_held("ShiftLeft".into(), true);
    assert!(physical.shift_held(true));
    assert!(!physical.shift_held(false));
    physical.track_held("ShiftRight".into(), true);
    physical.track_held("ShiftLeft".into(), false);
    assert!(
        physical.shift_held(true),
        "either physical Shift key remains held"
    );
    physical.track_held("ShiftRight".into(), false);
    physical.track_held("KeyS".into(), true);
    assert!(!physical.shift_held(true));
}

#[test]
fn native_menu_reopen_consumes_its_toggle_but_focused_capture_keeps_it() {
    let input = || {
        vec![
            PanelSample::Key(("ShiftRight".into(), None, false, Default::default())),
            PanelSample::Key(("KeyR".into(), None, false, Default::default())),
        ]
    };
    for owned in [false, true] {
        let mut keys = input();
        assert!(take_packet_toggle(
            &mut keys,
            "ShiftRight",
            owned,
            false,
            true
        ));
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key().unwrap().0, "KeyR");
    }
    let mut keys = input();
    assert!(!take_packet_toggle(
        &mut keys,
        "ShiftRight",
        true,
        true,
        true
    ));
    assert_eq!(keys.len(), 2);
    assert!(!take_packet_toggle(
        &mut keys,
        "ShiftRight",
        false,
        true,
        false
    ));
    assert_eq!(keys.len(), 2);
    assert!(take_packet_toggle(
        &mut keys,
        "ShiftRight",
        true,
        true,
        false
    ));
    assert_eq!(keys.len(), 1);
}

#[test]
fn reserved_fast_tap_never_reaches_gameplay_edges() {
    let mut keys = ButtonInput::default();
    keys.press(KeyCode::KeyW);
    keys.release(KeyCode::KeyW);
    keys.press(KeyCode::KeyA);
    consume_reserved(&mut keys, None, &["KeyW".into()], Some("ShiftRight"), None);
    assert!(!keys.just_pressed(KeyCode::KeyW));
    assert!(!keys.just_released(KeyCode::KeyW));
    assert!(keys.pressed(KeyCode::KeyA));
    assert!(keys.just_pressed(KeyCode::KeyA));
}

#[test]
fn keys_already_held_when_a_mod_attaches_are_reported_held_without_a_press_edge() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("empty.wat");
    std::fs::write(
        &path,
        r#"(component
        (core module $m (func (export "init")) (func (export "frame")))
        (core instance $i (instantiate $m))
        (func (export "init") (canon lift (core func $i "init")))
        (func (export "frame") (canon lift (core func $i "frame"))))"#,
    )
    .unwrap();
    let mut scratch = App::new();
    super::super::configure_set(&mut scratch, vec![(path, mod_host::ModGrants::default())]);
    let runtime = scratch
        .world_mut()
        .remove_resource::<super::super::ModRuntime>()
        .unwrap();
    let mut app = App::new();
    let mut keys = ButtonInput::<KeyCode>::default();
    keys.press(KeyCode::ShiftLeft);
    keys.clear_just_pressed(KeyCode::ShiftLeft);
    app.insert_resource(runtime)
        .insert_resource(keys)
        .insert_resource(UiRuntimeResource(UiRuntime::new(1)))
        .insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
            1,
        )))
        .insert_resource(UiPresentationResource(
            UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap(),
        ))
        .add_systems(Update, prepare_mod_input);
    app.world_mut().spawn((
        Window {
            focused: true,
            ..Default::default()
        },
        CursorOptions::default(),
        PrimaryWindow,
    ));
    app.update();
    let controls = &app.world().resource::<super::super::ModRuntime>().controls;
    assert_eq!(controls.keys_held, ["ShiftLeft"]);
    assert!(controls.keys_pressed.is_empty());
}

#[test]
fn held_keys_follow_press_and_release_and_stay_bounded() {
    let mut physical = PhysicalControls::default();
    physical.track_held("Digit1".into(), true);
    physical.track_held("Digit1".into(), true);
    physical.track_held("ShiftLeft".into(), true);
    assert_eq!(physical.held_keys, ["Digit1", "ShiftLeft"]);
    physical.track_held("Digit1".into(), false);
    assert_eq!(physical.held_keys, ["ShiftLeft"]);
    for index in 0..mod_api::MAX_CONTROL_KEYS * 2 {
        physical.track_held(format!("Key{index}"), true);
    }
    assert_eq!(physical.held_keys.len(), mod_api::MAX_CONTROL_KEYS);
    physical.discard_pending(None, None);
    assert!(physical.held_keys.is_empty());
}

#[test]
fn guest_removal_or_quarantine_restores_previous_capture_once() {
    let mut physical = PhysicalControls::default();
    assert!(!physical.finish_panel(true, true, false, true));
    assert!(physical.finish_panel(false, true, false, false));
    assert!(!physical.finish_panel(false, true, false, false));
}

#[test]
fn focus_or_other_ui_cancels_recapture_and_uncaptured_open_stays_free() {
    for (focused, absorbed) in [(false, false), (true, true)] {
        let mut physical = PhysicalControls::default();
        physical.finish_panel(true, true, false, true);
        assert!(!physical.finish_panel(false, focused, absorbed, false));
        assert!(!physical.finish_panel(false, true, false, false));
    }
    let mut physical = PhysicalControls::default();
    physical.finish_panel(true, true, false, false);
    assert!(!physical.finish_panel(false, true, false, false));
}

#[test]
fn pointer_observations_count_edges_preserve_holds_and_ignore_other_windows() {
    let mut physical = PhysicalControls::default();
    let mut events = Messages::default();
    let mut world = World::new();
    let window = world.spawn_empty().id();
    let other = world.spawn_empty().id();
    let event = |window, button, state| MouseButtonInput {
        window,
        button,
        state,
    };
    events.write(event(other, MouseButton::Right, ButtonState::Pressed));
    events.write(event(window, MouseButton::Left, ButtonState::Pressed));
    events.write(event(window, MouseButton::Left, ButtonState::Pressed));
    assert_eq!(
        physical.pointer_edges(Some(&events), None, window, true),
        ["MouseLeft"]
    );
    assert!(physical.left_held && physical.held_keys.contains(&"MouseLeft".into()));
    assert!(
        physical
            .pointer_edges(Some(&events), None, window, true)
            .is_empty()
    );
    events.write(event(window, MouseButton::Left, ButtonState::Released));
    events.write(event(window, MouseButton::Left, ButtonState::Pressed));
    events.write(event(window, MouseButton::Right, ButtonState::Pressed));
    assert_eq!(
        physical.pointer_edges(Some(&events), None, window, true),
        ["MouseLeft", "MouseRight"]
    );
    physical.discard_pending(None, Some(&events));
    assert!(physical.held_keys.is_empty() && !physical.left_held);
    events.write(event(window, MouseButton::Left, ButtonState::Pressed));
    assert!(
        physical
            .pointer_edges(Some(&events), None, window, false)
            .is_empty()
    );
}

#[test]
fn dormant_input_discards_old_edges_and_observes_only_next_attachment_edges() {
    use bevy::input::keyboard::Key;
    let mut keyboard = Messages::default();
    let key = |key_code, logical_key| KeyboardInput {
        key_code,
        logical_key,
        state: ButtonState::Pressed,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    };
    keyboard.write(key(KeyCode::F8, Key::F8));
    let mut mouse = Messages::default();
    mouse.write(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Pressed,
        window: Entity::PLACEHOLDER,
    });
    let mut physical = PhysicalControls {
        left_held: true,
        ..Default::default()
    };
    physical.discard_pending(Some(&keyboard), Some(&mouse));
    assert!(!physical.left_held);
    assert_eq!(physical.keys.read(&keyboard).count(), 0);
    assert_eq!(physical.mouse.read(&mouse).count(), 0);
    keyboard.write(key(KeyCode::F10, Key::F10));
    mouse.write(MouseButtonInput {
        button: MouseButton::Left,
        state: ButtonState::Released,
        window: Entity::PLACEHOLDER,
    });
    assert_eq!(
        physical
            .keys
            .read(&keyboard)
            .map(|event| event.key_code)
            .collect::<Vec<_>>(),
        [KeyCode::F10]
    );
    assert_eq!(
        physical
            .mouse
            .read(&mouse)
            .map(|event| event.state)
            .collect::<Vec<_>>(),
        [ButtonState::Released]
    );
}
