//! Return navigation is exercised through the input system and painted controls.

use crate::menu::{
    MenuAction, MenuRuntime, MenuScreen,
    input::{MenuClipboard, MenuInputMode, drive_menu_input},
};
use bevy::{
    input::keyboard::Key,
    prelude::*,
    window::{CursorOptions, PrimaryWindow},
};
use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};

/// Paints the current screen and lets input consume its enabled controls.
fn draw(app: &mut App) {
    let view = app.world().resource::<MenuRuntime>().view();
    app.world_mut()
        .resource_scope(|world, mut presentation: Mut<UiPresentationRuntime>| {
            presentation.set_menu_view(Some(view));
            presentation
                .build(
                    world.resource::<crate::player_runtime::PlayerRuntime>(),
                    &UiRuntime::new(1),
                    0,
                    [1280, 720],
                    ui::DpiScale::new(1.0).unwrap(),
                )
                .unwrap();
        });
    app.update();
}

/// Creates an isolated menu with the installed carrier, or names the missing fixture.
fn fixture(test: &str) -> Option<(App, Entity)> {
    let Some(presentation) = client_ui::test_support::engine_presentation() else {
        eprintln!("skipping {test}: missing installed UI carrier; make assets");
        return None;
    };
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_message::<bevy::input::keyboard::KeyboardInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(MenuRuntime::new(true, 2, "BugTest".into()))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(presentation)
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
    draw(&mut app);
    Some((app, window))
}

/// Sends a real menu keyboard event without OS input.
fn key(app: &mut App, window: Entity, key_code: KeyCode) {
    app.world_mut()
        .write_message(bevy::input::keyboard::KeyboardInput {
            key_code,
            logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
            state: bevy::input::ButtonState::Pressed,
            text: None,
            repeat: false,
            window,
        });
    app.update();
    draw(app);
}

#[test]
fn keyboard_back_restores_the_visible_home_control() {
    let Some((mut app, window)) = fixture("keyboard_back_restores_the_visible_home_control") else {
        return;
    };
    for screen in [MenuScreen::Settings, MenuScreen::Play] {
        let action = MenuAction::Navigate(screen);
        {
            let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
            menu.input_mode = MenuInputMode::Keyboard;
            menu.focus_pointer(action);
        }
        key(&mut app, window, KeyCode::Enter);
        assert_eq!(app.world().resource::<MenuRuntime>().screen(), screen);
        key(&mut app, window, KeyCode::Escape);
        let view = app.world().resource::<MenuRuntime>().view();
        assert_eq!(view.screen, MenuScreen::Home);
        assert_eq!(view.focused_action, Some(action));
        assert!(view.navigation_focus_visible);
        assert!(
            app.world()
                .resource::<UiPresentationRuntime>()
                .visible_menu_actions()
                .any(|visible| Some(visible) == view.focused_action)
        );
    }
}

#[test]
fn gamepad_back_restores_home_without_changing_pointer_outline_admission() {
    let Some((mut app, window)) =
        fixture("gamepad_back_restores_home_without_changing_pointer_outline_admission")
    else {
        return;
    };
    let action = MenuAction::Navigate(MenuScreen::Settings);
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.input_mode = MenuInputMode::Mouse;
        menu.activate(action);
    }
    draw(&mut app);
    key(&mut app, window, KeyCode::Escape);
    assert!(
        !app.world()
            .resource::<MenuRuntime>()
            .view()
            .navigation_focus_visible
    );
    let pad = app.world_mut().spawn(Gamepad::default()).id();
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.input_mode = MenuInputMode::Gamepad;
        menu.focus_pointer(action);
    }
    app.world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::South);
    app.update();
    app.world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .clear();
    draw(&mut app);
    assert_eq!(
        app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::Settings
    );
    app.world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::East);
    app.update();
    app.world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .clear();
    draw(&mut app);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(action));
    assert!(view.navigation_focus_visible && view.gamepad_input);
}

