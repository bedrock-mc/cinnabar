//! Macro shortcuts respect physical presses, screen ownership and pointer editing.
use super::*;
use client_shell::menu::test_support as menu_fixture;

/// Sends an OS repeat without a new physical release or press.
fn repeat(app: &mut App, code: KeyCode, text: Option<&str>) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(code);
    let window = window(app);
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: true,
        window,
    });
}

/// Ends one frame's button edges while retaining physically held controls.
fn clear_edges(app: &mut App) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .clear();
}

#[test]
fn command_macro_ignores_os_repeats() {
    let mut app = crate::tests::gameplay_click::gameplay_app(false);
    arm(&mut app, PhysicalControl::KeyboardUsage(0x2c));
    app.update();
    key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, false);
    key(&mut app, KeyCode::Space, ButtonState::Pressed, false);
    app.update();
    assert_eq!(
        app.world()
            .resource::<UiRuntime>()
            .pending_chat_sends()
            .len(),
        1
    );
    clear_edges(&mut app);
    repeat(&mut app, KeyCode::Space, None);
    app.update();
    assert_eq!(
        app.world()
            .resource::<UiRuntime>()
            .pending_chat_sends()
            .len(),
        1
    );
}

#[test]
fn command_macro_repeat_filter_preserves_chat_text_repeats() {
    let mut app = crate::tests::gameplay_click::gameplay_app(false);
    app.update();
    key(&mut app, KeyCode::KeyT, ButtonState::Pressed, false);
    app.update();
    repeat(&mut app, KeyCode::KeyX, Some("x"));
    app.update();
    assert_eq!(
        app.world().resource::<UiRuntime>().chat_editor().as_str(),
        "x"
    );
}

#[test]
fn command_macro_cannot_run_while_editing_a_sign() {
    for pointer in [false, true] {
        let mut app = crate::tests::gameplay_click::gameplay_app(false);
        arm(
            &mut app,
            if pointer {
                PhysicalControl::MouseButton(1)
            } else {
                PhysicalControl::KeyboardUsage(0x2c)
            },
        );
        app.update();
        app.world_mut()
            .resource_mut::<UiRuntime>()
            .sign_editor_mut()
            .open(client_ui::ui_runtime::sign_editor::SignEdit::new(
                [0, 0, 0],
                true,
                Default::default(),
            ));
        key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, false);
        if pointer {
            mouse(&mut app, MouseButton::Left, false);
        } else {
            key(&mut app, KeyCode::Space, ButtonState::Pressed, false);
        }
        app.update();
        let runtime = app.world().resource::<UiRuntime>();
        assert!(runtime.sign_editor().is_open());
        assert!(runtime.pending_chat_sends().is_empty(), "pointer={pointer}");
    }
}

#[test]
fn command_macro_precedes_default_and_remapped_emote_bindings() {
    for code in [KeyCode::KeyB, KeyCode::KeyV] {
        for alt_first in [false, true] {
            let mut app = crate::tests::gameplay_click::gameplay_app(false);
            let control = PhysicalControl::KeyboardUsage(
                client_ui::physical_input::keyboard_usage(code).unwrap(),
            );
            arm(&mut app, control);
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            let row = launcher::menu::settings_options::KEY_BINDINGS.len()
                + launcher::menu::settings_options::EXTRA_KEYS
                    .iter()
                    .position(|(name, _)| *name == "key.emote")
                    .unwrap();
            assert!(menu_fixture::remap_control(&mut menu, row, control));
            app.update();
            if alt_first {
                key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, false);
            }
            key(&mut app, code, ButtonState::Pressed, false);
            if !alt_first {
                key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, false);
            }
            app.update();
            let runtime = app.world().resource::<UiRuntime>();
            assert_eq!(
                runtime.pending_chat_sends().len(),
                usize::from(alt_first),
                "{code:?} alt_first={alt_first}"
            );
            assert_eq!(runtime.emotes().is_open(), !alt_first);
        }
    }
}

