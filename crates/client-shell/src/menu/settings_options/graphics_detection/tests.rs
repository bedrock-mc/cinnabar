use super::*;
use launcher::menu::{
    MenuAction, MenuScreen, settings_options::graphics_detection::DeviceType,
    settings_options::video_presets::VideoPreset,
};

/// Supplies known hardware at the highest thresholds with ordinary device distance limits.
fn hardware() -> HardwareFacts {
    HardwareFacts {
        adapter: graphics_detection::AdapterIdentity {
            device_type: DeviceType::Discrete,
            ..Default::default()
        },
        graphics_memory_bytes: Some(graphics_detection::TIERS[2].discrete_memory_bytes),
        physical_memory_bytes: 32 << 30,
        logical_cores: graphics_detection::TIERS[2].logical_cores,
        physical_cores: graphics_detection::TIERS[2].physical_cores,
    }
}

/// Creates a launcher using the same memory defaults as startup hardware sampling.
fn menu() -> MenuRuntime {
    let mut menu = MenuRuntime::new(true, 2, "Graphics test".into());
    menu.sync_render_distance_device(ui::RenderDistanceDevice {
        physical_memory_bytes: hardware().physical_memory_bytes,
        dedicated_graphics_memory_bytes: hardware().graphics_memory_bytes.unwrap(),
        use_full_graphics_memory: false,
    });
    menu
}

#[test]
fn startup_applies_once_and_explicit_detection_finishes_in_the_same_update() {
    let mut app = App::new();
    app.insert_resource(menu())
        .insert_resource(GraphicsHardware(hardware()))
        .init_resource::<Detection>()
        .add_systems(Update, drive);
    app.update();
    let settings = app
        .world()
        .resource::<MenuRuntime>()
        .settings_options
        .clone();
    assert_eq!(
        settings.graphics_tier(&hardware().adapter.cache_key()),
        Some(VideoPreset::Quality)
    );
    assert!(app.world().resource::<MenuRuntime>().settings_apply);
    // Changing the supplied facts after initialization does not sample again each frame.
    app.world_mut()
        .resource_mut::<GraphicsHardware>()
        .0
        .graphics_memory_bytes = Some(graphics_detection::TIERS[1].discrete_memory_bytes);
    app.update();
    assert_eq!(
        *app.world().resource::<MenuRuntime>().settings_options,
        *settings
    );
    app.world_mut()
        .resource_mut::<MenuRuntime>()
        .activate(MenuAction::SettingsDetectGraphics);
    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert!(!menu.graphics_detection_requested);
    assert_eq!(
        menu.settings_options.value("msaa"),
        menu.settings_options
            .video_preset_values(VideoPreset::Balanced)
            .find(|(name, _)| *name == "msaa")
            .unwrap()
            .1
    );
    assert_eq!(
        app.world().resource::<Detection>().notice,
        Some(graphics_detection::TUNED_MESSAGE)
    );
}

#[test]
fn cached_and_explicit_detection_replace_lower_defaults_with_the_hardware_tier() {
    let hardware = hardware();
    let key = hardware.adapter.cache_key();
    let tier = hardware.tier();
    for explicit in [false, true] {
        let mut menu = menu();
        let lower = graphics_detection::recommend(&menu.settings_options, VideoPreset::Balanced);
        let settings = Arc::make_mut(&mut menu.settings_options);
        settings.cache_graphics_tier(key.clone(), tier);
        settings.apply_graphics_recommendation(lower);

        assert!(menu.detect_graphics(&hardware, explicit));
        assert_eq!(menu.settings_options.graphics_tier(&key), Some(tier));
        let expected = graphics_detection::recommend(&menu.settings_options, tier);
        for (name, value) in menu.settings_options.video_preset_values(tier) {
            let expected = if name == "render_distance" {
                i32::from(expected.render_distance)
            } else {
                value
            };
            assert_eq!(menu.settings_options.value(name), expected, "{name}");
        }
        assert!(menu.settings_apply);
    }
}

#[test]
fn detection_can_apply_in_world_settings_without_replacing_manual_values() {
    let mut menu = menu();
    menu.show_world();
    menu.open_pause();
    menu.activate(MenuAction::Navigate(MenuScreen::Settings));
    menu.set_named_option("render_distance", 7);
    menu.set_named_option("render_clouds", 0);
    menu.activate(MenuAction::SettingsDetectGraphics);
    let mut app = App::new();
    app.insert_resource(menu)
        .insert_resource(GraphicsHardware(hardware()))
        .init_resource::<Detection>()
        .add_systems(Update, drive);
    app.update();
    let menu = app.world().resource::<MenuRuntime>();
    assert_eq!(menu.settings_options.value("render_distance"), 7);
    assert_eq!(menu.settings_options.value("render_clouds"), 0);
    assert!(!menu.graphics_detection_requested);
    assert!(
        menu.settings_options
            .graphics_tier(&hardware().adapter.cache_key())
            .is_some()
    );
}

#[test]
fn driver_change_on_a_new_launch_redetects_and_explicit_requests_refresh_the_cache() {
    let mut menu = menu();
    let mut hardware = hardware();
    menu.detect_graphics(&hardware, false);
    hardware.graphics_memory_bytes = Some(1 << 30);
    // The same adapter and driver retain their cached tier.
    menu.detect_graphics(&hardware, false);
    assert_eq!(
        menu.settings_options
            .graphics_tier(&hardware.adapter.cache_key()),
        Some(VideoPreset::Quality)
    );
    hardware.adapter.driver_info = "updated driver".into();
    menu.detect_graphics(&hardware, false);
    assert_eq!(
        menu.settings_options
            .graphics_tier(&hardware.adapter.cache_key()),
        Some(VideoPreset::Performance)
    );
    hardware.graphics_memory_bytes = Some(graphics_detection::TIERS[2].discrete_memory_bytes);
    menu.detect_graphics(&hardware, true);
    assert_eq!(
        menu.settings_options
            .graphics_tier(&hardware.adapter.cache_key()),
        Some(VideoPreset::Quality)
    );
}

#[test]
fn a_request_waits_for_startup_facts_instead_of_being_lost() {
    let mut app = App::new();
    let mut menu = menu();
    menu.activate(MenuAction::SettingsDetectGraphics);
    app.insert_resource(menu)
        .init_resource::<Detection>()
        .add_systems(Update, drive);
    app.update();
    assert!(
        app.world()
            .resource::<MenuRuntime>()
            .graphics_detection_requested
    );
    app.insert_resource(GraphicsHardware(hardware()));
    app.update();
    assert!(
        !app.world()
            .resource::<MenuRuntime>()
            .graphics_detection_requested
    );
}
