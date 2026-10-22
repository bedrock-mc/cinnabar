//! Translates one backend arrival stream without combining independently ordered readers.

use super::*;
use bevy::{
    ecs::message::MessageCursor, input::ButtonState, prelude::Messages, window::WindowEvent,
};
use semantic_input::{DesktopEvent, DesktopEvents, PhysicalControl};

#[cfg(test)]
mod tests;

/// Drains the backend batch even while capture is unavailable; missing order fails closed.
pub(super) fn collect(
    inputs: &SemanticPhysicalInputs,
    messages: Option<&Messages<WindowEvent>>,
    cursor: &mut MessageCursor<WindowEvent>,
) -> Option<(DesktopEvents, bool)> {
    let messages = messages?;
    let window = inputs.window.0;
    let mut batch = Vec::new();
    let mut output = DesktopEvents::default();
    let mut lost = false;
    let reserved = |name: &str| {
        inputs
            .reserved
            .as_ref()
            .is_some_and(|reserved| reserved.0.iter().any(|key| key == name))
    };
    for event in cursor.read(messages) {
        let relevant = match event {
            WindowEvent::KeyboardInput(event) => {
                event.window == window
                    && !event.repeat
                    && !reserved(&format!("{:?}", event.key_code))
            }
            WindowEvent::MouseButtonInput(event) => {
                event.window == window && !reserved(&format!("Mouse{:?}", event.button))
            }
            WindowEvent::MouseWheel(event) => event.window == window,
            WindowEvent::KeyboardFocusLost(_) => {
                lost = true;
                false
            }
            WindowEvent::WindowFocused(event) => {
                lost |= event.window == window && !event.focused;
                false
            }
            WindowEvent::WindowOccluded(event) => {
                lost |= event.window == window && event.occluded;
                false
            }
            _ => false,
        };
        if relevant {
            batch.push(event.clone());
        }
    }
    let mut keys = (*inputs.keys).clone();
    for event in batch.iter().rev() {
        if let WindowEvent::KeyboardInput(event) = event {
            change_key(
                &mut keys,
                event.key_code,
                event.state != ButtonState::Pressed,
            );
        }
    }
    output.set_initial_modifiers(modifiers(&keys));
    for event in batch {
        let button = match event {
            WindowEvent::KeyboardInput(event) => {
                let pressed = event.state == ButtonState::Pressed;
                change_key(&mut keys, event.key_code, pressed);
                keyboard_usage(event.key_code)
                    .map(|code| (PhysicalControl::KeyboardUsage(code), pressed))
            }
            WindowEvent::MouseButtonInput(event) => mouse_button_code(event.button).map(|code| {
                (
                    PhysicalControl::MouseButton(code),
                    event.state == ButtonState::Pressed,
                )
            }),
            WindowEvent::MouseWheel(event) => {
                if event.y.is_finite() && event.y != 0.0 {
                    output.push(DesktopEvent::Scroll {
                        next: event.y < 0.0,
                    });
                }
                None
            }
            _ => None,
        };
        if let Some((control, pressed)) = button {
            output.push(DesktopEvent::Button {
                control,
                pressed,
                modifiers: modifiers(&keys),
            });
        }
    }
    Some((output, lost))
}

/// Updates physical keyboard state while reconstructing the batch's modifier timeline.
fn change_key(keys: &mut ButtonInput<KeyCode>, key: KeyCode, pressed: bool) {
    if pressed {
        keys.press(key);
    } else {
        keys.release(key);
    }
}

/// Resolves both physical sides of each modifier from the event-time held state.
fn modifiers(keys: &ButtonInput<KeyCode>) -> ModifierChord {
    ModifierChord {
        shift: keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight),
        control: keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight),
        alt: keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight),
        super_key: keys.pressed(KeyCode::SuperLeft) || keys.pressed(KeyCode::SuperRight),
    }
}
