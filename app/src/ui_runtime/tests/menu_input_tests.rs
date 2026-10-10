use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::AccumulatedMouseMotion,
    },
    prelude::*,
    time::Real,
    window::{CursorOptions, PrimaryWindow},
};

use client_ui::ui_runtime::{UiRuntime, presentation::UiPresentationRuntime};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use {
    crate::menu::{MenuClipboard, MenuRuntime, drive_menu_input},
    client_ui::test_support::fixture_font,
    launcher::menu::{MenuAction, MenuField, MenuScreen},
};

fn menu_input_app(clipboard: MenuClipboard) -> (App, Entity) {
    menu_input_app_with(
        clipboard,
        UiPresentationRuntime::new(fixture_font()).unwrap(),
    )
}

/// A focused 1280x720 window driving `drive_menu_input` over `presentation`.
pub(crate) fn menu_input_app_with(
    clipboard: MenuClipboard,
    presentation: UiPresentationRuntime,
) -> (App, Entity) {
    let mut app = App::new();
    app.insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .add_message::<KeyboardInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .insert_resource(presentation)
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

pub(crate) fn press_key(app: &mut App, window: Entity, key_code: KeyCode, text: Option<&str>) {
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

fn release_key(app: &mut App, window: Entity, key_code: KeyCode) {
    app.world_mut().write_message(KeyboardInput {
        key_code,
        logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        state: ButtonState::Released,
        text: None,
        repeat: false,
        window,
    });
    app.update();
}

fn queue_key(app: &mut App, window: Entity, key_code: KeyCode, text: Option<&str>) {
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
}

#[test]
fn tab_focus_and_edit_destination_stay_in_lockstep() {
    let (mut app, window) = menu_input_app(MenuClipboard::default());
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);

    press_key(&mut app, window, KeyCode::KeyA, Some("a"));
    press_key(&mut app, window, KeyCode::Enter, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().field,
        Some(MenuField::Name)
    );
    for _ in 0..3 {
        press_key(&mut app, window, KeyCode::Tab, None);
    }
    press_key(&mut app, window, KeyCode::KeyZ, Some("z"));
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(MenuAction::AddBack));
    assert_eq!(view.field, None);
    assert_eq!(view.name, "a");
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::AddName);

    press_key(&mut app, window, KeyCode::Tab, None);
    press_key(&mut app, window, KeyCode::KeyB, Some("b"));
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(MenuAction::AddAddress));
    assert_eq!(view.field, Some(MenuField::Address));
    assert_eq!(view.name, "a");
    assert_eq!(view.address, "b");

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    press_key(&mut app, window, KeyCode::Tab, None);
    release_key(&mut app, window, KeyCode::ShiftLeft);
    press_key(&mut app, window, KeyCode::KeyC, Some("c"));
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(MenuAction::AddName));
    assert_eq!(view.field, Some(MenuField::Name));
    assert_eq!(view.name, "ac");

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::AddName);
    for _ in 0..4 {
        press_key(&mut app, window, KeyCode::Tab, None);
    }
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(MenuAction::AddSave));
    assert_eq!(view.field, None);
    assert_eq!(view.screen, MenuScreen::AddServer);
}

#[test]
fn modifier_selection_and_paste_are_bounded_unicode_safe_and_input_owned() {
    let read_count = Arc::new(AtomicUsize::new(0));
    let observed_reads = Arc::clone(&read_count);
    let requested_maximum = Arc::new(AtomicUsize::new(0));
    let observed_maximum = Arc::clone(&requested_maximum);
    let copied = Arc::new(Mutex::new(None));
    let observed_copy = Arc::clone(&copied);
    let (mut app, window) = menu_input_app(MenuClipboard::with_access(
        move |maximum| {
            observed_reads.fetch_add(1, Ordering::Relaxed);
            observed_maximum.store(maximum, Ordering::Relaxed);
            let text = "server-🌍";
            (text.len() <= maximum).then(|| text.to_owned())
        },
        move |text| *observed_copy.lock().unwrap() = Some(text),
    ));
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
    press_key(&mut app, window, KeyCode::KeyA, Some("🙂"));
    assert_eq!(read_count.load(Ordering::Relaxed), 0);

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    press_key(&mut app, window, KeyCode::KeyA, Some("a"));
    press_key(&mut app, window, KeyCode::KeyV, Some("v"));
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.name, "server-🌍");
    assert_eq!(read_count.load(Ordering::Relaxed), 1);
    assert_eq!(requested_maximum.load(Ordering::Relaxed), 64);

    press_key(&mut app, window, KeyCode::ControlRight, None);
    release_key(&mut app, window, KeyCode::ControlLeft);
    press_key(&mut app, window, KeyCode::KeyA, Some("a"));
    press_key(&mut app, window, KeyCode::KeyC, Some("c"));
    assert_eq!(copied.lock().unwrap().as_deref(), Some("server-🌍"));
    press_key(&mut app, window, KeyCode::Backspace, None);
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "");

    release_key(&mut app, window, KeyCode::ControlRight);
    press_key(&mut app, window, KeyCode::KeyE, Some("\u{1}é"));
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "é");

    press_key(&mut app, window, KeyCode::SuperLeft, None);
    press_key(&mut app, window, KeyCode::KeyA, Some("a"));
    press_key(&mut app, window, KeyCode::KeyV, Some("v"));
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().name,
        "server-🌍"
    );
    assert_eq!(read_count.load(Ordering::Relaxed), 2);
    release_key(&mut app, window, KeyCode::SuperLeft);

    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .focused = false;
    queue_key(&mut app, window, KeyCode::ControlLeft, None);
    queue_key(&mut app, window, KeyCode::KeyV, Some("v"));
    queue_key(&mut app, window, KeyCode::Enter, None);
    queue_key(&mut app, window, KeyCode::Escape, None);
    app.update();
    assert_eq!(read_count.load(Ordering::Relaxed), 2);
    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .focused = true;
    app.update();
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.screen, MenuScreen::AddServer);
    assert_eq!(view.name, "server-🌍");
    assert_eq!(read_count.load(Ordering::Relaxed), 2);
}

