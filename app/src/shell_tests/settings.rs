use client_shell::menu::test_support as menu_fixture;
use client_shell::ui_resources::UiRuntimeResource;
use semantic_input::PhysicalControl;
use {
    launcher::menu::settings_options::EXTRA_KEYS, launcher::menu::settings_options::KEY_BINDINGS,
    launcher::menu::settings_options::SETTINGS_OPTIONS,
    launcher::menu::settings_options::SettingsOptions,
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
fn saved_volumes_reach_the_mixer() {
    use {bevy::prelude::App, bevy::prelude::ResMut, bevy::prelude::Update};
    use {
        client_audio::audio::AudioCategory, client_audio::audio::AudioSettings,
        client_shell::menu::MenuRuntime,
    };
    /// Exercises the production sound adapter without writing settings to disk.
    fn sync(menu: ResMut<MenuRuntime>, audio: ResMut<AudioSettings>) {
        crate::shell_adapters::sync_audio_settings(&menu, Some(audio));
    }
    let mut menu = MenuRuntime::new(true, 2, "Settings test".to_owned());
    menu.set_named_option("main_volume", 50);
    menu.set_named_option("music_volume", 40);
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
fn shared_hotbar_press_swaps_each_bound_slot_in_order() {
    use semantic_input::Action;
    use ui_screens::container_actions::InventoryCellHit;
    use {
        protocol::ContainerIdentity, protocol::InventoryContentEvent, protocol::InventoryEvent,
        protocol::NetworkItemStack,
    };

    for control in [
        PhysicalControl::KeyboardUsage(0x15),
        PhysicalControl::MouseButton(4),
    ] {
        let mut menu = client_shell::menu::MenuRuntime::new(true, 2, "Bindings".into());
        for action in [Action::Hotbar1, Action::Hotbar2] {
            let row = launcher::menu::settings_options::KEY_BINDINGS
                .iter()
                .position(|(candidate, _)| *candidate == action)
                .unwrap();
            assert!(menu_fixture::remap_control(&mut menu, row, control));
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
    use {
        bevy::prelude::App, bevy::prelude::ButtonInput, bevy::prelude::KeyCode,
        bevy::prelude::MouseButton, bevy::prelude::Update,
    };
    use {bevy::window::PrimaryWindow, bevy::window::Window};
    use {
        protocol::ContainerIdentity, protocol::InventoryContentEvent, protocol::InventoryEvent,
        protocol::NetworkItemStack,
    };

    for control in [
        PhysicalControl::KeyboardUsage(0x15),
        PhysicalControl::MouseButton(4),
    ] {
        for shared in [true, false] {
            let mut menu = client_shell::menu::MenuRuntime::new(false, 2, "Bindings".into());
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
                assert!(menu_fixture::remap_control(&mut menu, row, control));
            }
            let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(1));
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
                .insert_resource(UiRuntimeResource(runtime))
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
            let player = app.world().resource::<client_state::PlayerRuntime>();
            let runtime = app
                .world()
                .resource::<client_shell::ui_resources::UiRuntimeResource>();
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
    let mut menu = client_shell::menu::MenuRuntime::new(true, 2, "Bindings".into());
    let row = KEY_BINDINGS
        .iter()
        .position(|(action, _)| *action == Action::Hotbar1)
        .unwrap();
    assert!(menu_fixture::remap_control(
        &mut menu,
        row,
        PhysicalControl::KeyboardUsage(
            client_shell::physical_input::keyboard_usage(KeyCode::KeyR).unwrap()
        )
    ));
    assert!(inventory_consumes_key(Some(&menu), KeyCode::KeyR, false));
    assert!(!inventory_consumes_key(Some(&menu), KeyCode::KeyR, true));
    assert!(!inventory_consumes_key(Some(&menu), KeyCode::Digit1, false));
}
