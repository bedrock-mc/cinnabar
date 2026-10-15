use launcher::menu::settings_options::BindingId;
use launcher::menu::settings_options::SettingId;
use semantic_input::{InputContext, PhysicalControl};
use {
    crate::menu::settings_options::control_bindings::{
        binding_key, binding_macro_command, binding_mouse, binding_pressed, gamepad_button,
        hotbar_control_slots,
    },
    launcher::menu::settings_options::{
        EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET, KEY_BINDINGS, SETTINGS_FILE,
        SETTINGS_OPTIONS, SettingsOptions,
    },
};

/// Finds an option through the same stable controller identifier used on disk.
fn index(name: &str) -> usize {
    SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .unwrap()
}

#[test]
fn server_list_actions_save_preferences_without_changing_selection_or_gameplay() {
    use launcher::menu::server_list::{ServerGroup, ServerListAction};
    use {crate::menu::MenuRuntime, launcher::menu::MenuAction};
    let mut menu = MenuRuntime::new(true, 2, "Server list test".into());
    menu.feeds.select_saved(2);
    menu.settings_apply = false;
    menu.activate(MenuAction::ServerList(ServerListAction::Toggle(
        ServerGroup::Featured,
    )));
    menu.activate(MenuAction::ServerList(ServerListAction::MoveBefore(
        ServerGroup::Creator,
        Some(ServerGroup::Saved),
    )));
    assert!(menu.settings_dirty);
    assert!(!menu.settings_apply);
    assert_eq!(menu.feeds.selected_saved, Some(2));
    menu.sync_user_settings(None);
    assert!(!menu.settings_dirty);
    menu.flush_user_settings().unwrap();
    let saved = SettingsOptions::load(&menu.config_path.with_file_name(SETTINGS_FILE));
    assert!(saved.server_list().collapsed(ServerGroup::Featured));
    assert_eq!(
        saved.server_list().order(),
        [
            ServerGroup::Creator,
            ServerGroup::Saved,
            ServerGroup::Featured
        ]
    );
}

#[test]
fn supplemental_bindings_drive_production_keyboard_and_mouse_helpers() {
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
    assert!(binding_key(
        Some(&menu),
        BindingId::Inventory,
        KeyCode::KeyI
    ));
    assert!(!binding_key(
        Some(&menu),
        BindingId::Inventory,
        KeyCode::KeyE
    ));
    assert!(binding_pressed(
        Some(&menu),
        BindingId::Inventory,
        &keys,
        &mouse
    ));
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options)
            .remap(index, PhysicalControl::MouseButton(4))
    );
    mouse.press(MouseButton::Back);
    assert!(binding_mouse(Some(&menu), BindingId::Inventory, &mouse));
    assert!(binding_pressed(
        Some(&menu),
        BindingId::Inventory,
        &keys,
        &mouse
    ));
}

/// An Alt+key macro resolves its stored command, and only while it is armed.
#[test]
fn a_bound_macro_runs_its_command_only_while_a_command_is_stored() {
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Macros".to_owned());
    let modifiers = semantic_input::ModifierChord {
        alt: true,
        ..Default::default()
    };
    let slot = 1;
    let key = PhysicalControl::KeyboardUsage(0x1e);
    // A binding without a stored command stays silent.
    assert!(std::sync::Arc::make_mut(&mut menu.settings_options).remap_macro(slot, key));
    assert_eq!(binding_macro_command(Some(&menu), key, modifiers), None);
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options)
            .set_macro_command(slot, "time set day")
    );
    assert_eq!(
        binding_macro_command(Some(&menu), key, modifiers).as_deref(),
        Some("/time set day")
    );
    // Other keys, and the macro key after its binding is cleared, stay silent.
    assert_eq!(
        binding_macro_command(Some(&menu), PhysicalControl::KeyboardUsage(0x1f), modifiers),
        None
    );
    assert!(std::sync::Arc::make_mut(&mut menu.settings_options).reset_macro(slot));
    assert_eq!(binding_macro_command(Some(&menu), key, modifiers), None);
    // A stored slash is preserved rather than doubled.
    assert!(std::sync::Arc::make_mut(&mut menu.settings_options).remap_macro(slot, key));
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options).set_macro_command(slot, "/say hello")
    );
    assert_eq!(
        binding_macro_command(Some(&menu), key, modifiers).as_deref(),
        Some("/say hello")
    );
}

#[test]
fn chat_secondary_default_obeys_remapping_and_allows_a_shared_reset() {
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
    assert!(binding_key(Some(&menu), BindingId::Chat, KeyCode::KeyT));
    assert!(binding_pressed(
        Some(&menu),
        BindingId::Chat,
        &keys,
        &ButtonInput::default()
    ));
    let settings = std::sync::Arc::make_mut(&mut menu.settings_options);
    assert!(settings.remap(inventory, enter));
    assert!(settings.remap(chat, PhysicalControl::KeyboardUsage(0x1c)));
    assert!(settings.remap(inventory, enter));
    assert!(!binding_key(Some(&menu), BindingId::Chat, KeyCode::Enter));
    assert!(binding_key(Some(&menu), BindingId::Chat, KeyCode::KeyY));
    let settings = std::sync::Arc::make_mut(&mut menu.settings_options);
    assert!(settings.reset_key(chat));
    assert!(binding_key(Some(&menu), BindingId::Chat, KeyCode::Enter));
    assert!(binding_key(
        Some(&menu),
        BindingId::Inventory,
        KeyCode::Enter
    ));
    assert!(std::sync::Arc::make_mut(&mut menu.settings_options).reset_key(inventory));
}

