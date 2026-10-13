//! A schema-checked table maps supported player preferences to launcher settings.

use std::collections::BTreeMap;

use crate::menu::settings_options::{SETTINGS_OPTIONS, SettingsOptions};

/// The conversion from a saved numeric value to a launcher option.
#[derive(Clone, Copy, Debug)]
pub enum Encoding {
    Toggle,
    Integer,
    Fraction,
    Blocks,
}

/// One supported mapping; tests require its destination to remain in the settings schema.
#[derive(Clone, Copy, Debug)]
pub struct Mapping {
    pub source: &'static str,
    pub destination: &'static str,
    pub encoding: Encoding,
}

/// Supported preferences with equivalent launcher controls and explicit value encodings.
pub const MAPPINGS: &[Mapping] = &[
    Mapping {
        source: "gfx_field_of_view",
        destination: "field_of_view",
        encoding: Encoding::Integer,
    },
    Mapping {
        source: "gfx_viewdistance",
        destination: "render_distance",
        encoding: Encoding::Blocks,
    },
    Mapping {
        source: "gfx_gamma",
        destination: "gamma",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "gfx_vsync",
        destination: "vsync",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_hidehud",
        destination: "hide_hud",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_hidehand",
        destination: "hide_hand",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_hidepaperdoll",
        destination: "hide_paperdoll",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_viewbobbing",
        destination: "view_bobbing",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_fancygraphics",
        destination: "graphics_mode",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_fancyskies",
        destination: "fancy_skies",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_renderclouds",
        destination: "render_clouds",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_transparentleaves",
        destination: "transparent_leaves",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_smoothlighting",
        destination: "smooth_lighting",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_field_of_view_toggle",
        destination: "field_of_view_toggle",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "gfx_thirdperson",
        destination: "third_person",
        encoding: Encoding::Integer,
    },
    Mapping {
        source: "gfx_interface_opacity",
        destination: "interface_opacity",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "ctrl_sensitivity",
        destination: crate::menu::settings_options::MOUSE_SENSITIVITY_OPTION.name,
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "ctrl_invertmouse",
        destination: "keyboard_mouse_invert_y_axis",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "ctrl_autojump",
        destination: "keyboard_mouse_autojump",
        encoding: Encoding::Toggle,
    },
    Mapping {
        source: "audio_main",
        destination: "main_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_music",
        destination: "music_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_sound",
        destination: "sound_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_ambient",
        destination: "ambient_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_block",
        destination: "block_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_hostile",
        destination: "hostile_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_neutral",
        destination: "neutral_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_player",
        destination: "player_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_record",
        destination: "record_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_weather",
        destination: "weather_volume",
        encoding: Encoding::Fraction,
    },
    Mapping {
        source: "audio_texttospeech",
        destination: "texttospeech_volume",
        encoding: Encoding::Fraction,
    },
];

/// Validated changes and counts, excluding all unknown option contents.
#[derive(Clone, Debug, Default)]
pub struct ParsedOptions {
    pub values: BTreeMap<String, i32>,
    pub language: Option<String>,
    pub fullscreen: Option<bool>,
    pub bindings: BTreeMap<String, Option<semantic_input::PhysicalControl>>,
    pub skipped: usize,
}

impl ParsedOptions {
    /// Applies only validated destinations, leaving every other player preference intact.
    pub fn apply(&self, settings: &mut SettingsOptions) -> usize {
        for (name, value) in &self.values {
            if let Some(index) = SETTINGS_OPTIONS
                .iter()
                .position(|option| option.name == name)
            {
                settings.set(index, *value);
            }
        }
        if let Some(language) = &self.language {
            settings.set_language(language);
        }
        settings.import_keyboard_bindings(&self.bindings)
    }

    /// Lists the settings that will be replaced, using schema names for unknown translations.
    pub fn names(&self) -> Vec<String> {
        self.values
            .keys()
            .cloned()
            .chain(self.bindings.keys().cloned())
            .chain(self.language.as_ref().map(|_| "Language".to_owned()))
            .chain(self.fullscreen.map(|_| "Fullscreen".to_owned()))
            .collect()
    }
}

