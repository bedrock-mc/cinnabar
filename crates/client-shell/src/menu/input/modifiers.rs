//! Tracks both sides of each editing modifier across consumed keyboard events.

use super::{ButtonInput, ButtonState, KeyCode, KeyboardInput};

#[derive(Default)]
pub struct MenuModifiers(pub(super) u8);

impl MenuModifiers {
    const CONTROL_LEFT: u8 = 1 << 0;
    const CONTROL_RIGHT: u8 = 1 << 1;
    const SUPER_LEFT: u8 = 1 << 2;
    const SUPER_RIGHT: u8 = 1 << 3;
    const ALT_LEFT: u8 = 1 << 4;
    const ALT_RIGHT: u8 = 1 << 5;
    const SHIFT_LEFT: u8 = 1 << 6;
    const SHIFT_RIGHT: u8 = 1 << 7;

    pub(super) fn capture_pressed(&mut self, keys: &ButtonInput<KeyCode>) {
        for key in [
            KeyCode::ControlLeft,
            KeyCode::ControlRight,
            KeyCode::SuperLeft,
            KeyCode::SuperRight,
            KeyCode::AltLeft,
            KeyCode::AltRight,
            KeyCode::ShiftLeft,
            KeyCode::ShiftRight,
        ] {
            if keys.pressed(key) {
                self.0 |= Self::mask(key);
            }
        }
    }

    pub(super) fn observe(&mut self, input: &KeyboardInput) {
        let mask = Self::mask(input.key_code);
        if input.state == ButtonState::Pressed {
            self.0 |= mask;
        } else {
            self.0 &= !mask;
        }
    }

    /// Current physical modifiers for host-owned layout editing.
    pub(super) fn field(&self) -> ui::mod_panel::FieldModifiers {
        ui::mod_panel::FieldModifiers {
            shift: self.shift(),
            control: self.0 & 3 != 0,
            super_key: self.0 & 12 != 0,
            alt: self.0 & 48 != 0,
        }
    }

    pub(super) fn shortcut(&self) -> bool {
        self.0 & 0b0000_1111 != 0 && self.0 & 0b0011_0000 == 0
    }

    pub(super) fn shift(&self) -> bool {
        self.0 & 0b1100_0000 != 0
    }

    const fn mask(key: KeyCode) -> u8 {
        match key {
            KeyCode::ControlLeft => Self::CONTROL_LEFT,
            KeyCode::ControlRight => Self::CONTROL_RIGHT,
            KeyCode::SuperLeft => Self::SUPER_LEFT,
            KeyCode::SuperRight => Self::SUPER_RIGHT,
            KeyCode::AltLeft => Self::ALT_LEFT,
            KeyCode::AltRight => Self::ALT_RIGHT,
            KeyCode::ShiftLeft => Self::SHIFT_LEFT,
            KeyCode::ShiftRight => Self::SHIFT_RIGHT,
            _ => 0,
        }
    }
}
