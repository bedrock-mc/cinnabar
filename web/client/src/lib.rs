//! Page presentation and browser input; transport and gameplay keep their existing owners.

#[cfg(target_arch = "wasm32")]
pub mod browser;
pub mod launcher;
pub mod play;
#[cfg(any(target_arch = "wasm32", test))]
mod relay;

use semantic_input::{KEYBOARD_USAGES, KeyboardKey, KeyboardMouseFrame, MAX_MOUSE_BUTTONS};
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

/// Browser observations translated into the shared keyboard/mouse frame.
#[cfg_attr(target_arch = "wasm32", wasm_bindgen)]
#[derive(Clone, Default)]
pub struct BrowserInput {
    shared: std::rc::Rc<std::cell::RefCell<InputState>>,
}

/// Held controls and focus shared by DOM callbacks and the game frame consumer.
#[derive(Default)]
struct InputState {
    frame: KeyboardMouseFrame,
    focused: bool,
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
        let state = &mut *self.shared.borrow_mut();
        let code = code.replace("Meta", "Super");
        if let Some((_, usage)) = KEYBOARD_USAGES
            .iter()
            .find(|(key, _)| format!("{key:?}") == code)
        {
            update(
                &mut state.frame.keys,
                &mut state.frame.key_edges,
                *usage,
                down,
            );
            state.frame.activity_sequence = state.frame.activity_sequence.saturating_add(1);
            let held = &state.frame.keys;
            state.frame.modifiers = semantic_input::ModifierChord {
                shift: held_key(held, [KeyboardKey::ShiftLeft, KeyboardKey::ShiftRight]),
                control: held_key(held, [KeyboardKey::ControlLeft, KeyboardKey::ControlRight]),
                alt: held_key(held, [KeyboardKey::AltLeft, KeyboardKey::AltRight]),
                super_key: held_key(held, [KeyboardKey::SuperLeft, KeyboardKey::SuperRight]),
            };
        }
    }
    /// Converts DOM buttons to the input pipeline's left/right/middle ordering.
    pub fn mouse(&mut self, button: u8, down: bool) {
        let state = &mut *self.shared.borrow_mut();
        let Some(button) = button.checked_add(1) else {
            return;
        };
        let button = match button {
            2 => 3,
            3 => 2,
            other => other,
        };
        if usize::from(button) <= MAX_MOUSE_BUTTONS {
            update(
                &mut state.frame.mouse_buttons,
                &mut state.frame.mouse_edges,
                button,
                down,
            );
            state.frame.activity_sequence = state.frame.activity_sequence.saturating_add(1);
        }
    }
    /// Adds finite pointer-lock movement to the next frame.
    pub fn motion(&mut self, x: f32, y: f32) {
        let state = &mut *self.shared.borrow_mut();
        for (total, delta) in state.frame.mouse_motion.iter_mut().zip([x, y]) {
            if delta.is_finite() {
                *total = (*total + delta).clamp(-1e6, 1e6);
            }
        }
    }
    /// Sets canvas capture ownership; losing it retires every held control.
    pub fn focus(&mut self, focused: bool) {
        if !focused {
            self.release();
        }
        self.shared.borrow_mut().focused = focused;
    }
    /// Releases held controls on blur or pointer-lock loss, preserving release edges.
    pub fn release(&mut self) {
        let state = &mut *self.shared.borrow_mut();
        state.focused = false;
        for key in state.frame.keys.clone() {
            update(
                &mut state.frame.keys,
                &mut state.frame.key_edges,
                key,
                false,
            );
        }
        for button in state.frame.mouse_buttons.clone() {
            update(
                &mut state.frame.mouse_buttons,
                &mut state.frame.mouse_edges,
                button,
                false,
            );
        }
        state.frame.mouse_motion = [0.0; 2];
        state.frame.modifiers = Default::default();
    }
}
impl BrowserInput {
    /// Consumes the canvas sample at the game's raw-input boundary.
    pub fn take_device(&mut self) -> semantic_input::DeviceFrame {
        let focused = self.shared.borrow().focused;
        let keyboard = self.take();
        semantic_input::DeviceFrame {
            keyboard_mouse: focused.then_some(keyboard),
            window_focus_lost: !focused,
            ..Default::default()
        }
    }
    /// Consumes transient edges once while retaining held controls.
    pub fn take(&mut self) -> KeyboardMouseFrame {
        let state = &mut *self.shared.borrow_mut();
        let frame = state.frame.clone();
        state.frame.key_edges = Default::default();
        state.frame.mouse_edges = Default::default();
        state.frame.mouse_motion = [0.0; 2];
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
    fn canvas_and_game_share_one_consumed_sample_and_focus_loss() {
        let mut canvas = BrowserInput::new();
        let mut game = canvas.clone();
        canvas.focus(true);
        canvas.key("KeyW", true);
        assert_eq!(
            game.take_device()
                .keyboard_mouse
                .unwrap()
                .key_edges
                .pressed
                .len(),
            1
        );
        assert!(
            game.take_device()
                .keyboard_mouse
                .unwrap()
                .key_edges
                .pressed
                .is_empty()
        );
        canvas.release();
        let lost = game.take_device();
        assert!(lost.window_focus_lost);
        assert!(lost.keyboard_mouse.is_none());
        canvas.focus(true);
        let regained = game.take_device();
        assert!(!regained.window_focus_lost);
        assert!(regained.keyboard_mouse.unwrap().keys.is_empty());
    }

    #[test]
    fn browser_dom_mouse_codes_match_persisted_gameplay_bindings() {
        use ::launcher::menu::settings_options::{BindingId, SettingsOptions};
        use semantic_input::PhysicalControl;
        let pick = SettingsOptions::default().named_key_control(BindingId::PickItem);
        for (dom, expected) in [(0, 1), (1, 3), (2, 2), (3, 4), (4, 5), (7, 8)] {
            let mut input = BrowserInput::new();
            input.focus(true);
            input.mouse(dom, true);
            let device = input.take_device();
            semantic_input::SemanticInputRouter::default()
                .route(device.clone())
                .unwrap();
            let keyboard = device.keyboard_mouse.unwrap();
            assert_eq!(keyboard.mouse_edges.pressed, [expected], "DOM button {dom}");
            assert_eq!(keyboard.mouse_buttons, [expected]);
            assert_eq!(
                pick == Some(PhysicalControl::MouseButton(expected)),
                dom == 1
            );
            assert!(
                input
                    .take_device()
                    .keyboard_mouse
                    .unwrap()
                    .mouse_edges
                    .pressed
                    .is_empty()
            );
        }
        let mut input = BrowserInput::new();
        for invalid in [8, u8::MAX] {
            input.mouse(invalid, true);
        }
        assert!(input.take().mouse_buttons.is_empty());
    }

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
        assert_eq!(frame.mouse_edges.pressed, vec![2]);
        assert!(input.take().key_edges.pressed.is_empty());
    }
}
