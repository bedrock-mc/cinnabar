//! Binding capture consumes input before normal menu navigation can handle it.

use bevy::{
    ecs::message::MessageReader,
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
    },
    prelude::{ButtonInput, KeyCode, MouseButton, Query},
};
use semantic_input::{AxisDirection, PhysicalControl};
use {
    crate::{
        menu::MenuRuntime,
        semantic_controls::{
            keyboard_usage,
            physical::{TRANSLATED_GAMEPAD_BUTTONS, mouse_button_code},
        },
    },
    launcher::menu::settings_options::GAMEPAD_OFFSET,
};

/// Captures only the selected device family and allows Escape to cancel either family.
pub(super) fn capture(
    menu: &mut MenuRuntime,
    messages: &mut MessageReader<KeyboardInput>,
    keys: &mut ButtonInput<KeyCode>,
    mouse: &mut ButtonInput<MouseButton>,
    gamepads: &Query<&Gamepad>,
) {
    let gamepad = menu
        .key_remap
        .is_some_and(|index| usize::from(index) >= GAMEPAD_OFFSET);
    for input in messages.read() {
        if input.state != ButtonState::Pressed {
            continue;
        }
        if input.key_code == KeyCode::Escape {
            menu.key_remap = None;
            break;
        }
        if !gamepad && let Some(code) = keyboard_usage(input.key_code) {
            menu.capture_key(PhysicalControl::KeyboardUsage(code));
            break;
        }
    }
    if gamepad && menu.key_remap.is_some() {
        for pad in gamepads {
            let button = TRANSLATED_GAMEPAD_BUTTONS
                .iter()
                .find(|(_, button)| pad.just_pressed(*button))
                .map(|(code, _)| PhysicalControl::GamepadButton(*code));
            let trigger = [
                (GamepadButton::LeftTrigger2, 4),
                (GamepadButton::RightTrigger2, 5),
            ]
            .into_iter()
            .find(|(button, _)| pad.just_pressed(*button))
            .map(|(_, axis)| PhysicalControl::GamepadAxis {
                axis,
                direction: AxisDirection::Positive,
            });
            if let Some(control) = button.or(trigger) {
                menu.capture_key(control);
                break;
            }
        }
    } else if menu.key_remap.is_some()
        && let Some(code) = mouse
            .get_just_pressed()
            .find_map(|button| mouse_button_code(*button))
    {
        menu.capture_key(PhysicalControl::MouseButton(code));
    }
    keys.reset_all();
    mouse.reset_all();
}
