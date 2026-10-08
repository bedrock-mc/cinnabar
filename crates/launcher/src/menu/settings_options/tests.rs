use super::*;
use semantic_input::{InputContext, PhysicalControl};

#[test]
fn chat_position_defaults_to_bottom_persists_and_resets_only_with_video() {
    let mut settings = SettingsOptions::decode(br#"{"values":{}}"#).unwrap();
    assert!(!settings.chat_at_top());
    let index = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == CHAT_POSITION_OPTION.name)
        .unwrap();
    settings.set(index, CHAT_POSITION_OPTION.max);
    let mut saved = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(saved.chat_at_top());
    saved.reset_group(SettingsGroup::Accessibility);
    assert!(saved.chat_at_top());
    saved.reset_group(SettingsGroup::Video);
    assert!(!saved.chat_at_top());
}

#[test]
fn existing_f_binding_survives_freelook_default() {
    let settings =
        SettingsOptions::decode(br#"{"keys":{"key.drop":9,"key.inventory":12}}"#).unwrap();
    assert_eq!(
        settings.named_key_control("key.drop"),
        Some(PhysicalControl::KeyboardUsage(9))
    );
    assert_eq!(
        settings.named_key_control("key.inventory"),
        Some(PhysicalControl::KeyboardUsage(12))
    );
    assert_eq!(settings.named_key_control("key.freelook"), None);
    let controls = settings.controls().unwrap();
    assert!(
        !controls
            .bindings()
            .iter()
            .any(|binding| binding.action == semantic_input::Action::Freelook)
    );
}

#[test]
fn freelook_defaults_to_f_and_remaps_to_mouse_across_reload() {
    let mut settings = SettingsOptions::default();
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == semantic_input::Action::Freelook)
        .unwrap();
    assert_eq!(
        settings.key_control(row),
        Some(PhysicalControl::KeyboardUsage(0x09))
    );
    assert!(settings.remap(row, PhysicalControl::MouseButton(4)));
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    let controls = loaded.controls().unwrap();
    let mut router = semantic_input::SemanticInputRouter::default();
    router.replace_bindings(controls).unwrap();
    router
        .route(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                activity_sequence: 1,
                mouse_buttons: vec![4],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    assert!(router.finalize().unwrap().phases[semantic_input::Action::Freelook as usize].held);
}

#[test]
fn open_notification_defaults_to_n_and_remaps_across_reload() {
    let mut settings = SettingsOptions::default();
    let row = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == OPEN_NOTIFICATION_KEY)
        .unwrap();
    assert_eq!(
        KEY_BINDINGS[row].0,
        semantic_input::Action::InteractWithToast
    );
    assert_eq!(
        settings.named_key_control(OPEN_NOTIFICATION_KEY),
        Some(PhysicalControl::KeyboardUsage(0x11))
    );
    // Another action's key is refused; a free one sticks and reaches the router.
    assert!(!settings.remap(row, PhysicalControl::KeyboardUsage(0x08)));
    assert!(settings.remap(row, PhysicalControl::KeyboardUsage(0x0f)));
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(
        loaded.named_key_control(OPEN_NOTIFICATION_KEY),
        Some(PhysicalControl::KeyboardUsage(0x0f))
    );
    let mut router = semantic_input::SemanticInputRouter::default();
    router.replace_bindings(loaded.controls().unwrap()).unwrap();
    router
        .route(semantic_input::DeviceFrame {
            keyboard_mouse: Some(semantic_input::KeyboardMouseFrame {
                activity_sequence: 1,
                keys: vec![0x0f],
                ..Default::default()
            }),
            ..Default::default()
        })
        .unwrap();
    let phases = router.finalize().unwrap().phases;
    assert!(phases[semantic_input::Action::InteractWithToast as usize].pressed);
}

