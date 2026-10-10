use semantic_input::{InputContext, PhysicalControl};
use {
    crate::menu::settings_options::control_bindings::{
        binding_key, binding_mouse, binding_pressed, gamepad_button, hotbar_control_slots,
    },
    launcher::menu::settings_options::{
        EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET, KEY_BINDINGS, SETTINGS_FILE,
        SETTINGS_OPTIONS, SettingsOptions, frame_rate_limit,
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
    let mut authority = client_presentation::camera::CameraSettingsAuthority::default();
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
    use launcher::menu::server_list::{ServerGroup, ServerListAction};
    use {crate::menu::MenuRuntime, launcher::menu::MenuAction};
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
    use bevy::prelude::{App, ResMut, Update};
    use {
        crate::menu::MenuRuntime,
        client_presentation::audio::{AudioCategory, AudioSettings},
    };
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
    assert!(binding_key(Some(&menu), "key.chat", KeyCode::KeyT));
    assert!(binding_pressed(
        Some(&menu),
        "key.chat",
        &keys,
        &ButtonInput::default()
    ));
    let settings = std::sync::Arc::make_mut(&mut menu.settings_options);
    assert!(settings.remap(inventory, enter));
    assert!(settings.remap(chat, PhysicalControl::KeyboardUsage(0x1c)));
    assert!(settings.remap(inventory, enter));
    assert!(!binding_key(Some(&menu), "key.chat", KeyCode::Enter));
    assert!(binding_key(Some(&menu), "key.chat", KeyCode::KeyY));
    let settings = std::sync::Arc::make_mut(&mut menu.settings_options);
    assert!(settings.reset_key(chat));
    assert!(binding_key(Some(&menu), "key.chat", KeyCode::Enter));
    assert!(binding_key(Some(&menu), "key.inventory", KeyCode::Enter));
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
fn shared_hotbar_press_swaps_each_bound_slot_in_order() {
    use std::sync::Arc;

    use client_ui::ui_runtime::presentation::inventory_pointer::InventoryCellHit;
    use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};
    use semantic_input::Action;

    for control in [
        PhysicalControl::KeyboardUsage(0x15),
        PhysicalControl::MouseButton(4),
    ] {
        let mut menu = crate::menu::MenuRuntime::new(true, 2, "Bindings".into());
        for action in [Action::Hotbar1, Action::Hotbar2] {
            let row = launcher::menu::settings_options::KEY_BINDINGS
                .iter()
                .position(|(candidate, _)| *candidate == action)
                .unwrap();
            assert!(Arc::make_mut(&mut menu.settings_options).remap(row, control));
        }
        let mut player = player_state::PlayerState::new(1);
        let mut runtime = client_ui::test_support::inventory_session(&mut player);
        let ledger = runtime.inventory_ledger_mut(&mut player);
        assert!(ledger.request_personal_open(42));
        assert!(ledger.mark_transport_enqueued(0));
        ledger.apply(&InventoryEvent::Open(protocol::ContainerOpenEvent {
            container: ContainerIdentity::window(2),
            window_type: inventory::inventory_ledger::PERSONAL_INVENTORY_WINDOW_TYPE,
            position: [0; 3],
            runtime_entity_id: -1,
        }));
        let mut slots = vec![
            NetworkItemStack::empty();
            inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT
        ];
        for (slot, network_id) in [(0, 745), (1, 846), (20, 947)] {
            slots[slot] = NetworkItemStack {
                network_id,
                stack_network_id: network_id,
                count: 1,
                ..NetworkItemStack::empty()
            };
        }
        runtime
            .enqueue_inventory_event(
                &mut player,
                1,
                1,
                InventoryEvent::Content(InventoryContentEvent {
                    container: ContainerIdentity::window(0),
                    slots: slots.into(),
                    storage_item: NetworkItemStack::empty(),
                }),
            )
            .unwrap();
        runtime.drain_pending_inventory(&mut player);
        assert!(
            crate::ui_runtime::interaction::dispatch_bound_inventory_hotbar(
                Some(&menu),
                control,
                &mut player,
                &mut runtime,
                Some(InventoryCellHit::Player(20)),
            )
        );
        let ledger = runtime.inventory_ledger(&player);
        assert_eq!(ledger.displayed_stack(0).unwrap().network_id, 947);
        assert_eq!(ledger.displayed_stack(1).unwrap().network_id, 745);
        assert_eq!(ledger.displayed_stack(20).unwrap().network_id, 846);
        assert_eq!(ledger.pending_request_count(), 2);
    }
}

#[test]
fn shared_use_and_drop_drops_an_ordinary_item_but_use_alone_does_not() {
    use bevy::prelude::{App, ButtonInput, KeyCode, MouseButton, Update};
    use bevy::window::{PrimaryWindow, Window};
    use protocol::{ContainerIdentity, InventoryContentEvent, InventoryEvent, NetworkItemStack};

    for control in [
        PhysicalControl::KeyboardUsage(0x15),
        PhysicalControl::MouseButton(4),
    ] {
        for shared in [true, false] {
            let mut menu = crate::menu::MenuRuntime::new(false, 2, "Bindings".into());
            for name in ["key.use", "key.drop"]
                .into_iter()
                .take(if shared { 2 } else { 1 })
            {
                let row = KEY_BINDINGS
                    .iter()
                    .map(|(_, name)| *name)
                    .chain(EXTRA_KEYS.iter().map(|(name, _)| *name))
                    .position(|candidate| candidate == name)
                    .unwrap();
                assert!(std::sync::Arc::make_mut(&mut menu.settings_options).remap(row, control));
            }
            let mut player = crate::player_runtime::PlayerRuntime::new(1);
            let mut runtime = client_ui::test_support::inventory_session(&mut player);
            let mut slots = vec![
                NetworkItemStack::empty();
                inventory::inventory_ledger::PLAYER_INVENTORY_SLOT_COUNT
            ];
            slots[0] = NetworkItemStack {
                network_id: 745,
                stack_network_id: 13,
                count: 2,
                ..NetworkItemStack::empty()
            };
            runtime
                .enqueue_inventory_event(
                    &mut player,
                    1,
                    1,
                    InventoryEvent::Content(InventoryContentEvent {
                        container: ContainerIdentity::window(0),
                        slots: slots.into(),
                        storage_item: NetworkItemStack::empty(),
                    }),
                )
                .unwrap();
            runtime.drain_pending_inventory(&mut player);
            player.inventory.set_local_selected_slot(0);
            let mut keys = ButtonInput::<KeyCode>::default();
            let mut mouse = ButtonInput::<MouseButton>::default();
            match control {
                PhysicalControl::KeyboardUsage(_) => keys.press(KeyCode::KeyR),
                PhysicalControl::MouseButton(_) => mouse.press(MouseButton::Back),
                _ => unreachable!(),
            }
            let mut app = App::new();
            app.insert_resource(player)
                .insert_resource(runtime)
                .insert_resource(menu)
                .insert_resource(keys)
                .insert_resource(mouse)
                .add_systems(
                    Update,
                    crate::ui_runtime::interaction::drive_world_inventory_keys,
                );
            app.world_mut().spawn((
                Window {
                    focused: true,
                    ..Window::default()
                },
                PrimaryWindow,
            ));
            app.update();
            let player = app
                .world()
                .resource::<crate::player_runtime::PlayerRuntime>();
            let runtime = app.world().resource::<client_ui::ui_runtime::UiRuntime>();
            let ledger = runtime.inventory_ledger(player);
            assert_eq!(
                ledger.displayed_stack(0).unwrap().count,
                if shared { 1 } else { 2 }
            );
            assert_eq!(ledger.pending_request_count(), usize::from(shared));
        }
    }
}

#[test]
fn rebound_hotbar_keys_are_reserved_only_while_the_container_is_visible() {
    use crate::ui_runtime::interaction::inventory_consumes_key;
    use bevy::prelude::KeyCode;

    use semantic_input::Action;
    let mut menu = crate::menu::MenuRuntime::new(true, 2, "Bindings".into());
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Hotbar1)
        .unwrap();
    assert!(std::sync::Arc::make_mut(&mut menu.settings_options).remap(
        row,
        PhysicalControl::KeyboardUsage(
            crate::semantic_controls::keyboard_usage(KeyCode::KeyR).unwrap()
        )
    ));
    assert!(inventory_consumes_key(Some(&menu), KeyCode::KeyR, false));
    assert!(!inventory_consumes_key(Some(&menu), KeyCode::KeyR, true));
    assert!(!inventory_consumes_key(Some(&menu), KeyCode::Digit1, false));
}
