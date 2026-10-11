//! Command macro input through the production menu and gameplay systems.
use std::sync::Arc;

use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput, NativeKey},
        mouse::{AccumulatedMouseMotion, MouseButtonInput},
    },
    prelude::*,
    window::WindowEvent,
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use launcher::menu::{MenuAction, MenuField, MenuScreen};
use semantic_input::{Action, PhysicalControl};

use crate::menu::MenuRuntime;
use client_state::SemanticInputSnapshot;

mod regression;

/// Arms one command without changing the ordinary gameplay bindings.
fn arm(app: &mut App, control: PhysicalControl) {
    let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
    let settings = Arc::make_mut(&mut menu.settings_options);
    assert!(settings.remap_macro(0, control));
    assert!(settings.set_macro_command(0, "time set day"));
}

/// Finds the focused window used by the input fixtures.
fn window(app: &mut App) -> Entity {
    app.world_mut()
        .query::<(Entity, &Window)>()
        .single(app.world())
        .unwrap()
        .0
}

/// Queues a physical keyboard edge, optionally recording its pointer-stream position.
fn key(app: &mut App, code: KeyCode, state: ButtonState, ordered: bool) {
    let input = KeyboardInput {
        key_code: code,
        logical_key: Key::Unidentified(NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: window(app),
    };
    let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
    match state {
        ButtonState::Pressed => keys.press(code),
        ButtonState::Released => keys.release(code),
    }
    app.world_mut().write_message(input.clone());
    if ordered {
        app.world_mut()
            .write_message(WindowEvent::KeyboardInput(input));
    }
}

/// Queues a pointer press in the same arrival stream as keyboard events.
fn mouse(app: &mut App, button: MouseButton, ordered: bool) {
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(button);
    if ordered {
        let input = MouseButtonInput {
            button,
            state: ButtonState::Pressed,
            window: window(app),
        };
        app.world_mut()
            .write_message(WindowEvent::MouseButtonInput(input));
    }
}

#[test]
fn command_macro_space_types_arguments_in_native_settings() {
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
    }
    let view = app.world().resource::<MenuRuntime>().view();
    let player = app
        .world()
        .resource::<client_state::PlayerRuntime>()
        .clone();
    {
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
    }
    for (code, text) in [
        (KeyCode::KeyT, "time"),
        (KeyCode::Space, " "),
        (KeyCode::KeyS, "set"),
        (KeyCode::Space, " "),
        (KeyCode::KeyD, "day"),
    ] {
        crate::ui_runtime::tests::menu_input_tests::press_key(&mut app, window, code, Some(text));
    }
    let menu = app.world().resource::<MenuRuntime>();
    let mut view = menu.view();
    view.focused_action = Some(MenuAction::SettingsMacroCommand(0));
    client_ui::test_support::snapshot_menu(
        app.world().resource::<client_state::PlayerRuntime>(),
        &view,
        "command-macro-spaces",
    );
    assert_eq!(menu.field_text(MenuField::MacroCommand(0)), "time set day");
    assert_eq!(
        menu.settings_options.macro_send_text(0).as_deref(),
        Some("/time set day")
    );
}

#[test]
fn command_macro_consumes_only_its_keyboard_control_even_when_enqueue_fails() {
    for rejection in [
        None,
        Some(ui::ChatSendError::RateLimited),
        Some(ui::ChatSendError::QueueFull),
    ] {
        let mut app = crate::tests::gameplay_click::gameplay_app(false);
        arm(&mut app, PhysicalControl::KeyboardUsage(0x2c));
        app.update();
        if let Some(error) = rejection {
            let mut now = 0;
            loop {
                let result = app
                    .world_mut()
                    .resource_mut::<UiRuntime>()
                    .queue_chat_message("/say seed", now);
                match result {
                    Err(actual) if actual == error => break,
                    Err(ui::ChatSendError::RateLimited)
                        if error == ui::ChatSendError::QueueFull =>
                    {
                        now += 10_000
                    }
                    Ok(_) => {}
                    other => panic!("unexpected queue result: {other:?}"),
                }
            }
            app.world_mut()
                .resource_mut::<bevy::time::Time<bevy::time::Real>>()
                .advance_by(std::time::Duration::from_millis(now));
        }
        let before = app
            .world()
            .resource::<UiRuntime>()
            .pending_chat_sends()
            .len();
        key(&mut app, KeyCode::KeyW, ButtonState::Pressed, false);
        mouse(&mut app, MouseButton::Left, false);
        app.update();
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .clear();
        app.world_mut()
            .resource_mut::<ButtonInput<MouseButton>>()
            .clear();
        app.world_mut()
            .resource_mut::<AccumulatedMouseMotion>()
            .delta = Vec2::new(3.0, 2.0);
        key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, false);
        key(&mut app, KeyCode::Space, ButtonState::Pressed, false);
        app.update();
        let keys = app.world().resource::<ButtonInput<KeyCode>>();
        assert!(keys.pressed(KeyCode::KeyW), "{rejection:?}");
        assert!(keys.pressed(KeyCode::Space), "{rejection:?}");
        assert!(
            app.world()
                .resource::<ButtonInput<MouseButton>>()
                .pressed(MouseButton::Left)
        );
        assert_eq!(
            app.world().resource::<AccumulatedMouseMotion>().delta,
            Vec2::new(3.0, 2.0)
        );
        let snapshot = app.world().resource::<SemanticInputSnapshot>();
        assert_eq!(snapshot.movement(), [0.0, 1.0]);
        assert!(snapshot.phase(Action::Attack).held);
        assert_ne!(snapshot.look_delta(), [0.0; 2]);
        assert!(!snapshot.phase(Action::Jump).held);
        assert!(!snapshot.phase(Action::Jump).pressed);
        assert_eq!(
            app.world()
                .resource::<UiRuntime>()
                .pending_chat_sends()
                .len(),
            before + usize::from(rejection.is_none())
        );
    }
}

