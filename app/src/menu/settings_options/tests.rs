use super::*;
use semantic_input::{InputContext, PhysicalControl};

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

#[test]
fn camera_input_and_window_settings_read_the_saved_values() {
    let mut settings = SettingsOptions::default();
    for (name, value) in [
        ("field_of_view", 82),
        ("view_bobbing", 0),
        ("field_of_view_toggle", 0),
        ("camera_shake", 0),
        ("damage_bob", 25),
        ("keyboard_mouse_sensitivity", 75),
        ("keyboard_mouse_invert_y_axis", 1),
        ("third_person", 2),
        ("max_framerate", 120),
    ] {
        settings.set(index(name), value);
    }
    let settings = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    let user = settings.user_settings();
    let mut authority = crate::camera::CameraSettingsAuthority::default();
    authority.replace(1, &user).unwrap();
    assert_eq!(authority.horizontal_fov_degrees(), 82.0);
    assert!(!authority.feel().view_bobbing);
    assert!(!authority.feel().camera_shake);
    assert_eq!(authority.feel().damage_bob, 0.25);
    assert_eq!(authority.feel().fov_effects_scale, 0.0);
    assert_eq!(user.controls.mouse_sensitivity, 1.5);
    assert!(user.controls.invert_mouse_y);
    assert_eq!(user.video.frame_cap, Some(120));
    assert_eq!(
        authority.perspective(),
        semantic_input::PerspectiveMode::ThirdPersonFront
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
fn saved_volumes_reach_the_mixer() {
    use crate::{
        audio::{AudioCategory, AudioSettings},
        menu::MenuRuntime,
    };
    use bevy::prelude::{App, ResMut, Update};
    /// Exercises the production sound adapter without writing settings to disk.
    fn sync(mut menu: ResMut<MenuRuntime>, audio: ResMut<AudioSettings>) {
        menu.sync_audio_settings(Some(audio));
    }
    let mut menu = MenuRuntime::new(true, 2, "Settings test".to_owned());
    menu.set_option(index("main_volume") as u16, 50);
    menu.set_option(index("music_volume") as u16, 40);
    let mut app = App::new();
    app.insert_resource(menu)
        .init_resource::<AudioSettings>()
        .add_systems(Update, sync);
    app.update();
    let mixer = app.world().resource::<AudioSettings>();
    assert!((mixer.effective(AudioCategory::Music) - 0.2).abs() < 0.0001);
    assert_eq!(mixer.volume(AudioCategory::Master), 0.5);
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
fn supplemental_bindings_drive_production_keyboard_and_mouse_helpers() {
    use super::{EXTRA_KEYS, binding_key, binding_mouse, binding_pressed};
    use bevy::prelude::{ButtonInput, KeyCode, MouseButton};
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Bindings".to_owned());
    let index = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.inventory")
            .unwrap();
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options)
            .remap(index, PhysicalControl::KeyboardUsage(0x0c))
    );
    let mut keys = ButtonInput::default();
    let mut mouse = ButtonInput::default();
    keys.press(KeyCode::KeyI);
    assert!(binding_key(Some(&menu), "key.inventory", KeyCode::KeyI));
    assert!(!binding_key(Some(&menu), "key.inventory", KeyCode::KeyE));
    assert!(binding_pressed(Some(&menu), "key.inventory", &keys, &mouse));
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options)
            .remap(index, PhysicalControl::MouseButton(4))
    );
    mouse.press(MouseButton::Back);
    assert!(binding_mouse(Some(&menu), "key.inventory", &mouse));
    assert!(binding_pressed(Some(&menu), "key.inventory", &keys, &mouse));
}

