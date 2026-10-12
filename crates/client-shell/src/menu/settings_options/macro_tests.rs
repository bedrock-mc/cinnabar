//! Command drafts and pointer editing use the production menu input driver.
use crate::menu::{MenuClipboard, MenuRuntime, drive_menu_input};
use crate::ui_resources::UiPresentationResource;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::MouseButtonInput,
    },
    prelude::*,
    window::{CursorOptions, PrimaryWindow, WindowEvent},
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use launcher::menu::{MenuAction, MenuField, MenuScreen};

mod regression;

/// A focused 1280x720 window driving `drive_menu_input` over `presentation`.
fn menu_input_app_with(
    clipboard: MenuClipboard,
    presentation: UiPresentationRuntime,
) -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        1,
    )))
    .add_message::<KeyboardInput>()
    .init_resource::<ButtonInput<KeyCode>>()
    .init_resource::<ButtonInput<MouseButton>>()
    .init_resource::<Touches>()
    .insert_resource(UiPresentationResource(presentation))
    .insert_resource(MenuRuntime::new(true, 2, "test".into()))
    .insert_resource(clipboard)
    .add_systems(Update, drive_menu_input);
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: true,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    (app, window)
}

/// Sends one physical key press and drives the menu frame.
fn press_key(app: &mut App, window: Entity, key_code: KeyCode, text: Option<&str>) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(key_code);
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        state: ButtonState::Pressed,
        text: text.map(Into::into),
        repeat: false,
        window,
    });
    app.update();
}

/// Finds the focused window used by the input fixtures.
fn window(app: &mut App) -> Entity {
    app.world_mut()
        .query::<(Entity, &Window)>()
        .single(app.world())
        .unwrap()
        .0
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
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    presentation
        .enable_json_ui(client_ui::test_support::mini_carrier())
        .unwrap();
    let (mut app, window) =
        menu_input_app_with(crate::menu::MenuClipboard::default(), presentation);
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
        let mut presentation = app.world_mut().resource_mut::<UiPresentationResource>();
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
        press_key(&mut app, window, code, Some(text));
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
    let presentation = UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let (mut app, window) =
        menu_input_app_with(crate::menu::MenuClipboard::default(), presentation);
    app.insert_resource(menu);
    press_key(&mut app, window, KeyCode::Backspace, None);
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
