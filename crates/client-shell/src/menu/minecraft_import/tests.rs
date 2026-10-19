use super::*;
use launcher::menu::settings_options::SettingId;
use launcher::menu::{MenuScreen, settings_options::SettingsOptions};

/// Reports the single queued request as undeliverable, as a disconnected worker does.
fn reject_request(menu: &mut MenuRuntime) {
    let requests = menu.take_minecraft_import_requests();
    assert_eq!(requests.len(), 1);
    menu.fail_minecraft_import(requests[0].action, "Fixture worker unavailable.");
}

/// Round-trips preferences through the restore snapshot's serialization boundary.
fn saved_state(settings: &SettingsOptions) -> UserState {
    let state = UserState {
        settings: settings.clone(),
        servers: Vec::new(),
        video: Default::default(),
        active: Vec::new(),
    };
    serde_json::from_slice(&serde_json::to_vec(&state).unwrap()).unwrap()
}

/// Delivers an import operation, including its confirmation when applying preferences.
fn perform(
    menu: &mut MenuRuntime,
    service: &mut launcher_host::minecraft_import::Service,
    library: &mut resource_pack::GlobalPackLibrary,
    mut action: Action,
) -> UserState {
    if action == Action::Apply {
        menu.activate(MenuAction::MinecraftImport(Action::Apply));
        let request = menu.take_minecraft_import_requests().remove(0);
        let outcome = service.act(request, library);
        assert_eq!(outcome.snapshot.phase, Phase::ConfirmImport);
        assert!(outcome.state.is_none());
        menu.publish_minecraft_import(outcome.snapshot, outcome.state);
        action = Action::ConfirmApply;
    }
    menu.activate(MenuAction::MinecraftImport(action));
    let mut requests = menu.take_minecraft_import_requests();
    assert_eq!(requests.len(), 1);
    let request = requests.remove(0);
    let requested_state = request.state.clone();
    let outcome = service.act(request, library);
    menu.publish_minecraft_import(outcome.snapshot, outcome.state);
    requested_state
}

#[test]
fn import_and_revert_save_preferences_while_session_overrides_stay_active() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    let path = menu
        .config_path
        .with_file_name(launcher::menu::settings_options::SETTINGS_FILE);
    let data = menu.layout.user_data_root.join("synthetic-minecraft");
    std::fs::create_dir_all(data.join("minecraftpe")).unwrap();
    std::fs::write(
        data.join("minecraftpe/options.txt"),
        b"gfx_field_of_view:80",
    )
    .unwrap();
    let mut service = launcher_host::minecraft_import::Service::new_with_data_root(
        menu.layout.user_config_root.clone(),
        data,
    );
    let mut library =
        resource_pack::GlobalPackLibrary::open(menu.layout.user_data_root.join("packs"), [0; 3])
            .unwrap();
    let original = menu.settings_options.value(SettingId::FieldOfView);
    menu.set_session_option(SettingId::HideHand, Some(1));
    perform(&mut menu, &mut service, &mut library, Action::Begin);
    assert_eq!(menu.view().minecraft_import.phase, Phase::Preview);
    let requested = perform(&mut menu, &mut service, &mut library, Action::Apply);
    assert_eq!(requested.settings.value(SettingId::HideHand), 0);
    assert_eq!(menu.settings_options.value(SettingId::HideHand), 1);
    assert_eq!(menu.settings_options.value(SettingId::FieldOfView), 80);
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    let saved = SettingsOptions::load(&path);
    assert_eq!(saved.value(SettingId::HideHand), 0);
    assert_eq!(saved.value(SettingId::FieldOfView), 80);
    let stored: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert!(stored["values"].get("hide_hand").is_none());
    perform(&mut menu, &mut service, &mut library, Action::Revert);
    perform(&mut menu, &mut service, &mut library, Action::ConfirmRevert);
    assert_eq!(menu.settings_options.value(SettingId::HideHand), 1);
    assert_eq!(
        menu.settings_options.value(SettingId::FieldOfView),
        original
    );
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    let saved = SettingsOptions::load(&path);
    assert_eq!(saved.value(SettingId::HideHand), 0);
    assert_eq!(saved.value(SettingId::FieldOfView), original);
    menu.set_session_option(SettingId::HideHand, None);
    assert_eq!(menu.settings_options.value(SettingId::HideHand), 0);
}

