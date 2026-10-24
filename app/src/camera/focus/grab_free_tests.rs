use super::*;
use bevy::window::CursorGrabMode;

/// Without an OS grab, gameplay holds input exactly where desktop would lock the cursor, so a
/// hardware keyboard drives gameplay and screens still own their input.
#[test]
fn grab_free_capture_follows_the_desktop_capture_rules() {
    let gameplay = client_presentation::observations::CursorPolicy {
        capture_allowed: true,
        consent: false,
        absorbs_input: false,
        steals_mouse: Some(true),
        driven: false,
    };
    assert!(super::grab_free_capture(&gameplay, true));
    assert!(!super::grab_free_capture(&gameplay, false), "unfocused");
    for blocked in [
        client_presentation::observations::CursorPolicy {
            capture_allowed: false,
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            consent: true,
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            absorbs_input: true,
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            steals_mouse: Some(false),
            ..gameplay
        },
        client_presentation::observations::CursorPolicy {
            driven: true,
            ..gameplay
        },
    ] {
        assert!(!super::grab_free_capture(&blocked, true));
    }
}

/// Focus and screen capture still gate camera motion when the OS pointer is never grabbed.
#[test]
fn unsupported_grab_uses_gameplay_capture_for_camera_input() {
    let mut focus = CursorFocus::default();
    let mut window = Window::default();
    let cursor = CursorOptions::default();
    for (focused, screen_open, expected) in [
        (true, false, true),
        (true, true, false),
        (true, false, true),
        (false, false, false),
        (true, false, true),
    ] {
        window.focused = focused;
        focus.begin_frame(focused);
        focus.focus_changed(focused);
        let policy = client_presentation::observations::CursorPolicy {
            capture_allowed: focus.allow_capture(screen_open, true),
            driven: false,
            consent: false,
            absorbs_input: screen_open,
            steals_mouse: Some(!screen_open),
        };
        focus.set_grab_free_capture(super::grab_free_capture(&policy, focused));
        assert_eq!(
            super::mouse_input_active_with_grab(&window, &cursor, Some(&focus), false, false),
            expected,
        );
        assert_eq!(cursor.grab_mode, CursorGrabMode::None);
    }
    assert!(super::mouse_input_active_with_grab(
        &window,
        &cursor,
        Some(&focus),
        true,
        false
    ));
}
