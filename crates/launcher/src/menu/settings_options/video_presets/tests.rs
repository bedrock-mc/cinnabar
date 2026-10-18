use super::*;
use crate::menu::settings_options::SettingsGroup;
use std::collections::BTreeSet;

use super::SettingId;
/// Looks up the same option index used by UI edits and persistence.
fn index(name: &str) -> usize {
    SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == name)
        .unwrap()
}

/// Supplies both desktop and shared-memory recommendations to the preset resolver.
fn device(ram_gib: u64, gpu_gib: u64) -> ui::RenderDistanceDevice {
    ui::RenderDistanceDevice {
        physical_memory_bytes: ram_gib << 30,
        dedicated_graphics_memory_bytes: gpu_gib << 30,
        use_full_graphics_memory: false,
    }
}

#[test]
fn every_preset_has_one_valid_value_for_every_controlled_setting() {
    let mut names = BTreeSet::new();
    let mut columns = BTreeSet::new();
    for preset in VideoPreset::ALL {
        assert!(columns.insert(preset.index()));
    }
    assert_eq!(
        columns,
        (0..VideoPreset::ALL.len()).collect::<BTreeSet<_>>()
    );
    let settings = SettingsOptions::default();
    for option in PRESET_OPTIONS {
        assert!(
            names.insert(option.id),
            "duplicate preset option {:?}",
            option.id
        );
        let definition = SETTINGS_OPTIONS[option.index()];
        for preset in VideoPreset::ALL {
            let value = option.value(&settings, preset);
            assert!(
                (definition.min..=definition.max).contains(&value),
                "{:?}: {preset:?}",
                option.id
            );
            assert_eq!((value - definition.min) % definition.step, 0);
            let mut applied = settings.clone();
            applied.apply_video_preset(preset);
            assert_eq!(
                applied.value(option.id),
                value,
                "{:?}: {preset:?}",
                option.id
            );
        }
    }
}

#[test]
fn balanced_matches_all_defaults_and_keeps_the_device_recommendation_live() {
    for device_input in [
        ui::RenderDistanceDevice::default(),
        device(8, 0),
        device(16, 4),
        device(32, 8),
    ] {
        for counts in [
            vec![1],
            vec![1, 4],
            ui::ANTI_ALIASING_SAMPLE_COUNTS.to_vec(),
        ] {
            let mut defaults = SettingsOptions::default();
            defaults.set_render_distance_device(device_input);
            defaults.set_anti_aliasing_support(ui::AntiAliasingSupport::from_counts(counts));
            assert_eq!(defaults.video_preset(), Some(VideoPreset::Balanced));
            let mut settings = defaults.clone();
            settings.apply_video_preset(VideoPreset::Quality);
            settings.apply_video_preset(VideoPreset::Balanced);
            assert_eq!(settings, defaults);
            assert_eq!(settings.user_settings(), defaults.user_settings());
            settings.set_render_distance_device(device(8, 4));
            assert_eq!(
                settings.value(SettingId::RenderDistance),
                settings.render_distance_defaults().recommended as i32
            );
            assert_eq!(settings.video_preset(), Some(VideoPreset::Balanced));
        }
    }
}

#[test]
fn presets_respect_device_distance_and_antialiasing_limits() {
    for device in [
        ui::RenderDistanceDevice::default(),
        device(8, 0),
        device(16, 4),
        device(32, 8),
    ] {
        let mut settings = SettingsOptions::default();
        settings.set_render_distance_device(device);
        let defaults = settings.render_distance_defaults();
        settings.set_anti_aliasing_support(ui::AntiAliasingSupport::from_counts([1, 4]));
        settings.apply_video_preset(VideoPreset::Performance);
        assert_eq!(
            settings.value(SettingId::RenderDistance),
            defaults.recommended.min(12) as i32
        );
        assert_eq!(settings.user_settings().video.anti_aliasing_samples, 1);
        assert_eq!(settings.user_settings().video.smaa_mode, ui::SmaaMode::Off);
        assert_eq!(
            settings.user_settings().video.frame_rate_limit,
            render_api::FrameRateLimit::Unlimited
        );
        assert_eq!(settings.video_preset(), Some(VideoPreset::Performance));
        settings.apply_video_preset(VideoPreset::Quality);
        let distance = settings.value(SettingId::RenderDistance);
        assert!(distance >= defaults.recommended as i32);
        assert!(distance <= defaults.native_maximum as i32);
        assert!(distance <= 64);
        assert_eq!(settings.user_settings().video.anti_aliasing_samples, 4);
        assert_eq!(settings.user_settings().video.smaa_mode, ui::SmaaMode::Smaa);
        assert_eq!(settings.video_preset(), Some(VideoPreset::Quality));
    }
}

