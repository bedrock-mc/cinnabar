//! Bedrock command macros run a stored command when Alt and the bound key meet.
//!
//! Macros live below the other keyboard bindings in Keyboard & Mouse settings.
//! Each of the ten slots pairs one keyboard or mouse control with a command
//! string; pressing Alt with the bound control while playing sends the command.

use semantic_input::PhysicalControl;

use super::SettingsOptions;
use super::control_bindings::{decode_control, encode_control, is_gamepad};

/// Bedrock offers ten command macro slots.
pub const COMMAND_MACRO_COUNT: usize = 10;
/// Row-index base the settings UI uses to route macro rows out of the keybinding lists.
const MACRO_OFFSET: usize = 0xc000;
/// Persisted name prefix shared by macro rows and gameplay collision checks.
pub(super) const MACRO_KEY_PREFIX: &str = "key.macro";
/// Stored commands share the outbound chat packet's byte budget.
pub const MAX_COMMAND_MACRO_BYTES: usize = ui::MAX_CHAT_INPUT_BYTES;

/// Persisted key name for one macro slot's binding.
fn macro_key_name(index: usize) -> String {
    format!("{MACRO_KEY_PREFIX}{}", index + 1)
}

/// The macro slot a settings row routes to, when it points at one.
pub fn macro_row(index: usize) -> Option<usize> {
    (MACRO_OFFSET..MACRO_OFFSET + COMMAND_MACRO_COUNT)
        .contains(&index)
        .then(|| index - MACRO_OFFSET)
}

/// The settings row index that captures one macro slot's binding.
pub fn macro_row_index(slot: usize) -> usize {
    MACRO_OFFSET + slot
}

/// Title shown beside one macro slot's binding button.
pub fn macro_title(index: usize) -> String {
    format!("Command Macro {}", index + 1)
}

impl SettingsOptions {
    /// Reads one macro slot's stored command, or empty when untouched.
    pub fn macro_command(&self, index: usize) -> &str {
        self.command_macros
            .as_ref()
            .and_then(|macros| macros.get(index)?.as_deref())
            .unwrap_or_default()
    }

    /// Stores sendable command text, or clears a slot when its command is empty.
    /// Invalid text leaves the previous stored command unchanged.
    pub fn set_macro_command(&mut self, index: usize, command: &str) -> bool {
        if index >= COMMAND_MACRO_COUNT {
            return false;
        }
        let sanitized: String = command
            .chars()
            .filter(|character| !character.is_control())
            .collect();
        if !sanitized.is_empty() && macro_message(&sanitized).is_err() {
            return false;
        }
        if self.macro_command(index) == sanitized {
            return false;
        }
        let mut macros = self.command_macros.clone().unwrap_or_else(empty_macros);
        macros[index] = if sanitized.is_empty() {
            None
        } else {
            Some(sanitized)
        };
        // An all-empty edit returns to the untouched preference.
        self.command_macros = if macros.iter().all(Option::is_none) {
            None
        } else {
            Some(macros)
        };
        true
    }

    /// Reads one macro slot's bound control, if the player assigned one.
    pub fn macro_control(&self, index: usize) -> Option<PhysicalControl> {
        if index >= COMMAND_MACRO_COUNT {
            return None;
        }
        self.keys
            .get(&macro_key_name(index))
            .and_then(|code| decode_control(*code))
            .filter(|control| !is_gamepad(*control))
    }

    /// Saves one macro slot's binding; Alt supplies the modifier at runtime.
    ///
    /// Macros hold their own persisted rows rather than the gameplay action table,
    /// so the shared remap path never has to know about them.
    pub fn remap_macro(&mut self, index: usize, control: PhysicalControl) -> bool {
        if index >= COMMAND_MACRO_COUNT || is_gamepad(control) {
            return false;
        }
        let Some(code) = encode_control(control) else {
            return false;
        };
        self.keys.insert(macro_key_name(index), code);
        true
    }

    /// Clears one macro slot's binding while keeping its stored command.
    pub fn reset_macro(&mut self, index: usize) -> bool {
        if index >= COMMAND_MACRO_COUNT {
            return false;
        }
        self.keys.remove(&macro_key_name(index)).is_some()
    }

