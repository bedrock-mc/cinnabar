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

use crate::{
    menu::{MenuAction, MenuClipboard, MenuField, MenuRuntime, MenuScreen, drive_menu_input},
    ui_runtime::{
        UiRuntime,
        presentation::{UiPresentationRuntime, tests::fixture_font},
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn menu_input_app(clipboard: MenuClipboard) -> (App, Entity) {
    let mut app = App::new();
    app.add_message::<KeyboardInput>()
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<Touches>()
        .insert_resource(UiPresentationRuntime::new(fixture_font()).unwrap())
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

    press_key(&mut app, window, KeyCode::Tab, None);
    press_key(&mut app, window, KeyCode::Tab, None);
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

    press_key(&mut app, window, KeyCode::KeyA, Some("a"));
    press_key(&mut app, window, KeyCode::KeyC, Some("c"));
    assert_eq!(copied.lock().unwrap().as_deref(), Some("server-🌍"));
    press_key(&mut app, window, KeyCode::Backspace, None);
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "");

    release_key(&mut app, window, KeyCode::ControlLeft);
    press_key(&mut app, window, KeyCode::KeyE, Some("\u{1}é"));
    assert_eq!(app.world().resource::<MenuRuntime>().view().name, "é");

    app.world_mut()
        .entity_mut(window)
        .get_mut::<Window>()
        .unwrap()
        .focused = false;
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::ControlLeft);
    press_key(&mut app, window, KeyCode::KeyV, Some("v"));
    assert_eq!(read_count.load(Ordering::Relaxed), 1);
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
        menu.mark_connected();
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
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().screen,
        MenuScreen::Home
    );

    {
        let mut menu = app.world_mut().resource_mut::<MenuRuntime>();
        menu.mark_connected();
        menu.open_pause();
        menu.activate(MenuAction::PauseSettings);
        menu.mark_disconnected();
        menu.mark_connecting();
        menu.mark_connected();
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    }
    press_key(&mut app, window, KeyCode::Escape, None);
    assert_eq!(
        app.world().resource::<MenuRuntime>().view().screen,
        MenuScreen::Home
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
