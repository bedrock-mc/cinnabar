//! Desktop key remapping uses the same physical controls as the gameplay router.

use super::{
    SettingsOptions,
    control_bindings::{
        EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET, SECONDARY_KEYS,
        decode_control, encode_control, is_gamepad,
    },
};
use semantic_input::{Action, ControlSettings, InputContext, PhysicalControl};

pub const KEY_BINDINGS: &[(Action, &str)] = &[
    (Action::Attack, "key.attack"),
    (Action::Use, "key.use"),
    (Action::MoveForward, "key.forward"),
    (Action::MoveBackward, "key.back"),
    (Action::MoveLeft, "key.left"),
    (Action::MoveRight, "key.right"),
    (Action::Jump, "key.jump"),
    (Action::Sneak, "key.sneak"),
    (Action::Sprint, "key.sprint"),
    (Action::CyclePerspective, "key.togglePerspective"),
    (Action::Hotbar1, "key.hotbar.1"),
    (Action::Hotbar2, "key.hotbar.2"),
    (Action::Hotbar3, "key.hotbar.3"),
    (Action::Hotbar4, "key.hotbar.4"),
    (Action::Hotbar5, "key.hotbar.5"),
    (Action::Hotbar6, "key.hotbar.6"),
    (Action::Hotbar7, "key.hotbar.7"),
    (Action::Hotbar8, "key.hotbar.8"),
    (Action::Hotbar9, "key.hotbar.9"),
    (Action::Freelook, "key.freelook"),
    (Action::InteractWithToast, OPEN_NOTIFICATION_KEY),
];

/// Vanilla's "Open Notification" binding name.
pub const OPEN_NOTIFICATION_KEY: &str = "key.interactwithtoast";

/// Actions added after layouts were saved; their default yields to a stored key already using it.
const LATE_DEFAULTS: [Action; 2] = [Action::Freelook, Action::InteractWithToast];

impl SettingsOptions {
    fn default_yields(&self, action: Action) -> bool {
        let Some((_, name)) = KEY_BINDINGS
            .iter()
            .find(|(candidate, _)| *candidate == action)
        else {
            return false;
        };
        if !LATE_DEFAULTS.contains(&action) || self.keys.contains_key(*name) {
            return false;
        }
        let defaults = ControlSettings::default();
        let default = defaults
            .bindings()
            .iter()
            .find(|binding| binding.action == action)
            .map(|binding| binding.chord.control);
        self.keys.iter().any(|(name, code)| {
            !name.starts_with("gamepad:")
                && decode_control(*code).is_some_and(|control| Some(control) == default)
        })
    }
    /// Validates stored controls with the same device and collision rules as interactive remapping.
    pub fn stored_bindings_valid(&self) -> bool {
        self.controls().is_ok()
            && (0..KEY_BINDINGS.len() + EXTRA_KEYS.len())
                .chain(
                    GAMEPAD_OFFSET..GAMEPAD_OFFSET + GAMEPAD_BINDINGS.len() + EXTRA_GAMEPAD.len(),
                )
                .all(|index| {
                    let Some((name, _, _)) = self.binding(index) else {
                        return true;
                    };
                    let Some(code) = self.keys.get(&name) else {
                        return true;
                    };
                    decode_control(*code).is_some_and(|control| {
                        is_gamepad(control) == (index >= GAMEPAD_OFFSET)
                            && !self.binding_conflicts(index, self.swap_gamepad_control(control))
                    })
                })
    }

    /// Resolves a settings row to its persisted name and optional semantic action.
    fn binding(&self, index: usize) -> Option<(String, Option<Action>, Option<PhysicalControl>)> {
        if let Some(index) = index.checked_sub(GAMEPAD_OFFSET) {
            if let Some((action, name)) = GAMEPAD_BINDINGS.get(index) {
                return Some((format!("gamepad:{name}"), Some(*action), None));
            }
            let (name, control) = EXTRA_GAMEPAD.get(index.checked_sub(GAMEPAD_BINDINGS.len())?)?;
            return Some((format!("gamepad:{name}"), None, *control));
        }
        if let Some((action, name)) = KEY_BINDINGS.get(index) {
            return Some(((*name).to_owned(), Some(*action), None));
        }
        let (name, control) = EXTRA_KEYS.get(index.checked_sub(KEY_BINDINGS.len())?)?;
        Some(((*name).to_owned(), None, Some(*control)))
    }

    /// Reads the persisted device binding, falling back to the gameplay router's defaults.
    pub fn key_control(&self, index: usize) -> Option<PhysicalControl> {
        let (name, action, fallback) = self.binding(index)?;
        if action.is_some_and(|action| self.default_yields(action)) {
            return None;
        }
        self.keys
            .get(&name)
            .and_then(|code| decode_control(*code))
            .filter(|control| is_gamepad(*control) == (index >= GAMEPAD_OFFSET))
            .or(fallback)
            .or_else(|| {
                ControlSettings::default()
                    .bindings()
                    .iter()
                    .find(|binding| {
                        binding.context == InputContext::Gameplay
                            && Some(binding.action) == action
                            && is_gamepad(binding.chord.control) == (index >= GAMEPAD_OFFSET)
                            && !matches!(binding.chord.control, PhysicalControl::MouseAxis(_))
                    })
                    .map(|binding| binding.chord.control)
            })
            .map(|control| self.swap_gamepad_control(control))
    }

    /// Reads an existing UI action through the same remapping table as the settings grid.
    pub fn named_key_control(&self, name: &str) -> Option<PhysicalControl> {
        let index = KEY_BINDINGS
            .iter()
            .position(|(_, label)| *label == name)
            .or_else(|| {
                EXTRA_KEYS
                    .iter()
                    .position(|(label, _)| *label == name)
                    .map(|index| KEY_BINDINGS.len() + index)
            })?;
        self.key_control(index)
    }