#[test]
fn chat_secondary_default_obeys_remapping_and_reset_conflicts() {
    use super::{EXTRA_KEYS, binding_key, binding_pressed};
    use bevy::prelude::{ButtonInput, KeyCode};
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Bindings".to_owned());
    let chat = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.chat")
            .unwrap();
    let inventory = KEY_BINDINGS.len()
        + EXTRA_KEYS
            .iter()
            .position(|(name, _)| *name == "key.inventory")
            .unwrap();
    let enter = PhysicalControl::KeyboardUsage(0x28);
    let mut keys = ButtonInput::default();
    keys.press(KeyCode::Enter);
    assert!(binding_key(Some(&menu), "key.chat", KeyCode::KeyT));
    assert!(binding_pressed(
        Some(&menu),
        "key.chat",
        &keys,
        &ButtonInput::default()
    ));
    let settings = std::sync::Arc::make_mut(&mut menu.settings_options);
    assert!(!settings.remap(inventory, enter));
    assert!(settings.remap(chat, PhysicalControl::KeyboardUsage(0x1c)));
    assert!(settings.remap(inventory, enter));
    assert!(!settings.reset_key(chat));
    assert!(!binding_key(Some(&menu), "key.chat", KeyCode::Enter));
    assert!(binding_key(Some(&menu), "key.chat", KeyCode::KeyY));
    let settings = std::sync::Arc::make_mut(&mut menu.settings_options);
    assert!(settings.reset_key(inventory));
    assert!(settings.reset_key(chat));
    assert!(binding_key(Some(&menu), "key.chat", KeyCode::Enter));
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
fn controller_swaps_agree_between_display_capture_router_and_menu() {
    use super::{GAMEPAD_BINDINGS, GAMEPAD_OFFSET};
    use bevy::input::gamepad::GamepadButton;
    let mut settings = SettingsOptions::default();
    settings.set(index("swap_gamepad_ab_buttons"), 1);
    settings.set(index("swap_gamepad_xy_buttons"), 1);
    let jump = GAMEPAD_OFFSET
        + GAMEPAD_BINDINGS
            .iter()
            .position(|(_, name)| *name == "key.jump")
            .unwrap();
    assert_eq!(
        settings.key_control(jump),
        Some(PhysicalControl::GamepadButton(1))
    );
    assert_eq!(
        settings.gamepad_button(GamepadButton::South),
        GamepadButton::East
    );
    assert_eq!(
        settings.gamepad_button(GamepadButton::North),
        GamepadButton::West
    );
    assert!(
        settings
            .user_settings()
            .controls
            .bindings()
            .iter()
            .any(|binding| binding.action == semantic_input::Action::Jump
                && binding.context == InputContext::Gameplay
                && binding.chord.control == PhysicalControl::GamepadButton(1))
    );
    assert!(settings.remap(jump, PhysicalControl::GamepadButton(2)));
    assert_eq!(
        settings.key_control(jump),
        Some(PhysicalControl::GamepadButton(2))
    );
    assert!(
        settings
            .user_settings()
            .controls
            .bindings()
            .iter()
            .any(|binding| binding.action == semantic_input::Action::Jump
                && binding.context == InputContext::Gameplay
                && binding.chord.control == PhysicalControl::GamepadButton(2))
    );
}

#[test]
fn spyglass_damping_uses_the_selected_desktop_input_mode() {
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Spyglass".to_owned());
    menu.set_option(index("spyglass_mouse_dampening") as u16, 25);
    menu.set_option(index("spyglass_gamepad_dampening") as u16, 75);
    assert_eq!(
        menu.spyglass_damping(semantic_input::InputMode::KeyboardMouse),
        0.25
    );
    assert_eq!(
        menu.spyglass_damping(semantic_input::InputMode::GamePad),
        0.75
    );
}

#[test]
fn glint_renderer_factors_follow_each_persisted_percent() {
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Glint".to_owned());
    menu.set_option(index("glint_strength") as u16, 25);
    menu.set_option(index("glint_speed") as u16, 75);
    assert_eq!(
        menu.ui_glint_settings(),
        render::UiGlintSettings {
            strength: 0.25,
            speed: 0.75,
        }
    );
    menu.set_option(index("glint_strength") as u16, 0);
    assert_eq!(menu.ui_glint_settings().strength, 0.0);
    assert_eq!(menu.ui_glint_settings().speed, 0.75);
}

#[test]
fn settings_handoff_preserves_native_window_and_viewport_preferences() {
    use crate::{menu::MenuRuntime, settings_runtime::RuntimeSettings};
    use bevy::prelude::{App, ResMut, Update};

    /// Applies the runtime handoff without flushing host configuration.
    fn sync(mut menu: ResMut<MenuRuntime>, settings: ResMut<RuntimeSettings>) {
        menu.settings_dirty = false;
        menu.sync_user_settings(Some(settings));
    }
    let mut menu = MenuRuntime::new(true, 3, "Settings test".into());
    menu.sync_fullscreen(true);
    menu.set_option(index("gamma") as u16, 80);
    let mut runtime = RuntimeSettings::default();
    let mut user = ui::UserSettings::default();
    user.video.ui_scale = 3.0;
    user.video.render_mode = ui::RenderMode::Enhanced;
    runtime.replace_user_settings(user);
    let mut app = App::new();
    app.insert_resource(menu)
        .insert_resource(runtime)
        .add_systems(Update, sync);
    app.update();
    let settings = app
        .world()
        .resource::<RuntimeSettings>()
        .user_settings_update()
        .1;
    assert!(settings.video.fullscreen);
    assert_eq!(settings.video.ui_scale, 3.0);
    assert_eq!(settings.video.render_mode, ui::RenderMode::Enhanced);
    assert_eq!(settings.video.brightness, 0.8);
    assert_eq!(
        app.world().resource::<MenuRuntime>().gui_scale_preference(),
        Some(3)
    );
    let saved = SettingsOptions::decode(br#"{"values":{"gui_scale":4,"full_screen":0}}"#).unwrap();
    assert!(!saved.values.contains_key("gui_scale"));
    assert!(!saved.values.contains_key("full_screen"));
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
fn a_failed_settings_save_remains_pending_until_storage_recovers() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-settings-retry-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::create_dir_all(&root).unwrap();
    let blocked = root.join("config");
    std::fs::write(&blocked, b"blocked").unwrap();
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Steve".into());
    menu.config_path = blocked.join("servers.json");
    menu.set_option(index("gamma") as u16, 70);
    menu.sync_user_settings(None);
    assert!(menu.settings_dirty);
    std::fs::remove_file(&blocked).unwrap();
    menu.settings_retry_at = Some(std::time::Instant::now());
    menu.sync_user_settings(None);
    assert!(!menu.settings_dirty);
    assert_eq!(
        SettingsOptions::load(&blocked.join(SETTINGS_FILE)).value("gamma"),
        70
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn review_ui_failed_settings_save_does_not_retry_on_the_next_frame() {
    let root = std::env::temp_dir().join(format!(
        "cinnabar-settings-backoff-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let path = root.join(SETTINGS_FILE);
    std::fs::create_dir_all(&path).unwrap();
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Steve".into());
    menu.config_path = root.join("servers.json");
    menu.set_option(index("gamma") as u16, 70);
    menu.sync_user_settings(None);
    assert!(menu.settings_dirty);
    std::fs::remove_dir(&path).unwrap();
    // Even recovered storage must wait: this frame must not perform another write.
    menu.set_option(index("gamma") as u16, 80);
    menu.sync_user_settings(None);
    assert!(menu.settings_dirty);
    assert!(!path.exists());
    // Advance the retry deadline without sleeping, and persist the latest edit.
    menu.settings_retry_at = Some(std::time::Instant::now());
    menu.sync_user_settings(None);
    assert!(!menu.settings_dirty);
    assert!(menu.settings_retry_at.is_none());
    assert_eq!(SettingsOptions::load(&path).value("gamma"), 80);
    std::fs::remove_dir_all(root).unwrap();
}
