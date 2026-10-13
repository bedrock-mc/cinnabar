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

/// Matches the command binding, also accepting a typed slash on other layouts
/// while the default command control is selected.
pub(crate) fn command_key(
    menu: Option<&client_shell::menu::MenuRuntime>,
    input: &KeyboardInput,
) -> bool {
    use client_shell::menu::settings_options::control_bindings::named_control;
    binding_key(menu, "key.command", input.key_code)
        || (input.text.as_deref() == Some("/")
            && named_control(menu, "key.command") == named_control(None, "key.command"))
}

/// Whether `key` edits chat by whole words when the word-edit modifier is held.
pub(crate) fn is_word_edit(key: KeyCode) -> bool {
    matches!(
        key,
        KeyCode::Backspace | KeyCode::Delete | KeyCode::ArrowLeft | KeyCode::ArrowRight
    )
}

/// Applies the word edit for `key`; `selecting` extends the selection instead of moving the caret.
pub(crate) fn word_edit(editor: &mut ui::ChatEditor, key: KeyCode, selecting: bool) {
    match key {
        KeyCode::Backspace => editor.backspace_word(),
        KeyCode::Delete => editor.delete_word(),
        KeyCode::ArrowLeft => editor.move_word_left(selecting),
        KeyCode::ArrowRight => editor.move_word_right(selecting),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
