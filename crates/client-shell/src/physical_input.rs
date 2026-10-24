//! Bevy device codes shared by menu binding capture, UI input and gameplay sampling.
use bevy::{
    input::mouse::MouseScrollUnit,
    prelude::{GamepadButton, KeyCode, MouseButton, Mut},
    window::{CursorGrabMode, CursorOptions},
};
use semantic_input::{KeyboardKey, ScrollUnit};

/// Bevy key codes for every desktop key that has a HID usage.
pub const DESKTOP_KEYS: &[(KeyCode, KeyboardKey)] = &[
    (KeyCode::KeyA, KeyboardKey::KeyA),
    (KeyCode::KeyD, KeyboardKey::KeyD),
    (KeyCode::KeyB, KeyboardKey::KeyB),
    (KeyCode::KeyC, KeyboardKey::KeyC),
    (KeyCode::KeyE, KeyboardKey::KeyE),
    (KeyCode::KeyF, KeyboardKey::KeyF),
    (KeyCode::KeyG, KeyboardKey::KeyG),
    (KeyCode::KeyH, KeyboardKey::KeyH),
    (KeyCode::KeyI, KeyboardKey::KeyI),
    (KeyCode::KeyJ, KeyboardKey::KeyJ),
    (KeyCode::KeyK, KeyboardKey::KeyK),
    (KeyCode::KeyL, KeyboardKey::KeyL),
    (KeyCode::KeyM, KeyboardKey::KeyM),
    (KeyCode::KeyN, KeyboardKey::KeyN),
    (KeyCode::KeyO, KeyboardKey::KeyO),
    (KeyCode::KeyP, KeyboardKey::KeyP),
    (KeyCode::KeyQ, KeyboardKey::KeyQ),
    (KeyCode::KeyR, KeyboardKey::KeyR),
    (KeyCode::KeyT, KeyboardKey::KeyT),
    (KeyCode::KeyU, KeyboardKey::KeyU),
    (KeyCode::KeyV, KeyboardKey::KeyV),
    (KeyCode::KeyX, KeyboardKey::KeyX),
    (KeyCode::KeyY, KeyboardKey::KeyY),
    (KeyCode::KeyZ, KeyboardKey::KeyZ),
    (KeyCode::KeyS, KeyboardKey::KeyS),
    (KeyCode::KeyW, KeyboardKey::KeyW),
    (KeyCode::Digit1, KeyboardKey::Digit1),
    (KeyCode::Digit2, KeyboardKey::Digit2),
    (KeyCode::Digit3, KeyboardKey::Digit3),
    (KeyCode::Digit4, KeyboardKey::Digit4),
    (KeyCode::Digit5, KeyboardKey::Digit5),
    (KeyCode::Digit6, KeyboardKey::Digit6),
    (KeyCode::Digit7, KeyboardKey::Digit7),
    (KeyCode::Digit8, KeyboardKey::Digit8),
    (KeyCode::Digit9, KeyboardKey::Digit9),
    (KeyCode::Digit0, KeyboardKey::Digit0),
    (KeyCode::Backspace, KeyboardKey::Backspace),
    (KeyCode::Enter, KeyboardKey::Enter),
    (KeyCode::Escape, KeyboardKey::Escape),
    (KeyCode::Tab, KeyboardKey::Tab),
    (KeyCode::Space, KeyboardKey::Space),
    (KeyCode::Minus, KeyboardKey::Minus),
    (KeyCode::Equal, KeyboardKey::Equal),
    (KeyCode::BracketLeft, KeyboardKey::BracketLeft),
    (KeyCode::BracketRight, KeyboardKey::BracketRight),
    (KeyCode::Backslash, KeyboardKey::Backslash),
    (KeyCode::Semicolon, KeyboardKey::Semicolon),
    (KeyCode::Quote, KeyboardKey::Quote),
    (KeyCode::Backquote, KeyboardKey::Backquote),
    (KeyCode::Comma, KeyboardKey::Comma),
    (KeyCode::Period, KeyboardKey::Period),
    (KeyCode::Slash, KeyboardKey::Slash),
    (KeyCode::Insert, KeyboardKey::Insert),
    (KeyCode::Home, KeyboardKey::Home),
    (KeyCode::PageUp, KeyboardKey::PageUp),
    (KeyCode::Delete, KeyboardKey::Delete),
    (KeyCode::End, KeyboardKey::End),
    (KeyCode::PageDown, KeyboardKey::PageDown),
    (KeyCode::F1, KeyboardKey::F1),
    (KeyCode::F2, KeyboardKey::F2),
    (KeyCode::F3, KeyboardKey::F3),
    (KeyCode::F4, KeyboardKey::F4),
    (KeyCode::F5, KeyboardKey::F5),
    (KeyCode::F6, KeyboardKey::F6),
    (KeyCode::F7, KeyboardKey::F7),
    (KeyCode::F8, KeyboardKey::F8),
    (KeyCode::F9, KeyboardKey::F9),
    (KeyCode::F10, KeyboardKey::F10),
    (KeyCode::F11, KeyboardKey::F11),
    (KeyCode::F12, KeyboardKey::F12),
    (KeyCode::ArrowRight, KeyboardKey::ArrowRight),
    (KeyCode::ArrowLeft, KeyboardKey::ArrowLeft),
    (KeyCode::ArrowDown, KeyboardKey::ArrowDown),
    (KeyCode::ArrowUp, KeyboardKey::ArrowUp),
    (KeyCode::CapsLock, KeyboardKey::CapsLock),
    (KeyCode::PrintScreen, KeyboardKey::PrintScreen),
    (KeyCode::ScrollLock, KeyboardKey::ScrollLock),
    (KeyCode::Pause, KeyboardKey::Pause),
    (KeyCode::NumLock, KeyboardKey::NumLock),
    (KeyCode::NumpadDivide, KeyboardKey::NumpadDivide),
    (KeyCode::NumpadMultiply, KeyboardKey::NumpadMultiply),
    (KeyCode::NumpadSubtract, KeyboardKey::NumpadSubtract),
    (KeyCode::NumpadAdd, KeyboardKey::NumpadAdd),
    (KeyCode::NumpadEnter, KeyboardKey::NumpadEnter),
    (KeyCode::Numpad1, KeyboardKey::Numpad1),
    (KeyCode::Numpad2, KeyboardKey::Numpad2),
    (KeyCode::Numpad3, KeyboardKey::Numpad3),
    (KeyCode::Numpad4, KeyboardKey::Numpad4),
    (KeyCode::Numpad5, KeyboardKey::Numpad5),
    (KeyCode::Numpad6, KeyboardKey::Numpad6),
    (KeyCode::Numpad7, KeyboardKey::Numpad7),
    (KeyCode::Numpad8, KeyboardKey::Numpad8),
    (KeyCode::Numpad9, KeyboardKey::Numpad9),
    (KeyCode::Numpad0, KeyboardKey::Numpad0),
    (KeyCode::NumpadDecimal, KeyboardKey::NumpadDecimal),
    (KeyCode::IntlBackslash, KeyboardKey::IntlBackslash),
    (KeyCode::ContextMenu, KeyboardKey::ContextMenu),
    (KeyCode::NumpadEqual, KeyboardKey::NumpadEqual),
    (KeyCode::F13, KeyboardKey::F13),
    (KeyCode::F14, KeyboardKey::F14),
    (KeyCode::F15, KeyboardKey::F15),
    (KeyCode::F16, KeyboardKey::F16),
    (KeyCode::F17, KeyboardKey::F17),
    (KeyCode::F18, KeyboardKey::F18),
    (KeyCode::F19, KeyboardKey::F19),
    (KeyCode::F20, KeyboardKey::F20),
    (KeyCode::F21, KeyboardKey::F21),
    (KeyCode::F22, KeyboardKey::F22),
    (KeyCode::F23, KeyboardKey::F23),
    (KeyCode::F24, KeyboardKey::F24),
    (KeyCode::NumpadComma, KeyboardKey::NumpadComma),
    (KeyCode::ControlLeft, KeyboardKey::ControlLeft),
    (KeyCode::ShiftLeft, KeyboardKey::ShiftLeft),
    (KeyCode::AltLeft, KeyboardKey::AltLeft),
    (KeyCode::SuperLeft, KeyboardKey::SuperLeft),
    (KeyCode::ControlRight, KeyboardKey::ControlRight),
    (KeyCode::ShiftRight, KeyboardKey::ShiftRight),
    (KeyCode::AltRight, KeyboardKey::AltRight),
    (KeyCode::SuperRight, KeyboardKey::SuperRight),
];

