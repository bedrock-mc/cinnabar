use super::{
    BindingId, SETTINGS_OPTIONS, SettingId, SettingKind, SettingsGroup, SettingsOptions,
    bindings::{BINDINGS, DeviceRow},
    layout::SettingPanel,
    volume_settings,
};
use crate::menu::settings_sections::{SECTION_DEFINITIONS, SETTINGS_TABS, SettingSection};
use std::collections::BTreeSet;

/// Badge display defaults on and remains independent of advertising this client's presence.
#[test]
fn cinnabar_badge_display_is_independent_and_defaults_on() {
    let mut settings = SettingsOptions::default();
    assert_eq!(settings.value(SettingId::CinnabarBadges), 1);
    settings.set(SettingId::CinnabarVisible.index(), 0);
    assert_eq!(settings.value(SettingId::CinnabarBadges), 1);
    settings.set(SettingId::CinnabarBadges.index(), 0);
    assert_eq!(settings.value(SettingId::CinnabarVisible), 0);
    let network: Vec<_> = SettingPanel::GeneralNetwork
        .options()
        .map(|option| option.id)
        .collect();
    let visibility = network
        .iter()
        .position(|id| *id == SettingId::CinnabarVisible)
        .unwrap();
    assert_eq!(network[visibility + 1], SettingId::CinnabarBadges);
}

#[test]
fn registered_ids_keep_names_and_indices_unique_at_string_boundaries() {
    let mut names = BTreeSet::new();
    for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
        assert!(names.insert(option.name));
        assert_eq!(option.id.index(), index);
        assert_eq!(SettingId::from_name(option.name), Some(option.id));
        assert_eq!(option.id.name(), option.name);
        assert_eq!(SettingId::from_name(&format!("{}#", option.name)), None);
    }
    let mut names = BTreeSet::new();
    for binding in BINDINGS {
        assert!(names.insert(binding.name));
        assert_eq!(BindingId::from_name(binding.name), Some(binding.id));
        assert_eq!(binding.id.name(), binding.name);
        assert_eq!(BindingId::from_name(&format!("{}#", binding.name)), None);
    }
}

#[test]
fn section_rows_keep_order_and_shared_controls_from_the_registry() {
    for panel in SettingPanel::ALL {
        let rows: Vec<_> = panel.options().collect();
        for (order, row) in rows.iter().enumerate() {
            assert!(row.section.contains(&(*panel, order as u16)));
        }
    }
    let video: Vec<_> = SettingPanel::VideoAccessibility
        .options()
        .map(|row| row.id)
        .collect();
    let accessibility: Vec<_> = SettingPanel::AccessibilityGameplay
        .options()
        .map(|row| row.id)
        .collect();
    assert!(video.contains(&SettingId::CameraShake));
    assert!(accessibility.contains(&SettingId::CameraShake));
    assert_eq!(
        SettingPanel::Audio
            .options()
            .map(|row| row.id)
            .collect::<Vec<_>>(),
        volume_settings().map(|row| row.id).collect::<Vec<_>>()
    );
}

#[test]
fn every_reset_changes_only_its_registered_members_and_keeps_bindings() {
    for group in [
        SettingsGroup::Video,
        SettingsGroup::Accessibility,
        SettingsGroup::Audio,
        SettingsGroup::Chat,
    ] {
        let mut options = SettingsOptions::default();
        for row in SETTINGS_OPTIONS {
            if !matches!(row.kind, SettingKind::Host) {
                let changed = if row.default == row.min {
                    row.max
                } else {
                    row.min
                };
                options.set(row.id.index(), changed);
            }
        }
        let before = options.clone();
        assert!(options.reset_group(group));
        for row in SETTINGS_OPTIONS {
            let expected = if row.reset_group.contains(&group) {
                SettingsOptions::default().value(row.id)
            } else {
                before.value(row.id)
            };
            assert_eq!(options.value(row.id), expected, "{:?}: {:?}", group, row.id);
        }
        for binding in BINDINGS {
            assert_eq!(
                options.named_key_control(binding.id),
                before.named_key_control(binding.id)
            );
        }
    }
}

#[test]
fn shared_section_metadata_drives_titles_tabs_and_reset_routes() {
    let mut indices = BTreeSet::new();
    for section in SECTION_DEFINITIONS {
        assert!(indices.insert(section.index));
        assert_eq!(SettingSection::from_name(section.name), Some(section.id));
        assert_eq!(SettingSection::from_index(section.index), Some(section.id));
        if let Some(title) = section.tab_title {
            assert!(SETTINGS_TABS.contains(&(section.name, title)));
        }
    }
    assert_eq!(
        SettingSection::Video.definition().reset_group,
        Some(SettingsGroup::Video)
    );
    assert_eq!(
        SettingSection::Accessibility.definition().reset_group,
        Some(SettingsGroup::Accessibility)
    );
    assert_eq!(
        SettingSection::Sound.definition().reset_group,
        Some(SettingsGroup::Audio)
    );
    assert_eq!(
        SettingSection::Chat.definition().reset_group,
        Some(SettingsGroup::Chat)
    );
}