#[test]
fn a_saved_layout_already_using_n_keeps_it_over_the_open_notification_default() {
    let settings = SettingsOptions::decode(br#"{"keys":{"key.drop":17}}"#).unwrap();
    assert_eq!(
        settings.named_key_control("key.drop"),
        Some(PhysicalControl::KeyboardUsage(0x11))
    );
    assert_eq!(settings.named_key_control(OPEN_NOTIFICATION_KEY), None);
    assert!(
        !settings
            .controls()
            .unwrap()
            .bindings()
            .iter()
            .any(|binding| binding.action == semantic_input::Action::InteractWithToast)
    );
}

/// Finds an option through the same stable controller identifier used on disk.
fn index(name: &str) -> usize {
    SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .unwrap()
}

#[test]
fn all_option_families_round_trip_and_reject_unknown_fields() {
    let mut settings = SettingsOptions::default();
    for (index, definition) in SETTINGS_OPTIONS.iter().enumerate() {
        assert_eq!(settings.get(index), definition.default);
        assert!(definition.min <= definition.default && definition.default <= definition.max);
        settings.set(index, definition.min);
    }
    let bytes = serde_json::to_vec(&settings).unwrap();
    assert_eq!(SettingsOptions::decode(&bytes).unwrap(), settings);
    let loaded =
        SettingsOptions::decode(br#"{"values":{"gamma":500,"field_of_view":-20,"unknown":100}}"#)
            .unwrap();
    assert_eq!(loaded.value("gamma"), SETTINGS_OPTIONS[index("gamma")].max);
    assert_eq!(
        loaded.value("field_of_view"),
        SETTINGS_OPTIONS[index("field_of_view")].min
    );
    assert!(!loaded.values.contains_key("unknown"));
    assert!(SettingsOptions::decode(b"broken").is_none());
}

/// Fresh user settings and the FOV slider must start from the same vanilla default.
#[test]
fn fresh_settings_use_the_field_of_view_slider_default() {
    let slider = &SETTINGS_OPTIONS[index("field_of_view")];
    assert_eq!(
        ui::UserSettings::default().video.horizontal_fov_degrees,
        slider.default as f32
    );
    assert_eq!(
        SettingsOptions::default()
            .user_settings()
            .video
            .horizontal_fov_degrees,
        ui::UserSettings::default().video.horizontal_fov_degrees
    );
}

#[test]
fn remapped_keyboard_controls_reach_gameplay_and_survive_reload() {
    let mut settings = SettingsOptions::default();
    let index = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == "key.forward")
        .unwrap();
    let new_key = PhysicalControl::KeyboardUsage(0x0c);
    assert!(settings.remap(index, new_key));
    let restored = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(restored.key_control(index), Some(new_key));
    let controls = restored.user_settings().controls;
    assert!(controls.bindings().iter().any(|binding| binding.action
        == semantic_input::Action::MoveForward
        && binding.context == InputContext::Gameplay
        && binding.chord.control == new_key));
    assert!(!settings.remap(index, PhysicalControl::KeyboardUsage(0x16)));
    settings.reset_key(index);
    assert_eq!(
        settings.key_control(index),
        SettingsOptions::default().key_control(index)
    );
}

#[test]
fn settings_file_replacement_round_trips() {
    let directory = std::env::temp_dir().join(format!("cinnabar-settings-{}", std::process::id()));
    let path = directory.join(SETTINGS_FILE);
    let mut settings = SettingsOptions::default();
    settings.set(index("gamma"), 80);
    settings.save(&path).unwrap();
    assert_eq!(SettingsOptions::load(&path), settings);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn reset_conflict_preserves_every_saved_mapping() {
    let mut settings = SettingsOptions::default();
    let forward = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == "key.forward")
        .unwrap();
    let backward = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == "key.back")
        .unwrap();
    let default_forward = settings.key_control(forward).unwrap();
    assert!(settings.remap(forward, PhysicalControl::KeyboardUsage(0x0c)));
    assert!(settings.remap(backward, default_forward));
    let before = settings.clone();
    assert!(!settings.reset_key(forward));
    assert_eq!(settings, before);
}

#[test]
fn chat_settings_survive_reload_and_feed_the_chat_renderer() {
    let mut settings = SettingsOptions::default();
    for (name, value) in [
        ("chat_typeface", 1),
        ("chat_font_size", 15),
        ("chat_line_spacing", 25),
        ("chat_color", 3),
        ("chat_message_duration", 2),
    ] {
        settings.set(index(name), value);
    }
    let restored = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(restored.chat_font_scale(), 1.5);
    assert_eq!(restored.chat_line_padding(), 2.501);
    assert_eq!(restored.chat_color_code(), 'c');
    assert_eq!(restored.chat_lifetime(), 30.0);
}