#[test]
fn published_preferences_replace_the_stored_choice_beneath_an_override() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    menu.set_session_option(SettingId::HideHand, Some(0));
    let imported = SettingsOptions::decode(br#"{"values":{"hide_hand":1}}"#).unwrap();
    menu.publish_minecraft_import(
        launcher::minecraft_import::Snapshot {
            phase: Phase::Result,
            ..Default::default()
        },
        Some(saved_state(&imported)),
    );
    assert_eq!(menu.settings_options.value(SettingId::HideHand), 0);
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    let saved = SettingsOptions::load(
        &menu
            .config_path
            .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
    );
    assert_eq!(saved.value(SettingId::HideHand), 1);
    menu.set_session_option(SettingId::HideHand, None);
    assert_eq!(menu.settings_options.value(SettingId::HideHand), 1);
}

#[test]
fn import_requests_preserve_unset_distance_and_uncapped_msaa_preferences() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    menu.settings_options = Arc::new(SettingsOptions::decode(br#"{"values":{"msaa":8}}"#).unwrap());
    let device = ui::RenderDistanceDevice {
        physical_memory_bytes: 8 << 30,
        dedicated_graphics_memory_bytes: 4 << 30,
        use_full_graphics_memory: false,
    };
    menu.sync_render_distance_device(device);
    menu.sync_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4]));
    menu.set_session_option(SettingId::RenderDistance, Some(50));
    menu.set_session_option(SettingId::Msaa, Some(1));
    menu.activate(MenuAction::MinecraftImport(Action::Begin));
    let mut requests = menu.take_minecraft_import_requests();
    assert_eq!(requests.len(), 1);
    let state = requests.remove(0).state;
    let stored = serde_json::to_value(&state.settings).unwrap();
    assert!(stored["values"].get("render_distance").is_none());
    assert_eq!(stored["values"]["msaa"], 8);
    menu.publish_minecraft_import(
        launcher::minecraft_import::Snapshot {
            phase: Phase::Result,
            ..Default::default()
        },
        Some(state),
    );
    assert_eq!(menu.settings_options.value(SettingId::RenderDistance), 50);
    assert_eq!(menu.settings_options.value(SettingId::Msaa), 1);
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    let stored: serde_json::Value = serde_json::from_slice(
        &std::fs::read(
            menu.config_path
                .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(stored["values"].get("render_distance").is_none());
    assert_eq!(stored["values"]["msaa"], 8);
    let device = ui::RenderDistanceDevice {
        physical_memory_bytes: 16 << 30,
        ..device
    };
    menu.sync_render_distance_device(device);
    menu.set_session_option(SettingId::RenderDistance, None);
    assert_eq!(
        menu.settings_options.value(SettingId::RenderDistance),
        i32::from(device.defaults().recommended)
    );
    menu.set_session_option(SettingId::Msaa, None);
    assert_eq!(menu.settings_options.value(SettingId::Msaa), 4);
}

#[test]
fn unavailable_worker_errors_allow_close_escape_and_generic_dismissal() {
    for dismissal in 0..3 {
        let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
        menu.activate(MenuAction::Navigate(MenuScreen::Settings));
        menu.activate(MenuAction::MinecraftImport(Action::Begin));
        reject_request(&mut menu);
        assert_eq!(menu.view().dialog, Some(MenuDialog::MinecraftImport));
        assert_eq!(menu.view().minecraft_import.phase, Phase::Result);
        assert_eq!(
            menu.view().minecraft_import.body,
            "Fixture worker unavailable."
        );
        match dismissal {
            0 => menu.activate(MenuAction::MinecraftImport(Action::Close)),
            1 => menu.go_back(),
            _ => menu.activate(MenuAction::DismissDialog),
        }
        reject_request(&mut menu);
        assert_eq!(menu.view().dialog, None);
        assert_eq!(menu.view().minecraft_import.phase, Phase::Idle);
        assert_eq!(menu.screen(), MenuScreen::Settings);
        assert!(menu.take_minecraft_import_requests().is_empty());
        menu.activate(MenuAction::Navigate(MenuScreen::Home));
        assert_eq!(menu.screen(), MenuScreen::Home);
    }
}

#[test]
fn an_available_worker_still_receives_close_to_release_its_preview() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    menu.publish_minecraft_import(
        launcher::minecraft_import::Snapshot {
            phase: Phase::Preview,
            ..Default::default()
        },
        None,
    );
    menu.activate(MenuAction::MinecraftImport(Action::Close));
    let requests = menu.take_minecraft_import_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].action, Action::Close);
    menu.publish_minecraft_import(launcher::minecraft_import::Snapshot::default(), None);
    assert_eq!(menu.view().dialog, None);
    assert_eq!(menu.view().minecraft_import.phase, Phase::Idle);
}