#[test]
fn command_macro_precedes_a_mouse_emote_binding() {
    for ordered in [false, true] {
        for release_first in [false, true] {
            let mut app = crate::tests::gameplay_click::gameplay_app(false);
            app.add_message::<WindowEvent>();
            let control = PhysicalControl::MouseButton(2);
            arm(&mut app, control);
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            let row = launcher::menu::settings_options::KEY_BINDINGS.len()
                + launcher::menu::settings_options::EXTRA_KEYS
                    .iter()
                    .position(|(name, _)| *name == "key.emote")
                    .unwrap();
            assert!(menu_fixture::remap_control(&mut menu, row, control));
            app.update();
            key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, ordered);
            app.update();
            clear_edges(&mut app);
            if ordered && release_first {
                key(&mut app, KeyCode::AltLeft, ButtonState::Released, true);
            }
            mouse(&mut app, MouseButton::Right, ordered);
            if ordered && !release_first {
                key(&mut app, KeyCode::AltLeft, ButtonState::Released, true);
            }
            app.update();
            let runtime = app.world().resource::<UiRuntime>();
            let macro_runs = !ordered || !release_first;
            assert_eq!(
                runtime.pending_chat_sends().len(),
                usize::from(macro_runs),
                "ordered={ordered} release_first={release_first}"
            );
            assert_eq!(runtime.emotes().is_open(), !macro_runs);
        }
    }
}

#[test]
fn command_macro_keeps_an_open_emote_wheel_in_control() {
    for code in [KeyCode::Space, KeyCode::KeyB] {
        let mut app = crate::tests::gameplay_click::gameplay_app(false);
        arm(
            &mut app,
            PhysicalControl::KeyboardUsage(
                client_ui::physical_input::keyboard_usage(code).unwrap(),
            ),
        );
        app.update();
        key(&mut app, KeyCode::KeyB, ButtonState::Pressed, false);
        app.update();
        assert!(app.world().resource::<UiRuntime>().emotes().is_open());
        clear_edges(&mut app);
        key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, false);
        key(&mut app, code, ButtonState::Pressed, false);
        app.update();
        assert!(
            app.world()
                .resource::<UiRuntime>()
                .pending_chat_sends()
                .is_empty()
        );
        assert!(!app.world().resource::<UiRuntime>().emotes().is_open());
        if code == KeyCode::KeyB {
            // The wheel cleared the buttons it owned; a later press is an ordinary binding.
            clear_edges(&mut app);
            key(&mut app, code, ButtonState::Pressed, false);
            app.update();
            assert!(app.world().resource::<UiRuntime>().emotes().is_open());
            assert!(
                app.world()
                    .resource::<UiRuntime>()
                    .pending_chat_sends()
                    .is_empty()
            );
        }
    }
}

#[test]
fn command_macro_suppresses_a_held_key_until_physical_release() {
    let mut app = crate::tests::gameplay_click::gameplay_app(false);
    arm(&mut app, PhysicalControl::KeyboardUsage(0x2c));
    app.update();
    key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, false);
    key(&mut app, KeyCode::Space, ButtonState::Pressed, false);
    app.update();
    // The next raw frame must still see Space so quarantine survives.
    clear_edges(&mut app);
    app.update();
    key(&mut app, KeyCode::AltLeft, ButtonState::Released, false);
    app.update();
    clear_edges(&mut app);
    repeat(&mut app, KeyCode::Space, None);
    app.update();
    let phase = app
        .world()
        .resource::<SemanticInputSnapshot>()
        .phase(Action::Jump);
    assert!(!phase.held && !phase.pressed);
    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::Space)
    );
    clear_edges(&mut app);
    key(&mut app, KeyCode::Space, ButtonState::Released, false);
    app.update();
    clear_edges(&mut app);
    key(&mut app, KeyCode::Space, ButtonState::Pressed, false);
    app.update();
    assert!(
        app.world()
            .resource::<SemanticInputSnapshot>()
            .phase(Action::Jump)
            .pressed
    );
}