#[test]
fn gamepad_remaps_reach_router_and_reset_independently() {
    use super::{GAMEPAD_BINDINGS, GAMEPAD_OFFSET};
    let mut settings = SettingsOptions::default();
    let jump = GAMEPAD_OFFSET
        + GAMEPAD_BINDINGS
            .iter()
            .position(|(_, name)| *name == "key.jump")
            .unwrap();
    assert!(settings.remap(jump, PhysicalControl::GamepadButton(3)));
    assert!(!settings.remap(jump, PhysicalControl::KeyboardUsage(0x0c)));
    let keyboard = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == "key.forward")
        .unwrap();
    assert!(settings.remap(keyboard, PhysicalControl::KeyboardUsage(0x0c)));
    let mut restored = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(
        restored
            .user_settings()
            .controls
            .bindings()
            .iter()
            .any(|binding| binding.action == semantic_input::Action::Jump
                && binding.context == InputContext::Gameplay
                && binding.chord.control == PhysicalControl::GamepadButton(3))
    );
    restored.reset_bindings(false);
    assert_eq!(
        restored.key_control(jump),
        Some(PhysicalControl::GamepadButton(3))
    );
    assert_eq!(
        restored.key_control(keyboard),
        SettingsOptions::default().key_control(keyboard)
    );
    restored.reset_bindings(true);
    assert_eq!(
        restored.key_control(jump),
        SettingsOptions::default().key_control(jump)
    );
}

#[test]
fn resetting_a_ui_key_preserves_remaps_when_its_default_was_reassigned() {
    use super::EXTRA_KEYS;
    let mut settings = SettingsOptions::default();
    let inventory = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.inventory")
            .unwrap();
    let attack = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == "key.attack")
        .unwrap();
    assert!(settings.remap(inventory, PhysicalControl::KeyboardUsage(0x0c)));
    assert!(settings.remap(attack, PhysicalControl::KeyboardUsage(0x08)));
    assert!(!settings.reset_key(inventory));
    assert_eq!(
        settings.key_control(inventory),
        Some(PhysicalControl::KeyboardUsage(0x0c))
    );
}

#[test]
fn every_supplemental_binding_survives_reload_and_individual_reset() {
    use super::{EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET};
    for (row, _) in EXTRA_KEYS.iter().enumerate() {
        let mut settings = SettingsOptions::default();
        let index = KEY_BINDINGS.len() + row;
        let control = PhysicalControl::KeyboardUsage(0x45);
        assert!(settings.remap(index, control));
        let mut restored =
            SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored.key_control(index), Some(control));
        assert!(restored.reset_key(index));
        assert_eq!(
            restored.key_control(index),
            SettingsOptions::default().key_control(index)
        );
    }
    for (row, _) in EXTRA_GAMEPAD.iter().enumerate() {
        let mut settings = SettingsOptions::default();
        let index = GAMEPAD_OFFSET + GAMEPAD_BINDINGS.len() + row;
        let control = PhysicalControl::GamepadButton(9);
        assert!(settings.remap(index, control));
        let mut restored =
            SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored.key_control(index), Some(control));
        assert!(restored.reset_key(index));
        assert_eq!(
            restored.key_control(index),
            SettingsOptions::default().key_control(index)
        );
    }
}

#[test]
fn untouched_and_legacy_options_keep_block_outline_selection() {
    assert!(
        SettingsOptions::default()
            .user_settings()
            .video
            .outline_selection
    );
    for saved in [b"{}".as_slice(), br#"{"values":{"gamma":40}}"#.as_slice()] {
        let restored = SettingsOptions::decode(saved).unwrap();
        assert!(restored.user_settings().video.outline_selection);
    }
}

#[test]
fn outline_selection_reaches_render_settings_after_persistence() {
    let mut settings = SettingsOptions::default();
    for enabled in [true, false] {
        settings.set(index("classic_box_selection"), i32::from(enabled));
        let restored = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored.user_settings().video.outline_selection, enabled);
    }
}