#[test]
fn controller_swaps_agree_between_display_capture_router_and_menu() {
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
        render_scene::UiGlintSettings {
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
    use bevy::prelude::{App, ResMut, Update};
    use {crate::menu::MenuRuntime, client_state::RuntimeSettings};

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
fn a_failed_settings_save_is_reported_and_shutdown_saves_latest_edits_after_recovery() {
    let layout = launcher::test_support::scratch("settings-retry");
    let blocked = layout.user_config_root.join("config");
    std::fs::create_dir_all(&layout.user_config_root).unwrap();
    std::fs::write(&blocked, b"blocked").unwrap();
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Steve".into());
    menu.config_path = blocked.join("servers.json");
    menu.message = None;
    menu.set_option(index("gamma") as u16, 70);
    menu.sync_user_settings(None);
    test_time::eventually("settings write failure", || {
        menu.sync_user_settings(None);
        menu.message
            .as_deref()
            .is_some_and(|message| message.starts_with("Could not save settings:"))
    });
    std::fs::remove_file(&blocked).unwrap();
    menu.set_option(index("gamma") as u16, 80);
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    assert_eq!(
        SettingsOptions::load(&blocked.join(SETTINGS_FILE)).value(SettingId::Gamma),
        80
    );
    drop(menu);
    std::fs::remove_dir_all(layout.user_config_root).unwrap();
}

#[test]
fn shutdown_saves_the_latest_edit_even_without_another_frame() {
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Shutdown".into());
    let path = menu.config_path.with_file_name(SETTINGS_FILE);
    menu.set_option(index("gamma") as u16, 70);
    menu.sync_user_settings(None);
    menu.set_option(index("gamma") as u16, 80);
    drop(menu);
    assert_eq!(SettingsOptions::load(&path).value(SettingId::Gamma), 80);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn session_overrides_apply_in_memory_but_are_never_saved() {
    let layout = launcher::test_support::scratch("session-overrides");
    let settings_path = layout.server_file().with_file_name(SETTINGS_FILE);
    let skin = crate::player_skin::LocalPlayerSkin::generated_default("Overrides");
    let mut menu =
        crate::menu::MenuRuntime::new_with_layout(true, Some(2), "Overrides".into(), layout, skin);
    menu.set_session_option(SettingId::HideHud, Some(1));
    menu.set_session_option(SettingId::HideHand, Some(1));
    assert_eq!(menu.settings_snapshot().0.value(SettingId::HideHud), 1);
    // A real edit elsewhere saves the file without the overrides.
    menu.set_option(index("main_volume") as u16, 30);
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    let saved = SettingsOptions::load(&settings_path);
    assert_eq!(saved.value(SettingId::HideHud), 0);
    assert_eq!(saved.value(SettingId::HideHand), 0);
    assert_eq!(saved.value(SettingId::MainVolume), 30);
    menu.set_session_option(SettingId::HideHud, None);
    assert_eq!(menu.settings_snapshot().0.value(SettingId::HideHud), 0);
    assert_eq!(menu.settings_snapshot().0.value(SettingId::HideHand), 1);
}

#[test]
fn inventory_hotbar_controls_follow_saved_keyboard_and_mouse_remaps() {
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Bindings".to_owned());
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == semantic_input::Action::Hotbar1)
        .unwrap();
    assert_eq!(
        hotbar_control_slots(Some(&menu), PhysicalControl::KeyboardUsage(0x1e)).collect::<Vec<_>>(),
        vec![0]
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
        hotbar_control_slots(Some(&menu), PhysicalControl::KeyboardUsage(0x15)).collect::<Vec<_>>(),
        vec![0]
    );
    assert_eq!(
        hotbar_control_slots(Some(&menu), PhysicalControl::KeyboardUsage(0x1e)).collect::<Vec<_>>(),
        Vec::<u8>::new()
    );
    assert!(
        std::sync::Arc::make_mut(&mut menu.settings_options)
            .remap(row, PhysicalControl::MouseButton(4))
    );
    assert_eq!(
        hotbar_control_slots(Some(&menu), PhysicalControl::MouseButton(4)).collect::<Vec<_>>(),
        vec![0]
    );
    assert_eq!(
        hotbar_control_slots(Some(&menu), PhysicalControl::KeyboardUsage(0x15)).collect::<Vec<_>>(),
        Vec::<u8>::new()
    );
    for (slot, usage) in (0x1f..=0x26).enumerate() {
        assert_eq!(
            hotbar_control_slots(Some(&menu), PhysicalControl::KeyboardUsage(usage))
                .collect::<Vec<_>>(),
            vec![slot as u8 + 1]
        );
    }
}

#[test]
fn exit_message_flushes_edits_before_the_menu_is_dropped() {
    use bevy::prelude::{App, AppExit, Last};
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Exit".into());
    let path = menu.config_path.with_file_name(SETTINGS_FILE);
    menu.set_option(index("gamma") as u16, 80);
    let mut app = App::new();
    app.add_message::<AppExit>()
        .insert_resource(menu)
        .add_systems(Last, super::flush_on_exit);
    app.world_mut().write_message(AppExit::Success);
    app.update();
    assert_eq!(SettingsOptions::load(&path).value(SettingId::Gamma), 80);
    drop(app);
    std::fs::remove_file(path).unwrap();
}