#[test]
fn editing_each_controlled_setting_derives_custom_and_restoring_it_recovers_the_preset() {
    for preset in VideoPreset::ALL {
        for option in PRESET_OPTIONS {
            let mut settings = SettingsOptions::default();
            settings.apply_video_preset(preset);
            let index = option.index();
            let original = settings.get(index);
            let definition = SETTINGS_OPTIONS[index];
            let other = if original == definition.min {
                definition.max
            } else {
                definition.min
            };
            assert!(settings.set(index, other));
            assert_eq!(settings.video_preset(), None, "{:?}: {preset:?}", option.id);
            settings.set(index, original);
            assert_eq!(settings.video_preset(), Some(preset));
        }
    }
}

#[test]
fn switching_presets_preserves_vsync() {
    for vsync in [false, true] {
        let mut settings = SettingsOptions::default();
        settings.set(SettingId::Vsync.index(), i32::from(vsync));
        for preset in VideoPreset::ALL
            .into_iter()
            .chain(VideoPreset::ALL.into_iter().rev())
        {
            settings.apply_video_preset(preset);
            assert_eq!(settings.user_settings().video.vsync, vsync, "{preset:?}");
            assert_eq!(settings.video_preset(), Some(preset));

            let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
            assert_eq!(loaded.user_settings().video.vsync, vsync, "{preset:?}");
            assert_eq!(loaded.video_preset(), Some(preset));
        }
    }
}

#[test]
fn editing_vsync_keeps_the_selected_graphics_preset() {
    for preset in VideoPreset::ALL {
        let mut settings = SettingsOptions::default();
        settings.apply_video_preset(preset);
        for vsync in [false, true] {
            settings.set(SettingId::Vsync.index(), i32::from(vsync));
            assert_eq!(settings.video_preset(), Some(preset));
        }
    }
}

#[test]
fn presets_preserve_every_uncontrolled_option_and_input_binding() {
    let mut personal = SettingsOptions::default();
    for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
        if !VideoPreset::controls(option.id) {
            personal.set(
                index,
                if option.default == option.min {
                    option.max
                } else {
                    option.min
                },
            );
        }
    }
    for preset in VideoPreset::ALL {
        let mut settings = personal.clone();
        settings.apply_video_preset(preset);
        for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
            if !VideoPreset::controls(option.id) {
                assert_eq!(settings.get(index), personal.get(index), "{:?}", option.id);
            }
        }
        assert_eq!(settings.controls().unwrap(), personal.controls().unwrap());
        assert_eq!(settings.video_preset(), Some(preset));
    }
}

#[test]
fn persistence_round_trips_presets_and_custom_without_a_preset_field() {
    let layout = crate::test_support::scratch("video-presets");
    let path = layout
        .server_file()
        .with_file_name(super::super::SETTINGS_FILE);
    for preset in VideoPreset::ALL {
        let mut settings = SettingsOptions::default();
        settings.apply_video_preset(preset);
        settings.save(&path).unwrap();
        let loaded = SettingsOptions::load(&path);
        assert_eq!(loaded, settings);
        assert_eq!(loaded.video_preset(), Some(preset));
        assert_eq!(loaded.user_settings(), settings.user_settings());
        let document: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert!(document.get("preset").is_none());
        assert!(document["values"].get("video_preset").is_none());
        settings.set(index("max_framerate"), super::super::MIN_FIXED_FRAME_RATE);
        settings.save(&path).unwrap();
        assert_eq!(SettingsOptions::load(&path).video_preset(), None);
        assert!(settings.reset_group(SettingsGroup::Video));
        assert_eq!(settings.video_preset(), Some(VideoPreset::Balanced));
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn old_files_derive_the_matching_preset_or_custom() {
    let balanced = SettingsOptions::decode(br#"{"schema":1,"values":{"gamma":60}}"#).unwrap();
    assert_eq!(balanced.video_preset(), Some(VideoPreset::Balanced));
    let performance = SettingsOptions::decode(br#"{"schema":1,"values":{"render_distance":5,"msaa":1,"vsync":0,"graphics_mode":0,"smooth_lighting":0,"fancy_skies":0,"render_clouds":0,"transparent_leaves":0,"bubble_particles":0}}"#).unwrap();
    assert_eq!(performance.video_preset(), Some(VideoPreset::Performance));
    let custom =
        SettingsOptions::decode(br#"{"schema":1,"values":{"render_distance":4}}"#).unwrap();
    assert_eq!(custom.value(SettingId::RenderDistance), 4);
    assert_eq!(custom.video_preset(), None);
}