#[test]
fn loaded_supplemental_bindings_cannot_conflict_with_gameplay_controls() {
    let loaded = SettingsOptions::decode(br#"{"keys":{"key.inventory":257}}"#).unwrap();
    assert_eq!(
        loaded.named_key_control("key.inventory"),
        SettingsOptions::default().named_key_control("key.inventory")
    );
}

#[test]
fn always_sprint_defaults_off_and_persists_into_runtime_settings() {
    let mut settings = SettingsOptions::default();
    assert!(!settings.user_settings().gameplay.always_sprint);
    settings.set(index("always_sprint"), 1);
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(loaded.user_settings().gameplay.always_sprint);
    let legacy =
        SettingsOptions::decode(br#"{"values":{"keyboard_mouse_sensitivity":75}}"#).unwrap();
    assert!(!legacy.user_settings().gameplay.always_sprint);
}

#[test]
fn vsync_defaults_on_and_persists_into_runtime_settings() {
    let mut settings = SettingsOptions::default();
    assert!(settings.user_settings().video.vsync);
    settings.set(index("vsync"), 0);
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(!loaded.user_settings().video.vsync);
    let legacy = SettingsOptions::decode(br#"{"values":{"gamma":40}}"#).unwrap();
    assert!(legacy.user_settings().video.vsync);
}

#[test]
fn exact_server_ping_is_optional_and_persists_with_settings() {
    let mut settings = SettingsOptions::default();
    assert!(!settings.exact_server_ping());
    settings.set(index(SHOW_EXACT_SERVER_PING), 1);
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(loaded.exact_server_ping());
    let legacy = SettingsOptions::decode(br#"{"values":{"gamma":40}}"#).unwrap();
    assert!(!legacy.exact_server_ping());
}

#[test]
fn dark_mode_defaults_off_persists_and_resets_with_video_settings() {
    let mut settings = SettingsOptions::default();
    assert!(!settings.oreui_dark_mode());
    settings.set(index(OREUI_DARK_MODE), 1);
    let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(loaded.oreui_dark_mode());
    loaded.reset_group(SettingsGroup::Audio);
    assert!(loaded.oreui_dark_mode());
    loaded.reset_group(SettingsGroup::Video);
    assert!(!loaded.oreui_dark_mode());
    let legacy = SettingsOptions::decode(br#"{"values":{"gamma":40}}"#).unwrap();
    assert!(!legacy.oreui_dark_mode());
}

#[test]
fn animations_default_to_java_and_persist_both_choices() {
    let mut settings = SettingsOptions::default();
    let index = index("animations");
    assert_eq!(settings.get(index), 0);
    assert!(settings.user_settings().video.java_animations);
    for choice in [1, 0] {
        settings.set(index, choice);
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(loaded.get(index), choice);
        assert_eq!(loaded.user_settings().video.java_animations, choice == 0);
    }
    let untouched = SettingsOptions::decode(br#"{"values":{"gamma":40}}"#).unwrap();
    assert!(untouched.user_settings().video.java_animations);
    settings.set(index, 1);
    settings.reset_group(super::SettingsGroup::Video);
    assert!(settings.user_settings().video.java_animations);
}

#[test]
fn animations_migrate_legacy_toggle_without_overriding_a_saved_selection() {
    for (toggle, choice) in [(0, 1), (1, 0)] {
        let legacy = serde_json::json!({ "values": { "java_animations": toggle } });
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(loaded.value("animations"), choice);
        assert_eq!(loaded.user_settings().video.java_animations, toggle != 0);
        let saved = serde_json::to_value(&loaded).unwrap();
        assert!(saved["values"].get("java_animations").is_none());
        assert_eq!(saved["values"]["animations"], choice);
        let selected = serde_json::json!({
            "values": { "java_animations": toggle, "animations": toggle }
        });
        let loaded = SettingsOptions::decode(&serde_json::to_vec(&selected).unwrap()).unwrap();
        assert_eq!(loaded.value("animations"), toggle);
    }
}

#[test]
fn crosshair_preferences_persist_and_reset_without_changing_legacy_defaults() {
    use super::{INVERT_CROSSHAIR_OPTION, SettingsGroup, THIRD_PERSON_CROSSHAIR_OPTION};
    let mut options = SettingsOptions::decode(br#"{"values":{"gamma":40}}"#).unwrap();
    for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
        assert_eq!(options.value(option.name), option.default);
        options.set(index(option.name), 1 - option.default);
    }
    let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&options).unwrap()).unwrap();
    for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
        assert_eq!(loaded.value(option.name), 1 - option.default);
    }
    loaded.reset_group(SettingsGroup::Video);
    for option in [THIRD_PERSON_CROSSHAIR_OPTION, INVERT_CROSSHAIR_OPTION] {
        assert_eq!(loaded.value(option.name), option.default);
    }
}

/// Vanilla 1.26.50 ships auto-jump off for keyboard/mouse and controller alike.
#[test]
fn auto_jump_defaults_off_for_every_input_mode() {
    let settings = SettingsOptions::default();
    for name in ["keyboard_mouse_autojump", "controller_autojump"] {
        assert_eq!(settings.get(index(name)), 0, "{name}");
    }
}