#[test]
fn command_macro_mouse_press_is_dispatched_and_consumed_in_both_routes() {
    for ordered in [false, true] {
        for button in [
            MouseButton::Left,
            MouseButton::Right,
            MouseButton::Middle,
            MouseButton::Back,
            MouseButton::Forward,
            MouseButton::Other(5),
            MouseButton::Other(6),
            MouseButton::Other(7),
        ] {
            let mut app = crate::tests::gameplay_click::gameplay_app(false);
            app.add_message::<WindowEvent>();
            let code = crate::semantic_controls::physical::mouse_button_code(button).unwrap();
            arm(&mut app, PhysicalControl::MouseButton(code));
            app.update();
            key(&mut app, KeyCode::KeyW, ButtonState::Pressed, ordered);
            key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, ordered);
            mouse(&mut app, button, ordered);
            app.update();
            let runtime = app.world().resource::<UiRuntime>();
            assert_eq!(
                runtime
                    .pending_chat_sends()
                    .back()
                    .map(|request| &*request.message),
                Some("/time set day"),
                "{ordered} {button:?}"
            );
            assert!(
                app.world()
                    .resource::<ButtonInput<MouseButton>>()
                    .pressed(button)
            );
            let snapshot = app.world().resource::<SemanticInputSnapshot>();
            assert_eq!(snapshot.movement(), [0.0, 1.0]);
            assert!(!snapshot.phase(Action::Attack).held);
            assert!(!snapshot.phase(Action::Use).held);
        }
    }
}

#[test]
fn command_macro_uses_alt_state_at_each_keyboard_and_mouse_event() {
    for pointer in [false, true] {
        for alt_first in [false, true] {
            let mut app = crate::tests::gameplay_click::gameplay_app(false);
            app.add_message::<WindowEvent>();
            arm(
                &mut app,
                if pointer {
                    PhysicalControl::MouseButton(1)
                } else {
                    PhysicalControl::KeyboardUsage(0x2c)
                },
            );
            app.update();
            if alt_first {
                key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, true);
            }
            if pointer {
                mouse(&mut app, MouseButton::Left, true);
            } else {
                key(&mut app, KeyCode::Space, ButtonState::Pressed, true);
            }
            if !alt_first {
                key(&mut app, KeyCode::AltLeft, ButtonState::Pressed, true);
            }
            app.update();
            assert_eq!(
                app.world()
                    .resource::<UiRuntime>()
                    .pending_chat_sends()
                    .len(),
                usize::from(alt_first),
                "pointer={pointer}, alt_first={alt_first}"
            );
        }
    }
}

#[test]
fn command_macro_invalid_draft_does_not_replace_the_saved_command() {
    let mut menu = MenuRuntime::new(true, 2, "Macros".into());
    menu.screen = MenuScreen::Settings;
    menu.settings_section =
        client_ui::test_support::settings_section_index("keyboard_and_mouse_forced_index").unwrap();
    menu.activate(MenuAction::SettingsMacroCommand(0));
    menu.edit_text(&"x".repeat(ui::MAX_CHAT_INPUT_BYTES));
    let mut view = menu.view();
    view.focused_action = Some(MenuAction::SettingsMacroCommand(0));
    client_ui::test_support::snapshot_menu(
        &client_state::PlayerRuntime(player_state::PlayerState::new(1)),
        &view,
        "command-macro-validation",
    );
    assert_eq!(
        menu.field_text(MenuField::MacroCommand(0)).len(),
        ui::MAX_CHAT_INPUT_BYTES
    );
    assert_eq!(menu.settings_options.macro_command(0), "");
    let presentation = UiPresentationRuntime::new(client_ui::test_support::fixture_font()).unwrap();
    let (mut app, window) = crate::ui_runtime::tests::menu_input_tests::menu_input_app_with(
        crate::menu::MenuClipboard::default(),
        presentation,
    );
    app.insert_resource(menu);
    crate::ui_runtime::tests::menu_input_tests::press_key(
        &mut app,
        window,
        KeyCode::Backspace,
        None,
    );
    assert_eq!(
        app.world()
            .resource::<MenuRuntime>()
            .settings_options
            .macro_send_text(0)
            .unwrap()
            .len(),
        ui::MAX_CHAT_INPUT_BYTES
    );
}

#[test]
fn command_macro_observes_alt_release_at_each_event() {
    for pointer in [false, true] {
        for release_first in [false, true] {
            let mut app = crate::tests::gameplay_click::gameplay_app(false);
            app.add_message::<WindowEvent>();
            arm(
                &mut app,
                if pointer {
                    PhysicalControl::MouseButton(1)
                } else {
                    PhysicalControl::KeyboardUsage(0x2c)
                },
            );
            app.update();
            key(&mut app, KeyCode::AltRight, ButtonState::Pressed, true);
            app.update();
            app.world_mut()
                .resource_mut::<ButtonInput<KeyCode>>()
                .clear();
            if release_first {
                key(&mut app, KeyCode::AltRight, ButtonState::Released, true);
            }
            if pointer {
                mouse(&mut app, MouseButton::Left, true);
            } else {
                key(&mut app, KeyCode::Space, ButtonState::Pressed, true);
            }
            if !release_first {
                key(&mut app, KeyCode::AltRight, ButtonState::Released, true);
            }
            app.update();
            assert_eq!(
                app.world()
                    .resource::<UiRuntime>()
                    .pending_chat_sends()
                    .len(),
                usize::from(!release_first)
            );
        }
    }
}
