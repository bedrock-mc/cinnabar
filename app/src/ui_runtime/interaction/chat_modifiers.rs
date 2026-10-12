//! Physical modifiers survive gameplay-button resets while chat or mod screens own input.
use super::*;

const MODIFIERS: [KeyCode; 8] = [
    KeyCode::ControlLeft,
    KeyCode::ControlRight,
    KeyCode::SuperLeft,
    KeyCode::SuperRight,
    KeyCode::AltLeft,
    KeyCode::AltRight,
    KeyCode::ShiftLeft,
    KeyCode::ShiftRight,
];

/// Seeds held keys without applying a frame's later modifier edges before earlier presses.
pub(crate) fn capture_before_events(
    held: &mut ButtonInput<KeyCode>,
    gameplay: &ButtonInput<KeyCode>,
    events: &[KeyboardInput],
) {
    for key in MODIFIERS {
        if gameplay.pressed(key) && !events.iter().any(|event| event.key_code == key) {
            held.press(key);
        }
    }
}

/// Applies one physical modifier edge, retaining the other side independently.
pub(crate) fn track(held: &mut ButtonInput<KeyCode>, input: &KeyboardInput) {
    if !MODIFIERS.contains(&input.key_code) {
        return;
    }
    match input.state {
        ButtonState::Pressed => held.press(input.key_code),
        ButtonState::Released => held.release(input.key_code),
    }
}

/// Whether held modifiers turn a press into an edit shortcut rather than typed text.
///
/// Alt is excluded because AltGr and Option, alone or reported as Ctrl+Alt on Windows,
/// are how many keyboard layouts produce characters such as `@`, `{` and `ą`.
pub(super) fn text_modified(held: &ButtonInput<KeyCode>) -> bool {
    let held = held
        .get_pressed()
        .map(|key| keyboard_key(*key))
        .collect::<Vec<_>>();
    is_chat_edit_shortcut(&held)
}

/// Whether either physical Alt key is held, for vanilla's Alt+key command macros.
pub(crate) fn alt_held(held: &ButtonInput<KeyCode>) -> bool {
    held.pressed(KeyCode::AltLeft) || held.pressed(KeyCode::AltRight)
}

#[cfg(test)]
mod tests;