#[test]
fn caret_keys_move_inside_the_focused_field_and_edits_land_at_the_caret() {
    let (mut app, window) =
        menu_input_app(MenuClipboard::with_access(|_| Some("Z".to_owned()), |_| {}));
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
    for (key, text) in [
        (KeyCode::KeyA, "a"),
        (KeyCode::KeyB, "b"),
        (KeyCode::KeyC, "c"),
    ] {
        press_key(&mut app, window, key, Some(text));
    }
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    press_key(&mut app, window, KeyCode::KeyX, Some("x"));
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.name, "axbc");
    assert_eq!(view.caret.byte, 2);
    assert_eq!(
        view.field,
        Some(MenuField::Name),
        "Left/Right stay in the box"
    );
    assert_eq!(view.focused_action, Some(MenuAction::AddName));

    press_key(&mut app, window, KeyCode::Home, None);
    press_key(&mut app, window, KeyCode::Comma, Some("<"));
    press_key(&mut app, window, KeyCode::End, None);
    press_key(&mut app, window, KeyCode::Period, Some(">"));
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "<axbc>");

    press_key(&mut app, window, KeyCode::Home, None);
    press_key(&mut app, window, KeyCode::ArrowRight, None);
    press_key(&mut app, window, KeyCode::Delete, None);
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "<xbc>");
    press_key(&mut app, window, KeyCode::Backspace, None);
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "xbc>");

    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    press_key(&mut app, window, KeyCode::KeyV, Some("v"));
    release_key(&mut app, window, KeyCode::ControlLeft);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.name, "Zxbc>", "paste lands at the caret");
    assert_eq!(view.caret.byte, 1);

    press_key(&mut app, window, KeyCode::ArrowDown, None);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(
        view.field,
        Some(MenuField::Address),
        "Up/Down still move focus"
    );
    press_key(&mut app, window, KeyCode::KeyQ, Some("q"));
    assert_eq!(app.world().resource::<MenuRuntime>().view().address, "q");
}

#[test]
fn shift_arrows_select_characters_that_copy_and_typing_replace() {
    let (copies, copied) = std::sync::mpsc::channel();
    let (mut app, window) = menu_input_app(MenuClipboard::with_access(
        |_| None,
        move |text| copies.send(text).unwrap(),
    ));
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
    press_key(&mut app, window, KeyCode::KeyA, Some("ab🙂d"));
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ShiftLeft);
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    release_key(&mut app, window, KeyCode::ShiftLeft);
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    press_key(&mut app, window, KeyCode::KeyC, Some("c"));
    release_key(&mut app, window, KeyCode::ControlLeft);
    assert_eq!(copied.try_recv().as_deref(), Ok("b🙂"));
    press_key(&mut app, window, KeyCode::KeyY, Some("y"));
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "ayd");
}

#[test]
fn arrows_move_focus_when_no_text_box_is_focused() {
    let (mut app, window) = menu_input_app(MenuClipboard::default());
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
    for _ in 0..3 {
        press_key(&mut app, window, KeyCode::Tab, None);
    }
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(MenuAction::AddBack));
    assert_eq!(view.field, None);
    press_key(&mut app, window, KeyCode::ArrowLeft, None);
    let view = app.world().resource::<MenuRuntime>().view();
    assert_eq!(view.focused_action, Some(MenuAction::AddPort));
    assert_eq!(view.field, Some(MenuField::Port));
}

