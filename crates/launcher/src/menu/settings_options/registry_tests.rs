use super::{
    BindingId, SETTINGS_OPTIONS, SettingId, SettingKind, SettingsGroup, SettingsOptions,
    bindings::{BINDINGS, DeviceRow},
    layout::SettingPanel,
    volume_settings,
};
use crate::menu::settings_sections::{SECTION_DEFINITIONS, SETTINGS_TABS, SettingSection};
use std::collections::BTreeSet;

/// Third-person self visuals are opt-in, so third person stays vanilla until the player enables them.
#[test]
fn third_person_self_visuals_default_off() {
    let mut settings = SettingsOptions::default();
    assert!(!settings.f5_self_visuals());
    settings.set(SettingId::F5SelfVisuals.index(), 1);
    assert!(settings.f5_self_visuals());
}

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
        SettingsGroup::Discord,
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
    assert_eq!(
        SettingSection::Discord.definition().reset_group,
        Some(SettingsGroup::Discord)
    );
}

#[test]
fn shaders_tab_is_visible_only_in_shader_pack_builds() {
    let shaders = SettingSection::Shaders.definition();
    assert_eq!(shaders.tab_title.is_some(), super::SHADER_PACK_BINDINGS);
    assert_eq!(
        SETTINGS_TABS.iter().any(|(name, _)| *name == shaders.name),
        super::SHADER_PACK_BINDINGS
    );
    let position = |section: SettingSection| {
        SECTION_DEFINITIONS
            .iter()
            .position(|row| row.id == section)
            .unwrap()
    };
    assert_eq!(
        position(SettingSection::Shaders),
        position(SettingSection::GlobalTexturePack) + 1
    );
    assert_eq!(
        position(SettingSection::Mods),
        position(SettingSection::Shaders) + 1
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

/// Saved keys of settings that were removed; old files holding them must still load.
const RETIRED: &[&str] = &[
    "chat_typeface",
    "content_log_gui_level",
    "gamepad_cursor_sensitivity",
    "actionbar_text_background_opacity",
    "show_auto_save_icon",
    "bubble_particles",
    "keyboard_show_full_keyboard_options",
    "keyboard_mouse_autojump",
    "controller_autojump",
    "hide_tooltips",
    "hide_gamepad_cursor",
    "controller_clear_hotbar",
    "websockets_enabled",
    "websocket_encryption",
    "only_trusted_skins_allowed",
    "pause_option_toggle",
    "ecomode_toggle",
    "script_debugger_passcode_required",
    "script_debugger_auto_attach",
    "editor_collect_network_metrics",
    "serverbound_client_diagnostics_enabled",
    "script_watchdog_spike_warning",
    "script_watchdog_slow_warning",
    "device_info_use_memory_tier_override",
    "debug_text_filtering_use_delay_sec_override",
    "content_log_file",
    "content_log_gui",
    "content_log_gui_show_on_errors",
    "enable_gameplay_subtitles",
    "hide_own_gameplay_subtitles",
    "hide_ambient_gameplay_subtitles",
    "enable_ui_text_to_speech",
    "enable_chat_text_to_speech",
    "enable_open_chat_message",
    "hide_endflash",
    "enable_dithering_blocks",
    "enable_dithering_mobs",
    "texttospeech_volume",
];

#[test]
fn saved_schema_snapshot_keeps_active_setting_and_binding_keys() {
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
            .filter(|name| !RETIRED.contains(&name.as_str()))
            .collect::<Vec<_>>()
    );
    assert_eq!(encoded["keys"], stored["keys"]);
    for retired in RETIRED {
        assert!(SettingId::from_name(retired).is_none(), "{retired}");
        assert!(encoded["values"].get(retired).is_none(), "{retired}");
    }
    for name in stored["values"]
        .as_object()
        .unwrap()
        .keys()
        .filter(|name| !RETIRED.contains(&name.as_str()))
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

/// Accessibility neither displays nor resets preferences owned by the Chat panel.
#[test]
fn accessibility_keeps_chat_preferences_in_the_chat_panel() {
    let mut settings = SettingsOptions::default();
    for option in SettingPanel::Chat.options() {
        assert!(
            option
                .section
                .iter()
                .all(|(panel, _)| panel.section() != SettingSection::Accessibility)
        );
        let changed = if option.default == option.min {
            option.max
        } else {
            option.min
        };
        assert!(settings.set(option.id.index(), changed));
    }
    settings.set(SettingId::ToastNotificationDuration.index(), 1);
    let before = settings.clone();
    assert!(settings.reset_group(SettingsGroup::Accessibility));
    for option in SettingPanel::Chat.options() {
        assert_eq!(settings.value(option.id), before.value(option.id));
    }
    assert_eq!(
        settings.value(SettingId::ToastNotificationDuration),
        SettingId::ToastNotificationDuration.definition().default
    );
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

/// The Discord panel and its detailed rows share one reset group.
#[test]
fn discord_panel_reset_restores_all_presence_preferences() {
    let mut settings = SettingsOptions::default();
    let displayed: BTreeSet<_> = SettingPanel::Discord
        .options()
        .chain(SettingPanel::DiscordDetails.options())
        .map(|option| option.name)
        .collect();
    let reset: BTreeSet<_> = SettingsGroup::Discord
        .options()
        .map(|option| option.name)
        .collect();
    assert_eq!(displayed, reset);
    for option in SettingsGroup::Discord.options() {
        let changed = if option.default == option.min {
            option.max
        } else {
            option.min
        };
        assert!(settings.set(option.id.index(), changed));
    }
    assert!(settings.reset_group(SettingsGroup::Discord));
    for option in SettingsGroup::Discord.options() {
        assert_eq!(settings.value(option.id), option.default);
    }
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

/// Iris's reload key defaults to R and remaps like other client controls, in builds that
/// compile shader packs; other builds offer no row for it.
#[test]
fn reload_shaders_defaults_to_r_and_remaps_only_in_shader_pack_builds() {
    let r = semantic_input::PhysicalControl::KeyboardUsage(0x15);
    let mut options = SettingsOptions::default();
    let row = BindingId::ReloadShaders.row(false);
    assert_eq!(row.is_some(), super::SHADER_PACK_BINDINGS);
    let Some(row) = row else {
        assert_eq!(options.named_key_control(BindingId::ReloadShaders), None);
        return;
    };
    assert_eq!(
        super::custom_key_label(BindingId::ReloadShaders.name()),
        Some("Reload Shaders")
    );
    assert_eq!(options.named_key_control(BindingId::ReloadShaders), Some(r));
    let f5 = semantic_input::PhysicalControl::KeyboardUsage(0x3e);
    assert!(options.remap(row, f5));
    let reloaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
    assert_eq!(
        reloaded.named_key_control(BindingId::ReloadShaders),
        Some(f5)
    );
}

/// An older file that still holds removed settings loads its live values unchanged and drops the
/// removed keys on the next save.
#[test]
fn saved_values_survive_alongside_removed_setting_keys() {
    let mut values = serde_json::Map::new();
    for retired in RETIRED {
        values.insert((*retired).into(), 1.into());
    }
    let live = [
        (SettingId::FilterProfanity, 1),
        (SettingId::AlwaysSprint, 1),
        (SettingId::FieldOfView, 90),
        (SettingId::MainVolume, 40),
    ];
    for (id, value) in live {
        assert_ne!(
            id.definition().default,
            value,
            "{id:?} needs a nondefault value"
        );
        values.insert(id.name().into(), value.into());
    }
    let saved = serde_json::json!({ "schema": super::SETTINGS_SCHEMA, "values": values });
    let options = SettingsOptions::decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
    for (id, value) in live {
        assert_eq!(options.value(id), value, "{id:?}");
    }
    let encoded = serde_json::to_value(&options).unwrap();
    for retired in RETIRED {
        assert!(encoded["values"].get(retired).is_none(), "{retired}");
    }
}