    /// Finds the first armed macro bound to `control`, if its command is set.
    pub fn match_macro(&self, control: PhysicalControl) -> Option<usize> {
        (0..COMMAND_MACRO_COUNT).find(|index| {
            self.macro_control(*index) == Some(control) && !self.macro_command(*index).is_empty()
        })
    }

    /// The command text a matched macro sends, with a leading slash ensured.
    pub fn macro_send_text(&self, index: usize) -> Option<String> {
        let command = self.macro_command(index);
        if index >= COMMAND_MACRO_COUNT || command.is_empty() {
            return None;
        }
        macro_message(command).ok()
    }
}

/// Creates an untouched command store.
fn empty_macros() -> [Option<String>; COMMAND_MACRO_COUNT] {
    core::array::from_fn(|_| None)
}

/// Adds the command slash and applies the outbound chat text limits.
pub fn macro_message(command: &str) -> Result<String, ui::ChatSendError> {
    ui::validate_chat_message(command)?;
    let message = if command.starts_with('/') {
        command.to_owned()
    } else {
        format!("/{command}")
    };
    ui::validate_chat_message(&message)?;
    Ok(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_commands_default_to_empty_and_round_trip() {
        let mut settings = SettingsOptions::default();
        for index in 0..COMMAND_MACRO_COUNT {
            assert_eq!(settings.macro_command(index), "");
            assert_eq!(settings.macro_control(index), None);
        }
        assert!(settings.set_macro_command(0, "time set day"));
        assert!(settings.set_macro_command(9, "/give @s apple"));
        assert!(!settings.set_macro_command(0, "time set day"));
        assert!(!settings.set_macro_command(COMMAND_MACRO_COUNT, "time set day"));
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(loaded.macro_command(0), "time set day");
        assert_eq!(loaded.macro_command(9), "/give @s apple");
        assert_eq!(loaded.macro_command(1), "");
    }

    #[test]
    fn command_macro_rejects_messages_over_the_final_slash_budget() {
        let mut settings = SettingsOptions::default();
        let budget = ui::MAX_CHAT_INPUT_BYTES;
        for command in [
            "x".repeat(budget),
            format!("/{}", "x".repeat(budget)),
            "é".repeat(budget / 2),
        ] {
            assert!(!settings.set_macro_command(0, &command));
            assert_eq!(settings.macro_command(0), "");
        }
        for command in [
            "x".repeat(budget - 1),
            format!("/{}", "x".repeat(budget - 1)),
        ] {
            assert!(settings.set_macro_command(0, &command));
            let message = settings.macro_send_text(0).unwrap();
            let mut queue =
                ui::ChatSendQueue::new(1, ui::ChatRateLimit::new(1, 1000).unwrap()).unwrap();
            assert!(queue.push(1, &message, 0).is_ok());
        }
    }

    #[test]
    fn command_macro_keeps_unmodified_late_defaults() {
        for action in [
            semantic_input::Action::Freelook,
            semantic_input::Action::InteractWithToast,
        ] {
            let defaults = SettingsOptions::default().controls().unwrap();
            let binding = defaults
                .bindings()
                .iter()
                .find(|binding| binding.action == action)
                .unwrap();
            let mut settings = SettingsOptions::default();
            assert!(settings.remap_macro(0, binding.chord.control));
            for command in ["", "time set day"] {
                settings.set_macro_command(0, command);
                assert!(settings.controls().unwrap().bindings().contains(binding));
            }
        }
    }

    #[test]
    fn macro_commands_filter_controls_without_truncating_oversized_commands() {
        let mut settings = SettingsOptions::default();
        assert!(settings.set_macro_command(0, "say hello\n\t"));
        assert_eq!(settings.macro_command(0), "say hello");
        assert!(!settings.set_macro_command(0, &"x".repeat(ui::MAX_CHAT_INPUT_BYTES)));
        assert_eq!(settings.macro_command(0), "say hello");
    }

    #[test]
    fn invalid_saved_command_macros_are_dropped_without_losing_valid_slots() {
        let mut saved = SettingsOptions::default();
        let mut commands = empty_macros();
        commands[0] = Some("x".repeat(ui::MAX_CHAT_INPUT_BYTES));
        commands[1] = Some("say hello".to_owned());
        saved.command_macros = Some(commands);
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
        assert_eq!(loaded.macro_command(0), "");
        assert_eq!(loaded.macro_send_text(1).as_deref(), Some("/say hello"));
    }

    #[test]
    fn macro_bindings_persist_and_match_only_armed_slots() {
        let mut settings = SettingsOptions::default();
        let control = PhysicalControl::KeyboardUsage(0x0a);
        assert!(settings.set_macro_command(2, "time set day"));
        assert!(settings.remap_macro(2, control));
        assert_eq!(settings.macro_control(2), Some(control));
        assert_eq!(settings.match_macro(control), Some(2));
        assert_eq!(
            settings.macro_send_text(2).as_deref(),
            Some("/time set day")
        );
        // A bound slot without a command never fires.
        assert!(settings.remap_macro(3, PhysicalControl::KeyboardUsage(0x0b)));
        assert_eq!(
            settings.match_macro(PhysicalControl::KeyboardUsage(0x0b)),
            None
        );
        let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(loaded.macro_control(2), Some(control));
        assert_eq!(loaded.match_macro(control), Some(2));
        assert!(loaded.reset_macro(2));
        assert_eq!(loaded.macro_control(2), None);
        assert_eq!(loaded.macro_command(2), "time set day");
    }

    #[test]
    fn macro_bindings_reject_gamepad_controls() {
        let mut settings = SettingsOptions::default();
        assert!(!settings.remap_macro(0, PhysicalControl::GamepadButton(0)));
        assert!(!settings.reset_macro(COMMAND_MACRO_COUNT));
    }

    #[test]
    fn macro_row_indices_are_distinct_from_gameplay_and_gamepad_rows() {
        for slot in 0..COMMAND_MACRO_COUNT {
            let row = macro_row_index(slot);
            assert_eq!(macro_row(row), Some(slot));
            assert!(row >= MACRO_OFFSET);
        }
        assert_eq!(macro_row(MACRO_OFFSET - 1), None);
        assert_eq!(macro_row(MACRO_OFFSET + COMMAND_MACRO_COUNT), None);
    }

    #[test]
    fn macros_survive_the_settings_file_round_trip() {
        let mut settings = SettingsOptions::default();
        assert!(settings.set_macro_command(0, "time set night"));
        assert!(settings.remap_macro(0, PhysicalControl::KeyboardUsage(0x1e)));
        let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(loaded.macro_command(0), "time set night");
        assert_eq!(
            loaded.macro_control(0),
            Some(PhysicalControl::KeyboardUsage(0x1e))
        );
        assert!(loaded.stored_bindings_valid());
        // An all-empty macro store serializes as untouched.
        assert!(loaded.set_macro_command(0, ""));
        assert!(loaded.command_macros.is_none());
    }

    /// Macro rows follow the keyboard layout, so each device's reset drops only its own.
    #[test]
    fn macros_belong_to_the_keyboard_layout_so_each_reset_only_drops_its_own_rows() {
        let mut settings = SettingsOptions::default();
        assert!(settings.set_macro_command(0, "time set night"));
        assert!(settings.remap_macro(0, PhysicalControl::KeyboardUsage(0x1e)));
        let mut settings =
            SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(
            settings.macro_control(0),
            Some(PhysicalControl::KeyboardUsage(0x1e))
        );
        // The gamepad reset restores controller bindings and leaves the keyboard layout intact.
        settings.reset_bindings(true);
        assert_eq!(
            settings.macro_control(0),
            Some(PhysicalControl::KeyboardUsage(0x1e))
        );
        // The keyboard reset clears macro rows with the other keyboard bindings.
        settings.reset_bindings(false);
        assert_eq!(settings.macro_control(0), None);
        assert_eq!(settings.macro_command(0), "time set night");
        assert!(settings.stored_bindings_valid());
    }

    #[test]
    fn stored_macro_commands_stay_valid_when_reloading_a_full_saved_file() {
        let mut settings = SettingsOptions::default();
        settings.set_macro_command(3, "kill");
        settings.remap_macro(3, PhysicalControl::KeyboardUsage(0x13));
        let file = serde_json::to_vec(&settings).unwrap();
        let loaded = SettingsOptions::decode(&file).unwrap();
        assert_eq!(loaded.macro_command(3), "kill");
        assert_eq!(loaded.macro_send_text(3).as_deref(), Some("/kill"));
        assert!(loaded.stored_bindings_valid());
    }
}