#[test]
fn persisted_setting_and_binding_names_survive_reload_with_host_values_excluded() {
    let saved = br#"{"schema":1,"values":{"gamma":65,"camera_shake":0,"full_screen":1,"gui_scale":3},"keys":{"key.drop":69,"key.screenshot":70,"gamepad:key.emote":521}}"#;
    let options = SettingsOptions::decode(saved).unwrap();
    assert_eq!(options.value(SettingId::Gamma), 65);
    assert_eq!(options.value(SettingId::CameraShake), 0);
    assert_eq!(
        options.named_key_control(BindingId::Drop),
        Some(semantic_input::PhysicalControl::KeyboardUsage(69))
    );
    assert_eq!(
        options.named_key_control(BindingId::Screenshot),
        Some(semantic_input::PhysicalControl::KeyboardUsage(70))
    );
    assert_eq!(
        options.key_control(BindingId::Emote.row(true).unwrap()),
        Some(semantic_input::PhysicalControl::GamepadButton(9))
    );
    let encoded = serde_json::to_value(&options).unwrap();
    assert_eq!(encoded["values"][SettingId::Gamma.name()], 65);
    assert!(
        encoded["values"]
            .get(SettingId::FullScreen.name())
            .is_none()
    );
    assert!(encoded["values"].get(SettingId::GuiScale.name()).is_none());
    let reloaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
    assert_eq!(options, reloaded);
}

#[test]
fn binding_projections_keep_every_device_row_contiguous_and_unique() {
    for gamepad in [false, true] {
        let mut actions = BTreeSet::new();
        let mut extras = BTreeSet::new();
        for binding in BINDINGS {
            let row = if gamepad {
                binding.gamepad
            } else {
                binding.keyboard
            };
            match row {
                Some(DeviceRow::Action(index)) => {
                    assert!(actions.insert(index));
                }
                Some(DeviceRow::Extra(index, _)) => {
                    assert!(extras.insert(index));
                }
                None => {
                    assert_eq!(binding.id.row(gamepad), None);
                }
            }
        }
        assert_eq!(actions, (0..actions.len()).collect::<BTreeSet<_>>());
        assert_eq!(extras, (0..extras.len()).collect::<BTreeSet<_>>());
    }
}

#[test]
fn chat_color_codes_and_swatch_choices_share_one_palette() {
    let mut options = SettingsOptions::default();
    for (index, color) in super::chat::CHAT_PALETTE.iter().enumerate() {
        options.set(SettingId::ChatColor.index(), index as i32);
        assert_eq!(options.chat_color_code(), color.code);
        assert_eq!(
            super::chat::CHAT_COLORS[index].label,
            super::chat::MENTIONS_COLORS[index].label
        );
    }
}

#[test]
fn saved_schema_snapshot_keeps_active_setting_and_binding_keys() {
    let retired = "chat_typeface";
    let fixture = include_bytes!("../../../tests/data/settings-v1.json");
    let stored: serde_json::Value = serde_json::from_slice(fixture).unwrap();
    let options = SettingsOptions::decode(fixture).unwrap();
    let encoded = serde_json::to_value(&options).unwrap();
    assert_eq!(
        encoded["values"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        stored["values"]
            .as_object()
            .unwrap()
            .keys()
            .filter(|name| name.as_str() != retired)
            .collect::<Vec<_>>()
    );
    assert_eq!(encoded["keys"], stored["keys"]);
    assert!(encoded["values"].get(retired).is_none());
    for name in stored["values"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| name.as_str() != retired)
    {
        let id = SettingId::from_name(name).expect("saved setting must remain registered");
        assert_eq!(
            options.value(id),
            encoded["values"][name.as_str()].as_i64().unwrap() as i32
        );
    }
    for name in stored["keys"].as_object().unwrap().keys() {
        let (name, gamepad) = name
            .strip_prefix("gamepad:")
            .map_or((name.as_str(), false), |name| (name, true));
        let id = BindingId::from_name(name).expect("saved binding must remain registered");
        assert_eq!(options.key_control(id.row(gamepad).unwrap()), None);
    }
}

/// The dedicated Chat reset includes every displayed preference and preserves other panels.
#[test]
fn chat_panel_reset_restores_shared_values_and_persists_defaults() {
    let mut settings = SettingsOptions::default();
    let chat: BTreeSet<_> = SettingPanel::Chat
        .options()
        .map(|option| option.name)
        .collect();
    let reset: BTreeSet<_> = SettingsGroup::Chat
        .options()
        .map(|option| option.name)
        .collect();
    assert_eq!(chat, reset);
    for option in SettingPanel::Chat.options() {
        let changed = if option.default == option.min {
            option.max
        } else {
            option.min
        };
        assert!(settings.set(option.id.index(), changed));
    }
    let unrelated = SettingId::MainVolume;
    settings.set(unrelated.index(), unrelated.definition().min);
    let saved = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    for option in SettingPanel::Chat.options() {
        assert_ne!(saved.value(option.id), option.default);
    }
    assert!(settings.reset_group(SettingsGroup::Chat));
    let restored = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    for option in SettingPanel::Chat.options() {
        assert_eq!(restored.value(option.id), option.default);
    }
    assert_eq!(restored.value(unrelated), unrelated.definition().min);
}

/// Old typeface values cannot suppress font sizing or reintroduce a removed selector.
#[test]
fn retired_chat_typeface_is_ignored_without_losing_font_size() {
    let retired = "chat_typeface";
    for legacy in [0, 1] {
        let stored =
            serde_json::json!({"schema": 2, "values": {(retired): legacy, "chat_font_size": 15}});
        let options = SettingsOptions::decode(&serde_json::to_vec(&stored).unwrap()).unwrap();
        assert_eq!(options.chat_font_scale(), 1.5);
        assert!(SettingId::from_name(retired).is_none());
        let saved = serde_json::to_value(&options).unwrap();
        assert!(saved["values"].get(retired).is_none());
        assert_eq!(
            SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap())
                .unwrap()
                .chat_font_scale(),
            1.5
        );
    }
}
