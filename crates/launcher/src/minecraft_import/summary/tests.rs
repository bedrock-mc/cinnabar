use super::super::options::{self, Encoding};
use super::*;
use crate::menu::settings_options::SETTINGS_OPTIONS;

#[test]
fn every_supported_setting_has_a_category_and_is_counted() {
    let mut rows = Vec::new();
    for mapping in MAPPINGS {
        assert_ne!(
            category(mapping.destination),
            Category::Other,
            "{}",
            mapping.destination
        );
        let definition = SETTINGS_OPTIONS
            .iter()
            .find(|option| option.name == mapping.destination)
            .unwrap();
        let raw = match mapping.encoding {
            Encoding::Fraction => "0.5".to_owned(),
            Encoding::Blocks => (definition.default * 16).to_string(),
            _ => definition.default.to_string(),
        };
        rows.push(format!("{}:{raw}", mapping.source));
    }
    for key in KEY_BINDINGS
        .iter()
        .map(|(_, key)| key)
        .chain(EXTRA_KEYS.iter().map(|(key, _)| key))
    {
        assert_eq!(category(key), Category::Controls);
        rows.push(format!("keyboard_type_0_{key}:0"));
    }
    rows.extend([
        "game_language:en_US".to_owned(),
        "gfx_fullscreen:1".to_owned(),
    ]);
    let parsed = options::parse(rows.join("\n").as_bytes());
    assert_eq!(parsed.skipped, 0);
    assert_eq!(parsed.count(), rows.len());
    assert_eq!(category("language"), Category::General);
    assert_eq!(category("full_screen"), Category::Video);
    let summary = parsed.summary();
    for mapping in MAPPINGS {
        assert!(!summary.contains(mapping.destination));
    }
    assert!(!summary.contains("key."));
    assert!(summary.contains("Audio:"));
    assert!(summary.contains("Video:"));
    assert!(summary.contains("Controls:"));
    assert!(summary.contains("General:"));
}

#[test]
fn preview_groups_known_keys_and_hides_unknown_keys_under_other() {
    let mut parsed = options::parse(b"audio_main:0.5\naudio_music:0.2\ngfx_field_of_view:80\nctrl_invertmouse:1\nkeyboard_type_0_key.jump:32\ngame_language:en_US\ngfx_fullscreen:1");
    parsed.values.insert("future_setting".into(), 1);
    assert_eq!(category("future_setting"), Category::Other);
    assert_eq!(parsed.count(), 8);
    assert_eq!(
        parsed.summary(),
        "Audio: 2 volume settings\nVideo: 2 settings\nControls: 1 setting, 1 key binding\nGeneral: 1 setting\nOther: 1 setting"
    );
    assert_eq!(ParsedOptions::default().summary(), "None");
}
