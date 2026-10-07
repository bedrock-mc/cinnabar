//! Physical button events remain separate from the current held state.

use crate::{DeviceFrame, InputChord, PhysicalControl};

/// Resolves a bound button's accumulated edges without inventing a held state for a tap.
pub(crate) fn button_edges(chord: InputChord, frame: &DeviceFrame) -> (bool, bool) {
    match chord.control {
        PhysicalControl::KeyboardUsage(code) => {
            frame
                .keyboard_mouse
                .as_ref()
                .map_or((false, false), |sample| {
                    (
                        chord.modifiers.is_satisfied_by(sample.modifiers)
                            && sample.key_edges.pressed.contains(&code),
                        sample.key_edges.released.contains(&code),
                    )
                })
        }
        PhysicalControl::MouseButton(button) => {
            frame
                .keyboard_mouse
                .as_ref()
                .map_or((false, false), |sample| {
                    (
                        chord.modifiers.is_satisfied_by(sample.modifiers)
                            && sample.mouse_edges.pressed.contains(&button),
                        sample.mouse_edges.released.contains(&button),
                    )
                })
        }
        PhysicalControl::GamepadButton(button) => (
            frame
                .controllers
                .iter()
                .any(|sample| sample.button_edges.pressed.contains(&button)),
            frame
                .controllers
                .iter()
                .any(|sample| sample.button_edges.released.contains(&button)),
        ),
        _ => (false, false),
    }
}

/// Consumes finalized events so later authority changes inspect only the current held state.
pub(crate) fn discard_button_edges(frame: &mut DeviceFrame) {
    if let Some(keyboard) = frame.keyboard_mouse.as_mut() {
        keyboard.key_edges = Default::default();
        keyboard.mouse_edges = Default::default();
    }
    for controller in &mut frame.controllers {
        controller.button_edges = Default::default();
    }
}
