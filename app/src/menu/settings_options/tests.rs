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
    assert_eq!(user.controls.mouse_sensitivity, 0.75);
    assert!(user.controls.invert_mouse_y);
    assert_eq!(
        user.video.frame_rate_limit,
        render_api::FrameRateLimit::Fixed(std::num::NonZeroU16::new(120).unwrap())
    );
    assert_eq!(
        authority.perspective(),
        semantic_input::PerspectiveMode::ThirdPersonFront
    );
}

#[test]
fn server_list_actions_save_preferences_without_changing_selection_or_gameplay() {
    use crate::menu::{MenuAction, MenuRuntime};
    use launcher::menu::server_list::{ServerGroup, ServerListAction};
    let mut menu = MenuRuntime::new(true, 2, "Server list test".into());
    menu.feeds.select_saved(2);
    menu.settings_apply = false;
    menu.activate(MenuAction::ServerList(ServerListAction::Toggle(
        ServerGroup::Featured,
    )));
    menu.activate(MenuAction::ServerList(ServerListAction::MoveBefore(
        ServerGroup::Saved,
        Some(ServerGroup::Featured),
    )));
    assert!(menu.settings_dirty);
    assert!(!menu.settings_apply);
    assert_eq!(menu.feeds.selected_saved, Some(2));
    menu.sync_user_settings(None);
    assert!(!menu.settings_dirty);
    let saved = SettingsOptions::load(&menu.config_path.with_file_name(SETTINGS_FILE));
    assert!(saved.server_list().collapsed(ServerGroup::Featured));
    assert_eq!(saved.server_list().order()[0], ServerGroup::Saved);
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
        gamepad_button(&settings, GamepadButton::South),
        GamepadButton::East
    );
    assert_eq!(
        gamepad_button(&settings, GamepadButton::North),
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
    let encoded = serde_json::to_value(&saved).unwrap();
    let values = encoded["values"].as_object().unwrap();
    assert!(!values.contains_key("gui_scale"));
    assert!(!values.contains_key("full_screen"));
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
    let attempted_at = std::time::Instant::now();
    menu.sync_user_settings(None);
    assert!(menu.settings_dirty);
    let retry_at = menu
        .settings_retry_at
        .expect("a failed save schedules a retry");
    assert!(retry_at > attempted_at);
    // Hold the deadline ahead of this check even if the test thread is delayed.
    menu.settings_retry_at = Some(retry_at + std::time::Duration::from_secs(60));
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

#[test]
fn session_overrides_apply_in_memory_but_are_never_saved() {
    let layout = crate::install_layout::scratch("session-overrides");
    let settings_path = layout.server_file().with_file_name(SETTINGS_FILE);
    let skin = crate::player_skin::LocalPlayerSkin::generated_default("Overrides");
    let mut menu =
        crate::menu::MenuRuntime::new_with_layout(true, Some(2), "Overrides".into(), layout, skin);
    menu.set_session_option("hide_hud", Some(1));
    menu.set_session_option("hide_hand", Some(1));
    assert_eq!(menu.settings_snapshot().0.value("hide_hud"), 1);
    // A real edit elsewhere saves the file without the overrides.
    menu.set_option(index("main_volume") as u16, 30);
    menu.sync_user_settings(None);
    let saved = SettingsOptions::load(&settings_path);
    assert_eq!(saved.value("hide_hud"), 0);
    assert_eq!(saved.value("hide_hand"), 0);
    assert_eq!(saved.value("main_volume"), 30);
    menu.set_session_option("hide_hud", None);
    assert_eq!(menu.settings_snapshot().0.value("hide_hud"), 0);
    assert_eq!(menu.settings_snapshot().0.value("hide_hand"), 1);
}

#[test]
fn inventory_hotbar_controls_follow_saved_keyboard_and_mouse_remaps() {
    use super::hotbar_control_slot;
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Bindings".to_owned());
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == semantic_input::Action::Hotbar1)
        .unwrap();
    assert_eq!(
        hotbar_control_slot(Some(&menu), PhysicalControl::KeyboardUsage(0x1e)),
        Some(0)
    );
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options)
            .remap(row, PhysicalControl::KeyboardUsage(0x15))
    );
    menu.settings_options = std::sync::Arc::new(
        SettingsOptions::decode(&serde_json::to_vec(menu.settings_options.as_ref()).unwrap())
            .unwrap(),
    );
    assert_eq!(
        hotbar_control_slot(Some(&menu), PhysicalControl::KeyboardUsage(0x15)),
        Some(0)
    );
    assert_eq!(
        hotbar_control_slot(Some(&menu), PhysicalControl::KeyboardUsage(0x1e)),
        None
    );
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options)
            .remap(row, PhysicalControl::MouseButton(4))
    );
    assert_eq!(
        hotbar_control_slot(Some(&menu), PhysicalControl::MouseButton(4)),
        Some(0)
    );
    assert_eq!(
        hotbar_control_slot(Some(&menu), PhysicalControl::KeyboardUsage(0x15)),
        None
    );
    for (slot, usage) in (0x1f..=0x26).enumerate() {
        assert_eq!(
            hotbar_control_slot(Some(&menu), PhysicalControl::KeyboardUsage(usage)),
            Some(slot as u8 + 1)
        );
    }
}
