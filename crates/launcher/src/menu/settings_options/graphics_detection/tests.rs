use super::*;

/// Supplies ordinary memory-based render-distance limits.
fn settings() -> SettingsOptions {
    let mut settings = SettingsOptions::default();
    settings.set_render_distance_device(ui::RenderDistanceDevice {
        physical_memory_bytes: 16 << 30,
        dedicated_graphics_memory_bytes: 4 << 30,
        use_full_graphics_memory: false,
    });
    settings
}

/// Supplies known hardware at a table row's exact admission thresholds.
fn facts(row: &tiers::Tier, device_type: DeviceType) -> HardwareFacts {
    let memory = (1..=64)
        .map(|gib| (gib << 30) + 1)
        .find(|bytes| resource_pack::subpacks::device_memory_tier(*bytes) >= row.memory_tier)
        .unwrap();
    HardwareFacts {
        adapter: AdapterIdentity {
            device_type,
            ..Default::default()
        },
        graphics_memory_bytes: Some(
            match device_type {
                DeviceType::Integrated => row.integrated_memory_bytes,
                _ => row.discrete_memory_bytes,
            }
            .max(1),
        ),
        physical_memory_bytes: memory,
        logical_cores: row.logical_cores.max(1),
        physical_cores: row.physical_cores.max(1),
    }
}

#[test]
fn table_covers_every_preset_once_and_thresholds_are_monotonic() {
    assert_eq!(
        TIERS.iter().map(|row| row.preset).collect::<Vec<_>>(),
        VideoPreset::ALL
    );
    for rows in TIERS.windows(2) {
        assert!(rows[0].memory_tier <= rows[1].memory_tier);
        assert!(rows[0].logical_cores <= rows[1].logical_cores);
        assert!(rows[0].physical_cores <= rows[1].physical_cores);
        assert!(rows[0].discrete_memory_bytes <= rows[1].discrete_memory_bytes);
        assert!(rows[0].integrated_memory_bytes <= rows[1].integrated_memory_bytes);
    }
    for row in &TIERS {
        assert!(settings().video_preset_values(row.preset).next().is_some());
        for device in [DeviceType::Discrete, DeviceType::Integrated] {
            assert_eq!(facts(row, device).tier(), row.preset);
        }
    }
}

#[test]
fn ram_gpu_memory_and_both_core_counts_independently_bound_each_tier() {
    for (index, row) in TIERS.iter().enumerate().skip(1) {
        for device in [DeviceType::Discrete, DeviceType::Integrated] {
            let hardware = facts(row, device);
            for lower in [
                HardwareFacts {
                    graphics_memory_bytes: hardware.graphics_memory_bytes.map(|bytes| bytes - 1),
                    ..hardware.clone()
                },
                HardwareFacts {
                    physical_memory_bytes: hardware.physical_memory_bytes - 1,
                    ..hardware.clone()
                },
                HardwareFacts {
                    logical_cores: hardware.logical_cores - 1,
                    ..hardware.clone()
                },
                HardwareFacts {
                    physical_cores: hardware.physical_cores - 1,
                    ..hardware.clone()
                },
            ] {
                assert_eq!(lower.tier(), TIERS[index - 1].preset);
            }
        }
    }
}

#[test]
fn recommendations_preserve_the_hardware_tier_and_cap_distance_to_device_defaults() {
    let settings = settings();
    for preset in VideoPreset::ALL {
        let selected = recommend(&settings, preset);
        assert_eq!(selected.preset, preset);
        assert!(selected.render_distance <= settings.render_distance_defaults().recommended);
        assert!(selected.render_distance <= settings.render_distance_defaults().native_maximum);
    }
}

#[test]
fn unknown_hardware_uses_balanced_and_software_devices_use_performance() {
    assert_eq!(HardwareFacts::default().tier(), VideoPreset::Balanced);
    let hardware = facts(&TIERS[2], DeviceType::Discrete);
    for unknown in [
        HardwareFacts {
            graphics_memory_bytes: None,
            ..hardware.clone()
        },
        HardwareFacts {
            graphics_memory_bytes: Some(0),
            ..hardware.clone()
        },
        HardwareFacts {
            physical_memory_bytes: 0,
            ..hardware.clone()
        },
        HardwareFacts {
            logical_cores: 0,
            ..hardware.clone()
        },
        HardwareFacts {
            physical_cores: 0,
            ..hardware.clone()
        },
    ] {
        assert_eq!(unknown.tier(), VideoPreset::Balanced);
    }
    for device in [DeviceType::Cpu, DeviceType::Virtual] {
        assert_eq!(facts(&TIERS[2], device).tier(), VideoPreset::Performance);
    }
}

