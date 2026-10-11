//! Desktop device codes shared by menu binding capture and gameplay sampling.
use bevy::prelude::{GamepadButton, KeyCode, MouseButton};

/// Desktop keys and the USB usages settings and gameplay bind them by.
pub const KEYBOARD_USAGES: &[(KeyCode, u16)] = &[
    (KeyCode::KeyA, 0x04),
    (KeyCode::KeyD, 0x07),
    (KeyCode::KeyB, 0x05),
    (KeyCode::KeyC, 0x06),
    (KeyCode::KeyE, 0x08),
    (KeyCode::KeyF, 0x09),
    (KeyCode::KeyG, 0x0a),
    (KeyCode::KeyH, 0x0b),
    (KeyCode::KeyI, 0x0c),
    (KeyCode::KeyJ, 0x0d),
    (KeyCode::KeyK, 0x0e),
    (KeyCode::KeyL, 0x0f),
    (KeyCode::KeyM, 0x10),
    (KeyCode::KeyN, 0x11),
    (KeyCode::KeyO, 0x12),
    (KeyCode::KeyP, 0x13),
    (KeyCode::KeyQ, 0x14),
    (KeyCode::KeyR, 0x15),
    (KeyCode::KeyT, 0x17),
    (KeyCode::KeyU, 0x18),
    (KeyCode::KeyV, 0x19),
    (KeyCode::KeyX, 0x1b),
    (KeyCode::KeyY, 0x1c),
    (KeyCode::KeyZ, 0x1d),
    (KeyCode::KeyS, 0x16),
    (KeyCode::KeyW, 0x1a),
    (KeyCode::Digit1, 0x1e),
    (KeyCode::Digit2, 0x1f),
    (KeyCode::Digit3, 0x20),
    (KeyCode::Digit4, 0x21),
    (KeyCode::Digit5, 0x22),
    (KeyCode::Digit6, 0x23),
    (KeyCode::Digit7, 0x24),
    (KeyCode::Digit8, 0x25),
    (KeyCode::Digit9, 0x26),
    (KeyCode::Digit0, 0x27),
    (KeyCode::Backspace, 0x2a),
    (KeyCode::Enter, 0x28),
    (KeyCode::Escape, 0x29),
    (KeyCode::Tab, 0x2b),
    (KeyCode::Space, 0x2c),
    (KeyCode::Minus, 0x2d),
    (KeyCode::Equal, 0x2e),
    (KeyCode::BracketLeft, 0x2f),
    (KeyCode::BracketRight, 0x30),
    (KeyCode::Backslash, 0x31),
    (KeyCode::Semicolon, 0x33),
    (KeyCode::Quote, 0x34),
    (KeyCode::Backquote, 0x35),
    (KeyCode::Comma, 0x36),
    (KeyCode::Period, 0x37),
    (KeyCode::Slash, 0x38),
    (KeyCode::Insert, 0x49),
    (KeyCode::Home, 0x4a),
    (KeyCode::PageUp, 0x4b),
    (KeyCode::Delete, 0x4c),
    (KeyCode::End, 0x4d),
    (KeyCode::PageDown, 0x4e),
    (KeyCode::F1, 0x3a),
    (KeyCode::F2, 0x3b),
    (KeyCode::F3, 0x3c),
    (KeyCode::F4, 0x3d),
    (KeyCode::F5, 0x3e),
    (KeyCode::F6, 0x3f),
    (KeyCode::F7, 0x40),
    (KeyCode::F8, 0x41),
    (KeyCode::F9, 0x42),
    (KeyCode::F10, 0x43),
    (KeyCode::F11, 0x44),
    (KeyCode::F12, 0x45),
    // The UiFocused defaults bind these four HID usages; without them the
    // arrow keys are dead in every menu.
    (KeyCode::ArrowRight, 0x4f),
    (KeyCode::ArrowLeft, 0x50),
    (KeyCode::ArrowDown, 0x51),
    (KeyCode::ArrowUp, 0x52),
    (KeyCode::ControlLeft, 0xe0),
    (KeyCode::ShiftLeft, 0xe1),
    (KeyCode::AltLeft, 0xe2),
    (KeyCode::SuperLeft, 0xe3),
    (KeyCode::ControlRight, 0xe4),
    (KeyCode::ShiftRight, 0xe5),
    (KeyCode::AltRight, 0xe6),
    (KeyCode::SuperRight, 0xe7),
];

/// Converts a desktop key to the USB usage used by settings and gameplay.
pub fn keyboard_usage(key: KeyCode) -> Option<u16> {
    KEYBOARD_USAGES
        .iter()
        .find_map(|(candidate, usage)| (*candidate == key).then_some(*usage))
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

use bevy::{
    prelude::Mut,
    window::{CursorGrabMode, CursorOptions},
};
/// Releases capture only when needed, avoiding Bevy's repeated OS grab notifications.
pub fn release_cursor(cursor: &mut Mut<CursorOptions>) {
    if cursor.grab_mode != CursorGrabMode::None || !cursor.visible {
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
    }
}