/// The semantic key a Bevy key code reports.
pub fn keyboard_key(key: KeyCode) -> KeyboardKey {
    DESKTOP_KEYS
        .iter()
        .find_map(|(candidate, semantic)| (*candidate == key).then_some(*semantic))
        .unwrap_or(KeyboardKey::Unidentified)
}

/// Converts a desktop key to the USB usage used by settings and gameplay.
pub fn keyboard_usage(key: KeyCode) -> Option<u16> {
    keyboard_key(key).usage()
}

/// The wheel unit a Bevy scroll message reports.
pub const fn scroll_unit(unit: MouseScrollUnit) -> ScrollUnit {
    match unit {
        MouseScrollUnit::Line => ScrollUnit::Line,
        MouseScrollUnit::Pixel => ScrollUnit::Pixel,
    }
}

/// Converts desktop mouse buttons to the persisted gameplay binding codes.
pub fn mouse_button_code(button: MouseButton) -> Option<u8> {
    Some(match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 3,
        MouseButton::Back => 4,
        MouseButton::Forward => 5,
        MouseButton::Other(code) => u8::try_from(code).ok()?.checked_add(1)?,
    })
}

/// The exact gamepad buttons this layer translates, and the binding codes they
/// produce. This is the single source of truth: `gamepad_button_codes` reads it
/// to build a frame, and the binding-reachability test reads it to prove no
/// default binding names a code the app cannot emit.
pub const TRANSLATED_GAMEPAD_BUTTONS: &[(u8, GamepadButton)] = &[
    (0, GamepadButton::South),
    (1, GamepadButton::East),
    (2, GamepadButton::North),
    (3, GamepadButton::West),
    (4, GamepadButton::LeftTrigger),
    (5, GamepadButton::RightTrigger),
    (6, GamepadButton::Select),
    (7, GamepadButton::Start),
    (8, GamepadButton::LeftThumb),
    (9, GamepadButton::RightThumb),
    (11, GamepadButton::DPadUp),
    (12, GamepadButton::DPadDown),
    (13, GamepadButton::DPadLeft),
    (14, GamepadButton::DPadRight),
];