#[test]
fn every_explicit_edit_including_a_default_wins_after_a_rerun_and_reload() {
    for option in SETTINGS_OPTIONS
        .iter()
        .filter(|option| VideoPreset::controls(option.name))
    {
        let mut settings = settings();
        let index = SETTINGS_OPTIONS
            .iter()
            .position(|candidate| candidate.name == option.name)
            .unwrap();
        let chosen = settings.get(index);
        assert!(settings.set(index, chosen));
        settings.apply_graphics_recommendation(Recommendation {
            preset: VideoPreset::Quality,
            render_distance: 24,
        });
        assert_eq!(settings.value(option.name), chosen, "{}", option.name);
        let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
        loaded.set_render_distance_device(ui::RenderDistanceDevice {
            physical_memory_bytes: 16 << 30,
            dedicated_graphics_memory_bytes: 4 << 30,
            use_full_graphics_memory: false,
        });
        loaded.apply_graphics_recommendation(Recommendation {
            preset: VideoPreset::Performance,
            render_distance: 8,
        });
        assert_eq!(loaded.value(option.name), chosen, "{}", option.name);
    }
}

#[test]
fn automatic_defaults_round_trip_and_update_without_becoming_user_edits() {
    let mut settings = settings();
    settings.apply_graphics_recommendation(Recommendation {
        preset: VideoPreset::Quality,
        render_distance: 24,
    });
    let mut loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert_eq!(loaded.value("render_distance"), 24);
    loaded.apply_graphics_recommendation(Recommendation {
        preset: VideoPreset::Performance,
        render_distance: 8,
    });
    assert_eq!(loaded.value("render_distance"), 8);
    assert_eq!(loaded.value("msaa"), 1);
    assert_eq!(loaded.value("render_clouds"), 0);
}

#[test]
fn manual_preset_selection_protects_its_defaults_from_detection() {
    for preset in VideoPreset::ALL {
        let mut settings = settings();
        settings.apply_video_preset(preset);
        let expected = settings.user_settings();
        settings.apply_graphics_recommendation(Recommendation {
            preset: VideoPreset::Quality,
            render_distance: 8,
        });
        assert_eq!(settings.user_settings(), expected);
    }
}

#[test]
fn cache_round_trips_and_evicts_old_adapters() {
    let mut settings = settings();
    for index in 0..=CACHE_CAPACITY {
        assert!(settings.cache_graphics_tier(format!("{index:064x}"), VideoPreset::Quality));
    }
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    assert!(loaded.graphics_tier(&format!("{:064x}", 0)).is_none());
    assert_eq!(
        loaded.graphics_tier(&format!("{CACHE_CAPACITY:064x}")),
        Some(VideoPreset::Quality)
    );
    assert!(!settings.cache_graphics_tier("x".repeat(64), VideoPreset::Balanced));
    assert!(!settings.cache_graphics_tier("short".into(), VideoPreset::Balanced));
}

#[test]
fn old_measurement_cache_does_not_discard_user_values() {
    let settings = SettingsOptions::decode(
        br#"{
        "values": {"render_distance": 7},
        "graphics_detection": {"cache": [{"key": "old", "scores": {}}]}
    }"#,
    )
    .unwrap();
    assert_eq!(settings.value("render_distance"), 7);
    assert!(settings.graphics_detection.cache.is_empty());
}

#[test]
fn changed_adapter_or_driver_recomputes_defaults_and_preserves_edits() {
    let mut settings = settings();
    let mut hardware = facts(&TIERS[1], DeviceType::Discrete);
    let key = hardware.adapter.cache_key();
    settings.cache_graphics_tier(key.clone(), hardware.tier());
    settings.apply_graphics_recommendation(recommend(&settings, hardware.tier()));
    let clouds = SETTINGS_OPTIONS
        .iter()
        .position(|option| option.name == "render_clouds")
        .unwrap();
    settings.set(clouds, 0);
    for identity in [
        AdapterIdentity {
            driver_info: "new driver".into(),
            ..hardware.adapter.clone()
        },
        AdapterIdentity {
            driver_build: "new OS build".into(),
            ..hardware.adapter.clone()
        },
        AdapterIdentity {
            device: 1,
            ..hardware.adapter.clone()
        },
        AdapterIdentity {
            vendor: 1,
            ..hardware.adapter.clone()
        },
        AdapterIdentity {
            backend: "new backend".into(),
            ..hardware.adapter.clone()
        },
    ] {
        assert_ne!(identity.cache_key(), key);
    }
    hardware.adapter.driver_info = "new driver".into();
    hardware.graphics_memory_bytes = Some(TIERS[2].discrete_memory_bytes);
    hardware.physical_memory_bytes = 32 << 30;
    hardware.logical_cores = TIERS[2].logical_cores;
    hardware.physical_cores = TIERS[2].physical_cores;
    let loaded = SettingsOptions::decode(&serde_json::to_vec(&settings).unwrap()).unwrap();
    let mut loaded = loaded;
    assert!(
        loaded
            .graphics_tier(&hardware.adapter.cache_key())
            .is_none()
    );
    let tier = loaded
        .graphics_tier(&hardware.adapter.cache_key())
        .unwrap_or_else(|| hardware.tier());
    assert_eq!(tier, VideoPreset::Quality);
    loaded.apply_graphics_recommendation(recommend(&loaded, tier));
    assert_eq!(loaded.value("render_clouds"), 0);
    assert_eq!(
        loaded.value("msaa"),
        loaded
            .video_preset_values(VideoPreset::Quality)
            .find(|(name, _)| *name == "msaa")
            .unwrap()
            .1
    );
}