#[test]
fn restored_untouched_render_distance_uses_the_current_device_recommendation() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    let old_device = ui::RenderDistanceDevice {
        physical_memory_bytes: 8 << 30,
        dedicated_graphics_memory_bytes: 4 << 30,
        use_full_graphics_memory: false,
    };
    menu.sync_render_distance_device(old_device);
    let backup = saved_state(&menu.settings_options);
    assert_ne!(
        backup.settings.render_distance_defaults(),
        old_device.defaults()
    );
    let current_device = ui::RenderDistanceDevice {
        physical_memory_bytes: 16 << 30,
        ..old_device
    };
    menu.sync_render_distance_device(current_device);
    let support = ui::AntiAliasingSupport::from_counts([1, 4]);
    menu.sync_anti_aliasing_support(support);
    menu.set_named_option(
        SettingId::RenderDistance,
        i32::from(ui::MIN_RENDER_DISTANCE_CHUNKS),
    );
    menu.publish_minecraft_import(
        launcher::minecraft_import::Snapshot {
            phase: Phase::Result,
            ..Default::default()
        },
        Some(backup),
    );
    let settings = &menu.view().settings_options;
    assert_eq!(
        settings.render_distance_defaults(),
        current_device.defaults()
    );
    assert_eq!(
        settings.user_settings().video.render_distance_chunks,
        current_device.defaults().recommended
    );
    assert_eq!(settings.anti_aliasing_support(), support);
    let stored = serde_json::to_value(settings.as_ref()).unwrap();
    assert!(stored["values"].get("render_distance").is_none());
    assert!(menu.settings_apply && menu.settings_dirty);
}

#[test]
fn restored_resolution_retains_the_current_device_default_and_any_saved_choice() {
    use launcher::menu::settings_options::render_resolution::RASPBERRY_PI_DEFAULT_PERCENT;

    for choice in [None, Some(RASPBERRY_PI_DEFAULT_PERCENT), Some(37)] {
        let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
        menu.sync_render_resolution_default(RASPBERRY_PI_DEFAULT_PERCENT);
        let mut settings = SettingsOptions::default();
        if let Some(percent) = choice {
            settings.set(SettingId::RenderResolution.index(), i32::from(percent));
        }
        let backup = saved_state(&settings);
        assert_eq!(
            backup.settings.render_resolution_default(),
            ui::DEFAULT_RENDER_SCALE_PERCENT
        );
        menu.publish_minecraft_import(
            launcher::minecraft_import::Snapshot {
                phase: Phase::Result,
                ..Default::default()
            },
            Some(backup),
        );
        let published = &menu.view().settings_options;
        assert_eq!(
            published.render_resolution_default(),
            RASPBERRY_PI_DEFAULT_PERCENT
        );
        assert_eq!(
            published.user_settings().video.render_scale_percent,
            choice.unwrap_or(RASPBERRY_PI_DEFAULT_PERCENT)
        );
        let stored = serde_json::to_value(published.as_ref()).unwrap();
        assert_eq!(
            stored["values"]
                .get("render_resolution")
                .and_then(serde_json::Value::as_u64),
            choice.map(u64::from)
        );
        assert!(menu.settings_apply && menu.settings_dirty);
    }
}

#[test]
fn published_explicit_render_distance_keeps_its_choice_and_device_defaults() {
    let device = ui::RenderDistanceDevice {
        physical_memory_bytes: 16 << 30,
        dedicated_graphics_memory_bytes: 4 << 30,
        use_full_graphics_memory: false,
    };
    for distance in [
        4,
        i32::from(ui::MIN_RENDER_DISTANCE_CHUNKS),
        i32::from(device.defaults().recommended),
    ] {
        let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
        menu.sync_render_distance_device(device);
        let settings = SettingsOptions::decode(
            format!(r#"{{"values":{{"render_distance":{distance}}}}}"#).as_bytes(),
        )
        .unwrap();
        menu.publish_minecraft_import(
            launcher::minecraft_import::Snapshot {
                phase: Phase::Result,
                ..Default::default()
            },
            Some(saved_state(&settings)),
        );
        let published = &menu.view().settings_options;
        assert_eq!(published.render_distance_defaults(), device.defaults());
        assert_eq!(published.value(SettingId::RenderDistance), distance);
        let stored = serde_json::to_value(published.as_ref()).unwrap();
        assert_eq!(stored["values"]["render_distance"], distance);
    }
}

#[test]
fn servers_only_import_and_revert_preserve_fixed_gui_scale_and_saved_offset() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    menu.gui_scale_offset = -1;
    menu.set_gui_scale_preference(Some(3));
    let data = menu.layout.user_data_root.join("synthetic-minecraft");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(
        data.join("external_servers.txt"),
        b"1:Fixture:example.test:19132:0",
    )
    .unwrap();
    let mut service = launcher_host::minecraft_import::Service::new_with_data_root(
        menu.layout.user_config_root.clone(),
        data,
    );
    let mut library =
        resource_pack::GlobalPackLibrary::open(menu.layout.user_data_root.join("packs"), [0; 3])
            .unwrap();
    perform(&mut menu, &mut service, &mut library, Action::Begin);
    let requested = perform(&mut menu, &mut service, &mut library, Action::Apply);
    assert_eq!(requested.video.gui_scale_offset, -1);
    assert_eq!(menu.servers.len(), 1);
    assert_eq!(menu.gui_scale_preference(), Some(3));
    assert_eq!(menu.gui_scale_offset(), -1);
    menu.gui_scale_offset = 0;
    perform(&mut menu, &mut service, &mut library, Action::Revert);
    perform(&mut menu, &mut service, &mut library, Action::ConfirmRevert);
    assert!(menu.servers.is_empty());
    assert_eq!(menu.gui_scale_preference(), Some(3));
    assert_eq!(menu.gui_scale_offset(), -1);
}