#[test]
fn profile_return_preserves_available_focus_and_falls_back_when_removed() {
    for available in [true, false] {
        let mut menu = MenuRuntime::new(true, 2, "BugTest".into());
        menu.control_auth = Some(crate::menu::AuthState::Authenticated);
        menu.feeds.profile.loaded = true;
        menu.feeds.profile.avatar_loaded = true;
        menu.feeds.profile.featured_screenshot_loaded = true;
        menu.activate(MenuAction::Navigate(MenuScreen::Profile));
        menu.input_mode = MenuInputMode::Keyboard;
        let action = MenuAction::Navigate(MenuScreen::DressingRoom);
        menu.focus_pointer(action);
        menu.activate_focused();
        assert_eq!(menu.screen(), MenuScreen::DressingRoom);
        menu.feeds.profile.loaded = available;
        menu.activate(MenuAction::AddBack);
        assert_eq!(menu.screen(), MenuScreen::Profile);
        assert_eq!(
            menu.view().focused_action,
            Some(if available {
                action
            } else {
                MenuAction::AddBack
            })
        );
    }
}

#[test]
fn first_settings_entry_keeps_its_painted_entry_focus() {
    let Some((mut app, window)) = fixture("first_settings_entry_keeps_its_painted_entry_focus")
    else {
        return;
    };
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.input_mode = MenuInputMode::Keyboard;
        menu.focus_pointer(MenuAction::Navigate(MenuScreen::Settings));
    }
    key(&mut app, window, KeyCode::Enter);
    let view = app.world().resource::<MenuRuntime>().view();
    assert!(matches!(
        view.focused_action,
        Some(MenuAction::SettingsSection(_))
    ));
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .visible_menu_actions()
            .any(|action| Some(action) == view.focused_action)
    );
}

#[test]
fn changed_account_controls_and_modal_ownership_keep_focus_available() {
    let Some((mut app, window)) =
        fixture("changed_account_controls_and_modal_ownership_keep_focus_available")
    else {
        return;
    };
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.input_mode = MenuInputMode::Keyboard;
        menu.focus_pointer(MenuAction::StartSignIn);
    }
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().focused_action,
        Some(MenuAction::StartSignIn)
    );
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.enter(MenuScreen::Settings);
        menu.control_auth = Some(crate::menu::AuthState::Authenticated);
    }
    draw(&mut app);
    key(&mut app, window, KeyCode::Escape);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_ne!(view.focused_action, Some(MenuAction::StartSignIn));
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .visible_menu_actions()
            .any(|action| Some(action) == view.focused_action)
    );
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        menu.go_back();
        menu.activate(MenuAction::OpenExitDialog);
    }
    draw(&mut app);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(MenuAction::ConfirmExit));
}

#[test]
fn home_entry_and_navigation_select_only_painted_enabled_controls() {
    let Some((mut app, _)) =
        fixture("home_entry_and_navigation_select_only_painted_enabled_controls")
    else {
        return;
    };
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().focused_action,
        Some(MenuAction::Navigate(MenuScreen::Play))
    );
    let actions: Vec<_> = app
        .world()
        .resource::<UiPresentationRuntime>()
        .visible_menu_actions()
        .collect();
    for _ in 0..actions.len() * 2 {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.move_focus(1);
        assert!(actions.contains(&menu.view().focused_action.unwrap()));
    }
}

#[test]
fn newer_keyboard_navigation_survives_the_deferred_return_validation() {
    let Some((mut app, window)) =
        fixture("newer_keyboard_navigation_survives_the_deferred_return_validation")
    else {
        return;
    };
    let home_actions: Vec<_> = app
        .world()
        .resource::<UiPresentationRuntime>()
        .visible_menu_actions()
        .collect();
    let settings_at = home_actions
        .iter()
        .position(|action| *action == MenuAction::Navigate(MenuScreen::Settings))
        .unwrap();
    let next_control = home_actions[(settings_at + 1) % home_actions.len()];
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    draw(&mut app);
    for key_code in [KeyCode::Escape, KeyCode::Tab] {
        app.world_mut()
            .write_message(bevy::input::keyboard::KeyboardInput {
                key_code,
                logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
                state: bevy::input::ButtonState::Pressed,
                text: None,
                repeat: false,
                window,
            });
    }
    app.update();
    let selected = app.world().resource::<MenuRuntime>().view().focused_action;
    assert_eq!(selected, Some(next_control));
    draw(&mut app);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().focused_action,
        selected
    );
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .visible_menu_actions()
            .any(|action| Some(action) == selected)
    );
}