/// Parses UTF-8 lines independently; malformed, unsupported and duplicate rows are counted.
pub fn parse(bytes: &[u8]) -> ParsedOptions {
    let mut parsed = ParsedOptions::default();
    let mut seen = std::collections::HashSet::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        let Ok(line) = std::str::from_utf8(line) else {
            parsed.skipped += 1;
            continue;
        };
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        let Some((key, raw)) = line.split_once(':') else {
            parsed.skipped += 1;
            continue;
        };
        let (key, raw) = (key.trim(), raw.trim());
        if !seen.insert(key) {
            parsed.skipped += 1;
            continue;
        }
        if let Some(name) = key.strip_prefix("keyboard_type_0_") {
            if (crate::menu::settings_options::KEY_BINDINGS
                .iter()
                .any(|(_, key)| *key == name)
                || crate::menu::settings_options::EXTRA_KEYS
                    .iter()
                    .any(|(key, _)| *key == name))
                && let Some(control) =
                    super::keyboard::control(raw.split(',').next().unwrap_or_default().trim())
            {
                parsed.bindings.insert(name.to_owned(), control);
                parsed.skipped += raw
                    .split(',')
                    .skip(1)
                    .filter(|value| !matches!(value.trim(), "" | "0"))
                    .count();
            } else {
                parsed.skipped += 1;
            }
        } else if key == "game_language" && assets::is_language_code(raw) {
            parsed.language = Some(raw.to_owned());
        } else if key == "gfx_fullscreen" && matches!(raw, "0" | "1") {
            parsed.fullscreen = Some(raw == "1");
        } else if let Some(mapping) = MAPPINGS.iter().find(|mapping| mapping.source == key)
            && let Some(value) = convert(mapping, raw)
        {
            parsed.values.insert(mapping.destination.to_owned(), value);
        } else {
            parsed.skipped += 1;
        }
    }
    parsed
}

/// Rejects non-finite and out-of-range values before the settings setter can clamp them.
fn convert(mapping: &Mapping, raw: &str) -> Option<i32> {
    let option = SETTINGS_OPTIONS
        .iter()
        .find(|option| option.name == mapping.destination)?;
    let value = match mapping.encoding {
        Encoding::Toggle => match raw {
            "0" => 0,
            "1" => 1,
            _ => return None,
        },
        Encoding::Integer => {
            let value = raw.parse::<f64>().ok()?;
            if !value.is_finite()
                || value.fract() != 0.0
                || value < option.min as f64
                || value > option.max as f64
            {
                return None;
            }
            value as i32
        }
        Encoding::Fraction => {
            let value = raw.parse::<f64>().ok()?;
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return None;
            }
            (value * option.max as f64).round() as i32
        }
        Encoding::Blocks => {
            let blocks = raw.parse::<u32>().ok()?;
            if blocks % 16 != 0 {
                return None;
            }
            i32::try_from(blocks / 16).ok()?
        }
    };
    (option.min..=option.max).contains(&value).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imported_keyboard_swaps_are_validated_together() {
        let parsed = parse(b"keyboard_type_0_key.forward:83\nkeyboard_type_0_key.back:87\nkeyboard_type_0_key.jump:32\nkeyboard_type_0_key.attack:-99\nkeyboard_type_0_key.chat:84,13\nkeyboard_type_1_key.forward:90");
        let mut settings = SettingsOptions::default();
        assert_eq!(parsed.skipped, 2);
        assert_eq!(parsed.apply(&mut settings), 0);
        assert_eq!(
            settings.named_key_control("key.forward"),
            Some(semantic_input::PhysicalControl::KeyboardUsage(0x16))
        );
        assert_eq!(
            settings.named_key_control("key.back"),
            Some(semantic_input::PhysicalControl::KeyboardUsage(0x1a))
        );
        assert!(settings.stored_bindings_valid());
    }

    #[test]
    fn every_mapping_is_checked_against_the_current_schema() {
        let mut sources = std::collections::HashSet::new();
        let mut destinations = std::collections::HashSet::new();
        for mapping in MAPPINGS {
            assert!(sources.insert(mapping.source));
            assert!(destinations.insert(mapping.destination));
            let definition = SETTINGS_OPTIONS
                .iter()
                .find(|option| option.name == mapping.destination)
                .unwrap_or_else(|| panic!("missing import destination {}", mapping.destination));
            if matches!(mapping.encoding, Encoding::Toggle) {
                assert_eq!((definition.min, definition.max, definition.step), (0, 1, 1));
            }
            let raw = match mapping.encoding {
                Encoding::Fraction => "0.5".to_owned(),
                Encoding::Blocks => (definition.default * 16).to_string(),
                _ => definition.default.to_string(),
            };
            assert!(convert(mapping, &raw).is_some(), "{}", mapping.source);
        }
    }

    #[test]
    fn malformed_rows_are_counted_and_other_preferences_survive() {
        let parsed = parse(b"\xef\xbb\xbfgfx_field_of_view:80\r\naudio_music:0.25\ngfx_viewdistance:160\ngfx_gamma:NaN\ngfx_vsync:2\nunknown:private value\nbad\n\xff\ngfx_field_of_view:90\ngame_language:de_DE\ngfx_fullscreen:1\n");
        assert_eq!(parsed.skipped, 6);
        let mut settings = SettingsOptions::default();
        let previous = settings.value("animations");
        parsed.apply(&mut settings);
        assert_eq!(settings.value("field_of_view"), 80);
        assert_eq!(settings.value("music_volume"), 25);
        assert_eq!(settings.value("render_distance"), 10);
        assert_eq!(settings.language(), Some("de_DE"));
        assert_eq!(settings.value("animations"), previous);
        assert_eq!(parsed.fullscreen, Some(true));
    }
}
