//! The shared modifier required by every command macro.

use semantic_input::ModifierChord;

use super::{SettingId, SettingsOptions, definitions::SettingChoice};

macro_rules! modifiers {
    ($($variant:ident => $name:literal, $label:literal),+ $(,)?) => {
        /// Modifier selected for command macros; None allows a plain control press.
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum MacroModifier { $($variant,)+ }

        impl MacroModifier {
            pub const ALL: &[Self] = &[$(Self::$variant,)+];
            pub const CHOICES: &[SettingChoice] = &[$(SettingChoice { name: $name, label: $label },)+];
        }
    };
}

modifiers! {
    Alt => "macro_modifier_radio_alt", "Alt",
    Control => "macro_modifier_radio_control", "Ctrl",
    Shift => "macro_modifier_radio_shift", "Shift",
    Super => "macro_modifier_radio_super", "Super",
    None => "macro_modifier_radio_none", "None",
}

impl MacroModifier {
    /// Tests the selected modifier, allowing other modifiers to remain held.
    pub fn is_held(self, held: ModifierChord) -> bool {
        match self {
            Self::Alt => held.alt,
            Self::Control => held.control,
            Self::Shift => held.shift,
            Self::Super => held.super_key,
            Self::None => true,
        }
    }
}

impl SettingsOptions {
    /// Resolves the validated dropdown choice used by keyboard and mouse macros.
    pub fn macro_modifier(&self) -> MacroModifier {
        MacroModifier::ALL[self.value(SettingId::MacroModifier) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_modifier_choices_round_trip_and_require_only_the_selected_modifier() {
        let id = SettingId::MacroModifier;
        let defaults = SettingsOptions::decode(br#"{}"#).unwrap();
        assert_eq!(defaults.macro_modifier(), MacroModifier::Alt);
        for (index, modifier) in MacroModifier::ALL.iter().copied().enumerate() {
            let mut settings = SettingsOptions::default();
            settings.set(id.index(), index as i32);
            let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
            assert_eq!(loaded.macro_modifier(), modifier);
            for bits in 0..16 {
                let held = ModifierChord {
                    alt: bits & 1 != 0,
                    control: bits & 2 != 0,
                    shift: bits & 4 != 0,
                    super_key: bits & 8 != 0,
                };
                let required = match modifier {
                    MacroModifier::Alt => 1,
                    MacroModifier::Control => 2,
                    MacroModifier::Shift => 4,
                    MacroModifier::Super => 8,
                    MacroModifier::None => 0,
                };
                assert_eq!(
                    loaded.macro_modifier().is_held(held),
                    bits & required == required
                );
            }
        }
    }
}
