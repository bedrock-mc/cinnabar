//! Physical modifiers survive the gameplay-button reset while chat owns input.
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

pub(super) fn capture(held: &mut ButtonInput<KeyCode>, gameplay: &ButtonInput<KeyCode>) {
    for key in MODIFIERS {
        if gameplay.pressed(key) {
            held.press(key);
        }
    }
}

pub(super) fn track(held: &mut ButtonInput<KeyCode>, input: &KeyboardInput) {
    if !MODIFIERS.contains(&input.key_code) {
        return;
    }
    match input.state {
        ButtonState::Pressed => held.press(input.key_code),
        ButtonState::Released => held.release(input.key_code),
    }
}

pub(super) fn text_modified(held: &ButtonInput<KeyCode>) -> bool {
    MODIFIERS[..6].iter().any(|key| held.pressed(*key))
}

#[cfg(test)]
mod tests;
