//! Interaction behavior is independent of the bootstrap renderer.

use super::*;

#[test]
fn keyboard_and_controller_focus_wrap_and_activate_cancel() {
    for source in [Source::Keyboard, Source::Controller] {
        let mut input = Input::default();
        input.reset(&Screen::Consent);
        input.navigate(&Screen::Consent, true);
        assert_eq!(input.focused, Some(Action::Quit));
        assert!(input.focus_visible);
        input.navigate(&Screen::Consent, false);
        assert_eq!(input.focused, Some(Action::Accept));
        input.reset(&Screen::Starting);
        input.press(&Screen::Starting, source, None);
        assert_eq!(input.pressed(None), Some(Action::Quit));
        assert_eq!(input.release(source, None), Some(Action::Quit));
        assert_eq!(input.release(source, None), None);
    }
}

#[test]
fn cancel_press_survives_download_and_unpack_reports() {
    let download = Screen::Downloading {
        received: 50,
        total: Some(100),
        bytes_per_second: None,
    };
    let unpack = Screen::Preparing {
        step: 1,
        total: 2,
        label: "Unpacking".into(),
    };
    for source in [Source::Keyboard, Source::Controller, Source::Pointer] {
        let mut input = Input::default();
        input.reset(&Screen::Starting);
        input.press(&Screen::Starting, source, Some(Action::Quit));
        input.update_screen(&Screen::Starting, &download);
        input.update_screen(&download, &unpack);
        assert_eq!(
            input.release(source, Some(Action::Quit)),
            Some(Action::Quit)
        );
        input.update_screen(
            &unpack,
            &Screen::Failed {
                message: "offline".into(),
            },
        );
        assert_eq!(input.focused, Some(Action::Retry));
    }
}

#[test]
fn pointer_drag_out_blur_and_screen_changes_cancel_presses() {
    let mut input = Input::default();
    input.reset(&Screen::Starting);
    input.press(&Screen::Starting, Source::Pointer, Some(Action::Quit));
    assert_eq!(input.pressed(None), None);
    assert_eq!(input.release(Source::Pointer, None), None);
    input.press(&Screen::Starting, Source::Keyboard, None);
    input.blur();
    assert_eq!(input.release(Source::Keyboard, None), None);
    input.press(&Screen::Starting, Source::Keyboard, None);
    input.reset(&Screen::Failed {
        message: "offline".into(),
    });
    assert_eq!(input.focused, Some(Action::Retry));
    assert_eq!(input.release(Source::Keyboard, None), None);
    input.reset(&Screen::Done);
    input.navigate(&Screen::Done, false);
    assert_eq!(input.focused, None);
}

#[test]
fn retry_requires_matching_release_and_pointer_click_stays_hidden() {
    let screen = Screen::Failed {
        message: "offline".into(),
    };
    let mut input = Input::default();
    input.reset(&screen);
    input.press(&screen, Source::Pointer, Some(Action::Retry));
    assert!(!input.focus_visible);
    assert_eq!(input.release(Source::Keyboard, None), None);
    assert_eq!(
        input.release(Source::Pointer, Some(Action::Retry)),
        Some(Action::Retry)
    );
}

#[test]
fn controller_confirmation_uses_saved_ab_swap() {
    use gilrs::Button;
    for swapped in [false, true] {
        let settings = if swapped {
            SettingsOptions::decode(br#"{"values":{"swap_gamepad_ab_buttons":1}}"#).unwrap()
        } else {
            SettingsOptions::default()
        };
        let (confirm, cancel) = if swapped {
            (Button::East, Button::South)
        } else {
            (Button::South, Button::East)
        };
        assert_eq!(
            controller_button(confirm, true, &settings),
            Some(Command::Press)
        );
        assert_eq!(
            controller_button(confirm, false, &settings),
            Some(Command::Release)
        );
        assert_eq!(
            controller_button(cancel, true, &settings),
            Some(Command::Cancel)
        );
        assert_eq!(controller_button(cancel, false, &settings), None);
        let mut input = Input::default();
        input.reset(&Screen::Consent);
        let press = controller_button(confirm, true, &settings).unwrap();
        assert_eq!(
            input.apply(&Screen::Consent, Source::Controller, press, None),
            (None, true)
        );
        assert_eq!(input.pressed(None), Some(Action::Accept));
        let release = controller_button(confirm, false, &settings).unwrap();
        assert_eq!(
            input.apply(&Screen::Consent, Source::Controller, release, None),
            (Some(Action::Accept), true)
        );
    }
}

#[test]
fn unchanged_keyboard_input_does_not_dirty_overlay() {
    let screen = Screen::Starting;
    let mut input = Input::default();
    input.reset(&screen);
    for key in [Key::Character("a".into()), Key::Named(NamedKey::Shift)] {
        for state in [ElementState::Pressed, ElementState::Released] {
            assert_eq!(input.keyboard(&screen, &key, state, false), (None, false));
        }
    }
    assert_eq!(
        input.keyboard(
            &screen,
            &Key::Named(NamedKey::Tab),
            ElementState::Released,
            false
        ),
        (None, false)
    );
    assert_eq!(
        input.keyboard(
            &screen,
            &Key::Named(NamedKey::Enter),
            ElementState::Released,
            false
        ),
        (None, false)
    );
    assert_eq!(
        input.keyboard(
            &screen,
            &Key::Named(NamedKey::Tab),
            ElementState::Pressed,
            false
        ),
        (None, true)
    );
    assert_eq!(
        input.keyboard(
            &screen,
            &Key::Named(NamedKey::Tab),
            ElementState::Pressed,
            false
        ),
        (None, false)
    );
    assert_eq!(
        input.keyboard(
            &screen,
            &Key::Named(NamedKey::Enter),
            ElementState::Pressed,
            false
        ),
        (None, true)
    );
    assert_eq!(
        input.keyboard(
            &screen,
            &Key::Named(NamedKey::Enter),
            ElementState::Released,
            false
        ),
        (Some(Action::Quit), true)
    );
}

#[test]
fn unmatched_pointer_and_controller_events_do_not_dirty_overlay() {
    let screen = Screen::Starting;
    let mut input = Input::default();
    input.reset(&screen);
    for command in [Command::Press, Command::Release, Command::Blur] {
        assert_eq!(
            input.apply(&screen, Source::Pointer, command, None),
            (None, false)
        );
    }
    assert_eq!(
        input.apply(&screen, Source::Controller, Command::Release, None),
        (None, false)
    );
    assert_eq!(
        controller(gilrs::EventType::Disconnected, &SettingsOptions::default()),
        Some(Command::Blur)
    );
}