#[test]
fn escape_from_pause_settings_returns_to_pause_and_teardown_clears_context() {
    let (mut app, window) = menu_input_app(MenuClipboard::default());
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    press_key(&mut app, window, KeyCode::Escape, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().screen,
        MenuScreen::Home
    );

    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.show_world();
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);
    }
    press_key(&mut app, window, KeyCode::Escape, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().screen,
        MenuScreen::Pause
    );
    assert!(app.world().resource::<MenuRuntime>().is_visible());

    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PauseSettings);
    assert!(
        app.world_mut()
            .resource_mut::<MenuRuntime>()
            .absorb_session_failure("closed")
    );
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::Navigate(MenuScreen::Settings));
    press_key(&mut app, window, KeyCode::Escape, None);
    // Back pops to the play screen the failed join started from, never the old pause.
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().screen,
        MenuScreen::Play
    );

    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.show_world();
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);
        menu.show_home();
        menu.show_connecting();
        menu.show_world();
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    }
    press_key(&mut app, window, KeyCode::Escape, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().screen,
        MenuScreen::Home
    );
}

#[test]
fn chat_focus_loss_preserves_draft_and_does_not_open_pause() {
    let mut menu = MenuRuntime::new(false, 2, "test".into());
    let pause = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "pause_menu_on_focus_lost")
        .unwrap();
    menu.activate(MenuAction::SettingsOption(pause as u16, 1));
    let mut player = crate::player_runtime::PlayerRuntime::new(1);
    let mut runtime = UiRuntime::new(1);
    runtime.open_chat(&mut player);
    runtime.insert_chat_text("unsent draft").unwrap();
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .init_resource::<Touches>()
        .init_resource::<MenuClipboard>()
        .insert_resource(runtime)
        .insert_resource(player)
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
        .insert_resource(menu)
        .add_systems(
            Update,
            (super::super::drive_chat_keyboard_input, drive_menu_input).chain(),
        );
    let window = app
        .world_mut()
        .spawn((
            Window {
                focused: false,
                ..Default::default()
            },
            CursorOptions::default(),
            PrimaryWindow,
        ))
        .id();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();
    assert!(app.world().resource::<UiRuntime>().chat_focused());
    assert_eq!(
        app.world().resource::<UiRuntime>().chat_editor().as_str(),
        "unsent draft"
    );
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
    app.world_mut().get_mut::<Window>(window).unwrap().focused = true;
    app.update();
    assert!(app.world().resource::<UiRuntime>().chat_focused());
    assert_eq!(
        app.world().resource::<UiRuntime>().chat_editor().as_str(),
        "unsent draft"
    );
}

#[test]
fn chat_input_preserves_buttons_for_the_visible_menu() {
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .insert_resource(MenuRuntime::new(true, 2, "test".into()))
        .add_systems(Update, super::super::drive_chat_keyboard_input);
    app.world_mut()
        .spawn((Window::default(), CursorOptions::default(), PrimaryWindow));
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);

    app.update();

    assert!(
        app.world()
            .resource::<ButtonInput<KeyCode>>()
            .just_pressed(KeyCode::Enter)
    );
    assert!(
        app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert!(!app.world().resource::<UiRuntime>().chat_focused());
}

#[test]
fn chat_typing_consumes_movement_keys_and_mouse_input() {
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<Time<Real>>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<AccumulatedMouseMotion>()
        .insert_resource(UiRuntime::new(1))
        .insert_resource(crate::player_runtime::PlayerRuntime::new(1))
        .add_systems(Update, super::super::drive_chat_keyboard_input);
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
    press_key(&mut app, window, KeyCode::KeyT, Some("t"));
    app.update();
    assert!(app.world().resource::<UiRuntime>().chat_focused());
    assert_eq!(
        app.world().resource::<UiRuntime>().chat_editor().as_str(),
        ""
    );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut()
        .resource_mut::<AccumulatedMouseMotion>()
        .delta = Vec2::ONE;
    press_key(&mut app, window, KeyCode::KeyW, Some("w"));
    app.update();
    assert_eq!(
        app.world().resource::<UiRuntime>().chat_editor().as_str(),
        "w"
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::KeyW)
    );
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert_eq!(
        app.world().resource::<AccumulatedMouseMotion>().delta,
        Vec2::ZERO
    );
    press_key(&mut app, window, KeyCode::Escape, None);
    app.update();
    assert!(!app.world().resource::<UiRuntime>().chat_focused());
    assert!(
        !app.world()
            .resource::<ButtonInput<KeyCode>>()
            .pressed(KeyCode::Escape)
    );
}