    /// Keeps vanilla secondary defaults until a remap replaces the complete key list.
    pub fn secondary_key_control(&self, name: &str) -> Option<PhysicalControl> {
        // Vanilla keyboard remapping replaces the list with one captured key.
        SECONDARY_KEYS
            .iter()
            .find(|(label, _)| *label == name && !self.keys.contains_key(name))
            .map(|(_, control)| *control)
    }

    /// Restores one device's full layout without disturbing bindings on the other device.
    pub fn reset_bindings(&mut self, gamepad: bool) {
        self.keys
            .retain(|name, _| name.starts_with("gamepad:") != gamepad);
    }

    /// Saves a mapping only if the complete gameplay binding set remains valid.
    pub fn remap(&mut self, index: usize, control: PhysicalControl) -> bool {
        let Some((name, _, _)) = self.binding(index) else {
            return false;
        };
        if is_gamepad(control) != (index >= GAMEPAD_OFFSET) {
            return false;
        }
        let Some(code) = encode_control(self.swap_gamepad_control(control)) else {
            return false;
        };
        if self.binding_conflicts(index, control) {
            return false;
        }
        let previous = self.keys.insert(name.to_owned(), code);
        if self.controls().is_err() {
            match previous {
                Some(code) => {
                    self.keys.insert(name.to_owned(), code);
                }
                None => {
                    self.keys.remove(&name);
                }
            }
            return false;
        }
        true
    }

    /// Rejects collisions across semantic actions and host-owned UI actions on one device.
    fn binding_conflicts(&self, index: usize, control: PhysicalControl) -> bool {
        let indices = if index >= GAMEPAD_OFFSET {
            GAMEPAD_OFFSET..GAMEPAD_OFFSET + GAMEPAD_BINDINGS.len() + EXTRA_GAMEPAD.len()
        } else {
            0..KEY_BINDINGS.len() + EXTRA_KEYS.len()
        };
        indices.into_iter().any(|other| {
            other != index
                && (self.key_control(other) == Some(control)
                    || self.binding(other).is_some_and(|(name, _, _)| {
                        self.secondary_key_control(&name) == Some(control)
                    }))
        })
    }

    /// Restores one action's default control while preserving other remaps.
    pub fn reset_key(&mut self, index: usize) -> bool {
        let Some((name, _, _)) = self.binding(index) else {
            return false;
        };
        let previous = self.keys.remove(&name);
        if (self.controls().is_err()
            || self
                .key_control(index)
                .is_some_and(|control| self.binding_conflicts(index, control))
            || self
                .secondary_key_control(&name)
                .is_some_and(|control| self.binding_conflicts(index, control)))
            && let Some(previous) = previous
        {
            self.keys.insert(name.to_owned(), previous);
            return false;
        }
        true
    }

    /// Rebuilds and validates the gameplay bindings before handing them to the router.
    pub fn controls(&self) -> Result<ControlSettings, semantic_input::BindingError> {
        let original = ControlSettings::default();
        let mut bindings = original.bindings().to_vec();
        for action in LATE_DEFAULTS {
            if self.default_yields(action) {
                bindings.retain(|binding| binding.action != action);
            }
        }
        for index in (0..KEY_BINDINGS.len())
            .chain((0..GAMEPAD_BINDINGS.len()).map(|index| GAMEPAD_OFFSET + index))
        {
            let Some((name, Some(action), _)) = self.binding(index) else {
                continue;
            };
            let Some(control) = self.keys.get(&name).and_then(|code| decode_control(*code)) else {
                continue;
            };
            let mut replaced = false;
            bindings.retain_mut(|binding| {
                if binding.action != action
                    || binding.context != InputContext::Gameplay
                    || is_gamepad(binding.chord.control) != (index >= GAMEPAD_OFFSET)
                    || matches!(binding.chord.control, PhysicalControl::MouseAxis(_))
                {
                    return true;
                }
                if replaced {
                    return false;
                }
                binding.chord.control = control;
                replaced = true;
                true
            });
        }
        for binding in &mut bindings {
            binding.chord.control = self.swap_gamepad_control(binding.chord.control);
        }
        ControlSettings::new(
            bindings,
            original.mouse_sensitivity,
            original.gamepad_look_sensitivity,
            original.touch_look_sensitivity,
            original.invert_mouse_y,
            original.invert_gamepad_y,
            original.gamepad_move_deadzone,
            original.gamepad_look_deadzone,
        )
    }
}

/// Displays USB controls in the keyboard layout's common desktop notation.
pub fn key_name(control: PhysicalControl) -> String {
    match control {
        PhysicalControl::KeyboardUsage(code @ 0x04..=0x1d) => {
            char::from(b'A' + (code - 4) as u8).to_string()
        }
        PhysicalControl::KeyboardUsage(code @ 0x1e..=0x26) => (code - 0x1d).to_string(),
        PhysicalControl::KeyboardUsage(0x27) => "0".to_owned(),
        PhysicalControl::KeyboardUsage(0x2c) => "Space".to_owned(),
        PhysicalControl::KeyboardUsage(0xe0) => "Left Control".to_owned(),
        PhysicalControl::KeyboardUsage(0xe1) => "Left Shift".to_owned(),
        PhysicalControl::KeyboardUsage(code @ 0x3a..=0x45) => format!("F{}", code - 0x39),
        PhysicalControl::MouseButton(button) => format!("Mouse {button}"),
        PhysicalControl::KeyboardUsage(code) => format!("Key {code:02X}"),
        _ => String::new(),
    }
}