/// Whether the window backend accepts cursor grab and release requests.
pub const OS_CURSOR_GRAB: bool = !cfg!(target_os = "android");

/// Updates OS capture only on supported platforms and only when its state changes.
pub fn set_cursor_capture(cursor: &mut Mut<CursorOptions>, captured: bool) {
    set_cursor_capture_supported(cursor, captured, OS_CURSOR_GRAB);
}

/// Leaves cursor options untouched when even an ungrab request is unsupported.
fn set_cursor_capture_supported(cursor: &mut Mut<CursorOptions>, captured: bool, supported: bool) {
    if !supported {
        return;
    }
    let mode = if captured {
        CursorGrabMode::Locked
    } else {
        CursorGrabMode::None
    };
    if cursor.grab_mode != mode || cursor.visible == captured {
        cursor.grab_mode = mode;
        cursor.visible = !captured;
    }
}

/// Releases capture only when needed, avoiding Bevy's repeated OS grab notifications.
pub fn release_cursor(cursor: &mut Mut<CursorOptions>) {
    set_cursor_capture(cursor, false);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unsupported backends must never see a changed cursor, including release requests.
    #[test]
    fn unsupported_capture_transitions_do_not_notify_the_window_backend() {
        use bevy::prelude::{DetectChanges, World};
        let mut world = World::new();
        let entity = world.spawn(CursorOptions::default()).id();
        world.clear_trackers();
        for captured in [false, true, true, false, true, false] {
            let mut cursor = world.get_mut::<CursorOptions>(entity).unwrap();
            set_cursor_capture_supported(&mut cursor, captured, false);
            assert_eq!(cursor.grab_mode, CursorGrabMode::None);
            assert!(cursor.visible);
            assert!(!cursor.is_changed());
        }
    }

    /// Only a real capture transition should schedule another backend request.
    #[test]
    fn unchanged_capture_does_not_notify_the_window_backend() {
        use bevy::prelude::{DetectChanges, World};
        let mut world = World::new();
        let entity = world.spawn(CursorOptions::default()).id();
        for captured in [true, false] {
            world.clear_trackers();
            {
                let mut cursor = world.get_mut::<CursorOptions>(entity).unwrap();
                set_cursor_capture_supported(&mut cursor, captured, true);
                assert!(cursor.is_changed());
                assert_eq!(cursor.visible, !captured);
                assert_eq!(
                    cursor.grab_mode,
                    if captured {
                        CursorGrabMode::Locked
                    } else {
                        CursorGrabMode::None
                    }
                );
            }
            world.clear_trackers();
            let mut cursor = world.get_mut::<CursorOptions>(entity).unwrap();
            set_cursor_capture_supported(&mut cursor, captured, true);
            assert!(!cursor.is_changed());
        }
    }

    /// Each Bevy key reports a distinct semantic key, and every keyed semantic variant has one.
    #[test]
    fn desktop_keys_cover_every_keyed_variant_once() {
        let keyed = KeyboardKey::Unidentified as usize;
        let mut variants: Vec<_> = DESKTOP_KEYS.iter().map(|(_, key)| *key as usize).collect();
        variants.sort_unstable();
        assert_eq!(variants, (0..keyed).collect::<Vec<_>>());
        let codes: std::collections::HashSet<_> =
            DESKTOP_KEYS.iter().map(|(code, _)| *code).collect();
        assert_eq!(codes.len(), DESKTOP_KEYS.len());
        for (code, key) in DESKTOP_KEYS {
            assert_eq!(keyboard_key(*code), *key);
            assert!(keyboard_usage(*code).is_some(), "{code:?}");
        }
    }
}
