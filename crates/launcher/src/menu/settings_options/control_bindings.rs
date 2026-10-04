//! Device-specific bindings share the physical control codes used by gameplay.

use semantic_input::{Action, AxisDirection, PhysicalControl};

pub const GAMEPAD_OFFSET: usize = 0x8000;
pub const EXTRA_KEYS: &[(&str, PhysicalControl)] = &[
    ("key.inventory", PhysicalControl::KeyboardUsage(0x08)),
    ("key.chat", PhysicalControl::KeyboardUsage(0x17)),
    ("key.command", PhysicalControl::KeyboardUsage(0x38)),
    ("key.drop", PhysicalControl::KeyboardUsage(0x14)),
    ("key.pickItem", PhysicalControl::MouseButton(3)),
    ("key.screenshot", PhysicalControl::KeyboardUsage(0x3b)),
    ("key.fullscreen", PhysicalControl::KeyboardUsage(0x44)),
];
pub const SECONDARY_KEYS: &[(&str, PhysicalControl)] =
    &[("key.chat", PhysicalControl::KeyboardUsage(0x28))];
pub const EXTRA_GAMEPAD: &[(&str, Option<PhysicalControl>)] = &[
    ("key.inventory", Some(PhysicalControl::GamepadButton(2))),
    ("key.chat", Some(PhysicalControl::GamepadButton(14))),
    ("key.drop", Some(PhysicalControl::GamepadButton(12))),
    ("key.pickItem", None),
];
pub const GAMEPAD_BINDINGS: &[(Action, &str)] = &[
    (Action::Attack, "key.attack"),
    (Action::Use, "key.use"),
    (Action::Jump, "key.jump"),
    (Action::Sneak, "key.sneak"),
    (Action::Sprint, "key.sprint"),
    (Action::CyclePerspective, "key.togglePerspective"),
    (Action::HotbarPrevious, "key.cycleItemLeft"),
    (Action::HotbarNext, "key.cycleItemRight"),
];

/// Keeps keyboard and gamepad bindings in disjoint persisted code ranges.
pub fn encode_control(control: PhysicalControl) -> Option<u16> {
    match control {
        PhysicalControl::KeyboardUsage(code) if (0x04..=0xe7).contains(&code) => Some(code),
        PhysicalControl::MouseButton(button) if (1..=8).contains(&button) => {
            Some(0x100 + u16::from(button))
        }
        PhysicalControl::GamepadButton(button) if button <= 31 => Some(0x200 + u16::from(button)),
        PhysicalControl::GamepadAxis {
            axis: axis @ 4..=5,
            direction: AxisDirection::Positive,
        } => Some(0x300 + u16::from(axis)),
        _ => None,
    }
}

/// Rejects persisted controls the desktop capture path cannot produce.
pub fn decode_control(code: u16) -> Option<PhysicalControl> {
    match code {
        0x04..=0xe7 => Some(PhysicalControl::KeyboardUsage(code)),
        0x101..=0x108 => Some(PhysicalControl::MouseButton((code - 0x100) as u8)),
        0x200..=0x21f => Some(PhysicalControl::GamepadButton((code - 0x200) as u8)),
        0x304..=0x305 => Some(PhysicalControl::GamepadAxis {
            axis: (code - 0x300) as u8,
            direction: AxisDirection::Positive,
        }),
        _ => None,
    }
}

/// Identifies gamepad controls without treating the movement sticks as remappable buttons.
pub fn is_gamepad(control: PhysicalControl) -> bool {
    matches!(
        control,
        PhysicalControl::GamepadButton(_) | PhysicalControl::GamepadAxis { .. }
    )
}

/// Displays the pack's Xbox button glyph for a captured gamepad control.
pub fn gamepad_icon(control: PhysicalControl) -> &'static str {
    match control {
        PhysicalControl::GamepadButton(0) => "textures/ui/xbox_face_button_down",
        PhysicalControl::GamepadButton(1) => "textures/ui/xbox_face_button_right",
        PhysicalControl::GamepadButton(2) => "textures/ui/xbox_face_button_up",
        PhysicalControl::GamepadButton(3) => "textures/ui/xbox_face_button_left",
        PhysicalControl::GamepadButton(4) => "textures/ui/xbox_bumper_left",
        PhysicalControl::GamepadButton(5) => "textures/ui/xbox_bumper_right",
        PhysicalControl::GamepadButton(8) => "textures/ui/xbox_stick_left",
        PhysicalControl::GamepadButton(9) => "textures/ui/xbox_stick_right",
        PhysicalControl::GamepadButton(11) => "textures/ui/xbox_dpad_up",
        PhysicalControl::GamepadButton(12) => "textures/ui/xbox_dpad_down",
        PhysicalControl::GamepadButton(13) => "textures/ui/xbox_dpad_left",
        PhysicalControl::GamepadButton(14) => "textures/ui/xbox_dpad_right",
        PhysicalControl::GamepadAxis { axis: 4, .. } => "textures/ui/xbox_left_trigger",
        PhysicalControl::GamepadAxis { axis: 5, .. } => "textures/ui/xbox_right_trigger",
        _ => "",
    }
}

impl super::SettingsOptions {
    /// Applies the vanilla A/B and X/Y physical-button swaps after resolving a binding.
    pub fn swap_gamepad_control(&self, control: PhysicalControl) -> PhysicalControl {
        let PhysicalControl::GamepadButton(button) = control else {
            return control;
        };
        let button = match button {
            0 | 1 if self.value("swap_gamepad_ab_buttons") != 0 => 1 - button,
            2 | 3 if self.value("swap_gamepad_xy_buttons") != 0 => 5 - button,
            _ => button,
        };
        PhysicalControl::GamepadButton(button)
    }
}