#[test]
fn imported_preferences_preserve_uncapped_session_msaa_choice() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    menu.sync_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4, 8]));
    menu.set_session_option(SettingId::Msaa, Some(8));
    menu.sync_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4]));
    assert_eq!(menu.settings_options.value(SettingId::Msaa), 4);
    menu.publish_minecraft_import(
        launcher::minecraft_import::Snapshot {
            phase: Phase::Result,
            ..Default::default()
        },
        Some(saved_state(&SettingsOptions::default())),
    );
    assert_eq!(menu.settings_options.value(SettingId::Msaa), 4);
    menu.sync_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4, 8]));
    assert_eq!(menu.settings_options.value(SettingId::Msaa), 8);
    assert!(
        serde_json::to_value(menu.persistent_settings().as_ref()).unwrap()["values"]
            .get("msaa")
            .is_none()
    );
}

#[test]
fn saving_the_active_session_choice_after_import_replaces_its_underlying_preference() {
    let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
    menu.set_session_option(SettingId::HideHand, Some(1));
    menu.publish_minecraft_import(
        launcher::minecraft_import::Snapshot {
            phase: Phase::Result,
            ..Default::default()
        },
        Some(saved_state(&SettingsOptions::default())),
    );
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    assert!(!menu.settings_dirty);
    menu.set_named_option(SettingId::HideHand, 1);
    menu.sync_user_settings(None);
    menu.flush_user_settings().unwrap();
    let saved = SettingsOptions::load(
        &menu
            .config_path
            .with_file_name(launcher::menu::settings_options::SETTINGS_FILE),
    );
    assert_eq!(saved.value(SettingId::HideHand), 1);
    menu.set_session_option(SettingId::HideHand, None);
    assert_eq!(menu.settings_options.value(SettingId::HideHand), 1);
}

#[test]
fn escape_and_generic_dismissal_cancel_confirmation_without_applying() {
    for escape in [true, false] {
        let mut menu = MenuRuntime::new(true, 2, "Import fixture".into());
        let data = menu.layout.user_data_root.join("synthetic-minecraft");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::write(data.join("options.txt"), b"gfx_field_of_view:80").unwrap();
        let mut service = launcher_host::minecraft_import::Service::new_with_data_root(
            menu.layout.user_config_root.clone(),
            data,
        );
        let mut library = resource_pack::GlobalPackLibrary::open(
            menu.layout.user_data_root.join("packs"),
            [0; 3],
        )
        .unwrap();
        perform(&mut menu, &mut service, &mut library, Action::Begin);
        let preview = menu.view().minecraft_import.body.clone();
        let original = menu.settings_options.value(SettingId::FieldOfView);
        menu.activate(MenuAction::MinecraftImport(Action::Apply));
        let request = menu.take_minecraft_import_requests().remove(0);
        let outcome = service.act(request, &mut library);
        assert!(outcome.state.is_none());
        menu.publish_minecraft_import(outcome.snapshot, outcome.state);
        assert_eq!(menu.view().minecraft_import.phase, Phase::ConfirmImport);
        if escape {
            menu.go_back();
        } else {
            menu.activate(MenuAction::DismissDialog);
        }
        let request = menu.take_minecraft_import_requests().remove(0);
        assert_eq!(request.action, Action::Close);
        let outcome = service.act(request, &mut library);
        assert!(outcome.state.is_none());
        menu.publish_minecraft_import(outcome.snapshot, outcome.state);
        assert_eq!(menu.view().minecraft_import.phase, Phase::Preview);
        assert_eq!(menu.view().minecraft_import.body, preview);
        assert_eq!(
            menu.settings_options.value(SettingId::FieldOfView),
            original
        );
    }
}