#[test]
fn consent_approval_frame_cannot_activate_or_edit_the_underlying_menu() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);

    use crate::server_experiences::input::{ConsentInput, consume};
    use bevy::input::{
        gamepad::{Gamepad, GamepadButton},
        touch::{TouchInput, TouchPhase, touch_screen_input_system},
    };
    let (mut app, window) = menu_input_app(MenuClipboard::default());
    app.insert_resource(ConsentInput(true))
        .add_message::<TouchInput>()
        .add_systems(
            Update,
            (touch_screen_input_system, consume)
                .chain()
                .before(drive_menu_input),
        );
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::PlayAddServer);
    let view = app.world().resource::<MenuRuntime>().view();
    let point = {
        let mut presentation = app.world_mut().resource_mut::<UiPresentationRuntime>();
        presentation.set_menu_view(Some(view));
        presentation
            .build(
                &player_runtime,
                &UiRuntime::new(1),
                0,
                [1280, 720],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        (0..720)
            .step_by(8)
            .flat_map(|y| {
                (0..1280)
                    .step_by(8)
                    .map(move |x| ui::UiPoint::new(x as f32, y as f32).unwrap())
            })
            .find(|point| presentation.hit_test_menu(*point) == Some(MenuAction::AddAddress))
            .unwrap()
    };
    app.world_mut()
        .get_mut::<Window>(window)
        .unwrap()
        .set_cursor_position(Some(Vec2::new(point.x(), point.y())));
    let before = app.world().resource::<MenuRuntime>().view();
    let mut pad = Gamepad::default();
    pad.digital_mut().press(GamepadButton::South);
    pad.digital_mut().press(GamepadButton::DPadDown);
    app.world_mut().spawn(pad);
    queue_key(&mut app, window, KeyCode::Enter, None);
    queue_key(&mut app, window, KeyCode::KeyA, Some("leaked"));
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.world_mut().write_message(TouchInput {
        phase: TouchPhase::Started,
        position: Vec2::new(point.x(), point.y()),
        window,
        force: None,
        id: 1,
    });
    // Approval has already replaced the prompt with its status indicator, but ownership persists.
    app.world_mut()
        .resource_mut::<UiPresentationRuntime>()
        .set_experience_chrome(Some("Approved"), false)
        .unwrap();
    app.update();
    let after = app.world().resource::<MenuRuntime>().view();
    assert_eq!(after.screen, before.screen);
    assert_eq!(after.focused_action, before.focused_action);
    assert_eq!(after.field, before.field);
    assert_eq!(after.name, before.name);
    assert!(
        !app.world()
            .resource::<ButtonInput<MouseButton>>()
            .pressed(MouseButton::Left)
    );
    assert!(app.world().resource::<Messages<KeyboardInput>>().is_empty());
}

#[test]
fn retained_overlay_loss_opens_pause_and_focus_gain_keeps_it_open() {
    let (mut app, window) = menu_input_app(MenuClipboard::with_access(|_| None, |_| {}));
    let mut menu = MenuRuntime::new(false, 2, "test".into());
    let pause = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "pause_menu_on_focus_lost")
        .unwrap();
    menu.activate(MenuAction::SettingsOption(pause as u16, 1));
    app.insert_resource(menu);
    let mut focus = client_presentation::camera::CursorFocus::default();
    focus.focus_changed(false);
    focus.focus_changed(true);
    app.insert_resource(focus);
    app.update();
    assert_eq!(
        app.world().resource::<MenuRuntime>().screen(),
        MenuScreen::Pause
    );
    assert!(app.world().resource::<MenuRuntime>().is_visible());
    app.world_mut()
        .resource_mut::<client_presentation::camera::CursorFocus>()
        .begin_frame(true);
    app.update();
    assert!(app.world().resource::<MenuRuntime>().is_visible());
    press_key(&mut app, window, KeyCode::Escape, None);
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
}

#[test]
fn disabled_focus_pause_still_leaves_overlay_input_released() {
    let (mut app, _) = menu_input_app(MenuClipboard::with_access(|_| None, |_| {}));
    let mut menu = MenuRuntime::new(false, 2, "test".into());
    let pause = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "pause_menu_on_focus_lost")
        .unwrap();
    menu.activate(MenuAction::SettingsOption(pause as u16, 0));
    app.insert_resource(menu);
    let mut focus = client_presentation::camera::CursorFocus::default();
    focus.focus_changed(false);
    app.insert_resource(focus);
    app.update();
    assert!(!app.world().resource::<MenuRuntime>().is_visible());
}