#[test]
fn borrowed_popup_ownership_matches_the_presented_prompt() {
    use crate::menu::{AuthState, MenuDialog};
    for auth in [
        AuthState::SignedOut,
        AuthState::Checking,
        AuthState::AwaitingCode {
            uri: "https://example.invalid".into(),
            code: "TEST-CODE".into(),
        },
        AuthState::Failed("Try again".into()),
        AuthState::Authenticated,
    ] {
        for requested in [false, true] {
            for context in 0..7 {
                let mut menu = MenuRuntime::new(true, 2, "BugTest".into());
                menu.control_auth = Some(auth.clone());
                menu.sign_in_requested = requested;
                match context {
                    1 => menu.dialog = Some(MenuDialog::Exit),
                    2 => menu.dialog = Some(MenuDialog::Accounts),
                    3 => menu.session.connecting = true,
                    4 => menu.disconnect_message = Some("Disconnected".into()),
                    5 => menu.push_join_request(1, "Placeholder".into(), std::time::Duration::ZERO),
                    6 => menu.presentation_accounts = true,
                    _ => {}
                }
                assert_eq!(
                    menu.navigation_popup_open(),
                    menu.view().popup_open(),
                    "auth={auth:?}, requested={requested}, context={context}"
                );
            }
        }
    }
}

/// Paints an event with an original badge and keeps its temporary image alive.
fn event_fixture(test: &str) -> Option<(App, Entity, tempfile::TempDir)> {
    let (mut app, window) = fixture(test)?;
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("badge.png");
    image::RgbaImage::from_pixel(4, 4, image::Rgba([80, 220, 70, 255]))
        .save(&path)
        .unwrap();
    let badge = path.to_string_lossy().into_owned();
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .feeds
        .home
        .live_event = Some(launcher::menu::LiveEventCard {
        button_text: "Live event".into(),
        badge_path: badge.clone(),
        ..Default::default()
    });
    {
        let mut presentation = app.world_mut().resource_mut::<UiPresentationRuntime>();
        presentation.sync_menu_artwork(vec![(badge, 512)]);
        presentation.finish_menu_artwork();
    }
    draw(&mut app);
    draw(&mut app);
    assert_eq!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .visible_menu_actions()
            .filter(|action| *action == MenuAction::OpenLiveEvent)
            .count(),
        2,
        "fixture must paint both event link and loaded badge"
    );
    Some((app, window, directory))
}

#[test]
fn a_loaded_event_badge_does_not_trap_keyboard_navigation() {
    let Some((mut app, window, _badge)) =
        event_fixture("a_loaded_event_badge_does_not_trap_keyboard_navigation")
    else {
        return;
    };
    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.input_mode = MenuInputMode::Keyboard;
        menu.focus_pointer(MenuAction::OpenLiveEvent);
    }
    key(&mut app, window, KeyCode::Tab);
    let selected = app.world().resource::<MenuRuntime>().view().focused_action;
    assert_ne!(selected, Some(MenuAction::OpenLiveEvent));
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .visible_menu_actions()
            .any(|action| Some(action) == selected)
    );
}

#[test]
fn a_loaded_event_badge_does_not_trap_gamepad_navigation() {
    let Some((mut app, _, _badge)) =
        event_fixture("a_loaded_event_badge_does_not_trap_gamepad_navigation")
    else {
        return;
    };
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .focus_pointer(MenuAction::OpenLiveEvent);
    let pad = app.world_mut().spawn(Gamepad::default()).id();
    app.world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .press(GamepadButton::DPadDown);
    app.update();
    app.world_mut()
        .get_mut::<Gamepad>(pad)
        .unwrap()
        .digital_mut()
        .clear();
    draw(&mut app);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_ne!(view.focused_action, Some(MenuAction::OpenLiveEvent));
    assert!(view.navigation_focus_visible && view.gamepad_input);
    assert!(
        app.world()
            .resource::<UiPresentationRuntime>()
            .visible_menu_actions()
            .any(|action| Some(action) == view.focused_action)
    );
}
