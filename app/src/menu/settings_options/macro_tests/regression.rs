//! Macro shortcuts respect physical presses, screen ownership and pointer editing.
use super::*;
use bevy::input::touch::{TouchInput, TouchPhase, touch_screen_input_system};

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
                crate::semantic_controls::keyboard_usage(code).unwrap(),
            );
            arm(&mut app, control);
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            let row = launcher::menu::settings_options::KEY_BINDINGS.len()
                + launcher::menu::settings_options::EXTRA_KEYS
                    .iter()
                    .position(|(name, _)| *name == "key.emote")
                    .unwrap();
            assert!(Arc::make_mut(&mut menu.settings_options).remap(row, control));
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
            assert!(Arc::make_mut(&mut menu.settings_options).remap(row, control));
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
            PhysicalControl::KeyboardUsage(crate::semantic_controls::keyboard_usage(code).unwrap()),
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

/// Builds a native command field and finds a visible point inside its existing text.
fn command_field() -> (App, Entity, ui::UiPoint, usize) {
    let mut presentation =
        UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap();
    presentation
        .enable_json_ui(client_ui::test_support::mini_carrier())
        .unwrap();
    let (mut app, window) = crate::ui_runtime::tests::menu_input_tests::menu_input_app_with(
        crate::menu::MenuClipboard::default(),
        presentation,
    );
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.screen = MenuScreen::Settings;
        menu.settings_section =
            client_ui::test_support::settings_section_index("keyboard_and_mouse_forced_index")
                .unwrap();
        menu.settings_focus = vec![MenuAction::SettingsMacroCommand(0)];
        menu.activate(MenuAction::SettingsMacroCommand(0));
        menu.edit_text("time set day");
    }
    let mut view = app.world().resource::<MenuRuntime>().view();
    view.focused_action = Some(MenuAction::SettingsMacroCommand(0));
    let player = app
        .world()
        .resource::<client_state::PlayerRuntime>()
        .clone();
    let (point, byte) = {
        let mut presentation = app.world_mut().resource_mut::<UiPresentationRuntime>();
        presentation.set_menu_view(Some(view));
        presentation
            .build(
                &player,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        assert!(presentation.uses_oreui_settings());
        (0..720)
            .step_by(4)
            .flat_map(|y| {
                (0..1280)
                    .step_by(4)
                    .map(move |x| ui::UiPoint::new(x as f32, y as f32).unwrap())
            })
            .find_map(|point| {
                if presentation.hit_test_menu(point) != Some(MenuAction::SettingsMacroCommand(0)) {
                    return None;
                }
                let byte = presentation.menu_caret_at(
                    point,
                    MenuField::MacroCommand(0),
                    "time set day",
                )?;
                (byte > 0 && byte < 12).then_some((point, byte))
            })
            .expect("native macro field publishes an interior caret target")
    };
    (app, window, point, byte)
}

#[test]
fn command_macro_mouse_release_places_caret_at_the_clicked_character() {
    let (mut app, window, point, byte) = command_field();
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(point.x(), point.y())));
    mouse(&mut app, MouseButton::Left, false);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    let mut view = app.world().resource::<MenuRuntime>().view();
    view.focused_action = Some(MenuAction::SettingsMacroCommand(0));
    client_ui::test_support::snapshot_menu(
        app.world().resource::<client_state::PlayerRuntime>(),
        &view,
        "command-macro-caret",
    );
    assert_eq!(view.caret.byte, byte);
    crate::ui_runtime::tests::menu_input_tests::press_key(
        &mut app,
        window,
        KeyCode::KeyX,
        Some("x"),
    );
    let mut expected = "time set day".to_owned();
    expected.insert(byte, 'x');
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .field_text(MenuField::MacroCommand(0)),
        expected
    );
}

#[test]
fn command_macro_touch_release_places_caret_at_the_tapped_character() {
    let (mut app, window, point, byte) = command_field();
    app.add_message::<TouchInput>()
        .add_systems(PreUpdate, touch_screen_input_system);
    for phase in [TouchPhase::Started, TouchPhase::Ended] {
        app.world_mut().write_message(TouchInput {
            phase,
            position: Vec2::new(point.x(), point.y()),
            window,
            force: None,
            id: 1,
        });
        app.update();
    }
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().caret.byte,
        byte
    );
    crate::ui_runtime::tests::menu_input_tests::press_key(
        &mut app,
        window,
        KeyCode::KeyX,
        Some("x"),
    );
    let mut expected = "time set day".to_owned();
    expected.insert(byte, 'x');
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .field_text(MenuField::MacroCommand(0)),
        expected
    );
}
