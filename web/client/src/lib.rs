//! Page presentation and browser input; transport and gameplay keep their existing owners.

#[cfg(target_arch = "wasm32")]
pub mod browser;
#[cfg(any(target_arch = "wasm32", test))]
mod relay;

use semantic_input::{KEYBOARD_USAGES, KeyboardKey, KeyboardMouseFrame, MAX_MOUSE_BUTTONS};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Browser observations translated into the shared keyboard/mouse frame.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Default)]
pub struct BrowserInput {
    frame: KeyboardMouseFrame,
}
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
impl BrowserInput {
    /// Creates empty physical input observations.
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen(constructor))]
    pub fn new() -> Self {
        Self::default()
    }
    /// Maps a DOM physical code through the authoritative USB usage table.
    pub fn key(&mut self, code: &str, down: bool) {
        let code = code.replace("Meta", "Super");
        if let Some((_, usage)) = KEYBOARD_USAGES
            .iter()
            .find(|(key, _)| format!("{key:?}") == code)
        {
            update(
                &mut self.frame.keys,
                &mut self.frame.key_edges,
                *usage,
                down,
            );
            self.frame.activity_sequence = self.frame.activity_sequence.saturating_add(1);
            let held = &self.frame.keys;
            self.frame.modifiers = semantic_input::ModifierChord {
                shift: held_key(held, [KeyboardKey::ShiftLeft, KeyboardKey::ShiftRight]),
                control: held_key(held, [KeyboardKey::ControlLeft, KeyboardKey::ControlRight]),
                alt: held_key(held, [KeyboardKey::AltLeft, KeyboardKey::AltRight]),
                super_key: held_key(held, [KeyboardKey::SuperLeft, KeyboardKey::SuperRight]),
            };
        }
    }
    /// Converts DOM buttons to the input pipeline's left/right/middle ordering.
    pub fn mouse(&mut self, button: u8, down: bool) {
        let button = match button {
            1 => 2,
            2 => 1,
            other => other,
        };
        if usize::from(button) < MAX_MOUSE_BUTTONS {
            update(
                &mut self.frame.mouse_buttons,
                &mut self.frame.mouse_edges,
                button,
                down,
            );
            self.frame.activity_sequence = self.frame.activity_sequence.saturating_add(1);
        }
    }
    /// Adds finite pointer-lock movement to the next frame.
    pub fn motion(&mut self, x: f32, y: f32) {
        for (total, delta) in self.frame.mouse_motion.iter_mut().zip([x, y]) {
            if delta.is_finite() {
                *total = (*total + delta).clamp(-1e6, 1e6);
            }
        }
    }
    /// Releases held controls on blur or pointer-lock loss, preserving release edges.
    pub fn release(&mut self) {
        for key in self.frame.keys.clone() {
            update(&mut self.frame.keys, &mut self.frame.key_edges, key, false);
        }
        for button in self.frame.mouse_buttons.clone() {
            update(
                &mut self.frame.mouse_buttons,
                &mut self.frame.mouse_edges,
                button,
                false,
            );
        }
        self.frame.mouse_motion = [0.0; 2];
        self.frame.modifiers = Default::default();
    }
}
impl BrowserInput {
    /// Consumes transient edges once while retaining held controls.
    pub fn take(&mut self) -> KeyboardMouseFrame {
        let frame = self.frame.clone();
        self.frame.key_edges = Default::default();
        self.frame.mouse_edges = Default::default();
        self.frame.mouse_motion = [0.0; 2];
        frame
    }
}

/// Looks up held modifier keys in the shared usage table.
fn held_key(held: &[u16], keys: [KeyboardKey; 2]) -> bool {
    KEYBOARD_USAGES
        .iter()
        .any(|(key, usage)| keys.contains(key) && held.contains(usage))
}

/// Records each edge once per frame and ignores repeated keydown observations.
fn update<T: Copy + PartialEq>(
    held: &mut Vec<T>,
    edges: &mut semantic_input::ButtonEdges<T>,
    value: T,
    down: bool,
) {
    let present = held.contains(&value);
    if present == down {
        return;
    }
    let target = if down {
        held.push(value);
        &mut edges.pressed
    } else {
        held.retain(|item| *item != value);
        &mut edges.released
    };
    if !target.contains(&value) {
        target.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn taps_repeat_blur_and_frame_consumption() {
        let mut input = BrowserInput::default();
        input.key("KeyW", true);
        input.key("KeyW", true);
        input.key("ShiftLeft", true);
        assert!(input.take().modifiers.shift);
        input.key("KeyW", false);
        input.key("KeyW", true);
        input.mouse(2, true);
        input.release();
        let frame = input.take();
        assert!(frame.keys.is_empty());
        assert_eq!(frame.key_edges.pressed.len(), 1);
        assert_eq!(frame.key_edges.released.len(), 2);
        assert!(!frame.modifiers.shift);
        assert_eq!(frame.mouse_edges.pressed, vec![1]);
        assert!(input.take().key_edges.pressed.is_empty());
    }
}
