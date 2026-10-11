use super::atmosphere::{
    BossEnvironmentState, apply_boss_environment, derive_atmosphere_frame,
    derive_atmosphere_frame_for_medium, derive_boss_environment, derive_profiled_atmosphere_frame,
};
use super::{
    EnvironmentContext, WeatherState, WorldClock, apply_environment_control,
    bind_session_generation, replace_session, visual_world_time,
};
use assets::{BiomeVisualProfile, FogDistance, FogDistanceMode, FogMedium, FogProfile};
use client_world::CommittedControlEvent;
use protocol::{
    ChangeDimensionEvent, DaylightCycleUpdateEvent, SetTimeEvent, WeatherChannel,
    WeatherUpdateEvent, WorldEnvironmentBootstrap,
};
use std::sync::Arc;
use ui::{BossBarView, BossColor, BossOverlay, BossStyle};

fn boss_bar_view(darken_sky: Option<bool>, create_world_fog: Option<bool>) -> BossBarView {
    BossBarView {
        target_entity_id: 1,
        title: Arc::from("boss"),
        filtered_title: Arc::from(""),
        health: 1.0,
        style: BossStyle {
            color: BossColor::Red,
            overlay: BossOverlay::Notched10,
            darken_sky,
            create_world_fog,
        },
    }
}

#[test]
fn boss_environment_requires_an_explicit_true_request() {
    assert_eq!(
        derive_boss_environment(&[]),
        BossEnvironmentState::default()
    );
    for flag_darken in [None, Some(false)] {
        for flag_fog in [None, Some(false)] {
            assert_eq!(
                derive_boss_environment(&[boss_bar_view(flag_darken, flag_fog)]),
                BossEnvironmentState::default(),
                "flags {flag_darken:?}/{flag_fog:?} must stay inert"
            );
        }
    }
    assert!(derive_boss_environment(&[boss_bar_view(Some(true), None)]).darken_sky);
    assert!(!derive_boss_environment(&[boss_bar_view(Some(true), None)]).world_fog);
    assert!(derive_boss_environment(&[boss_bar_view(None, Some(true))]).world_fog);
    let both = derive_boss_environment(&[
        boss_bar_view(None, None),
        boss_bar_view(Some(false), Some(false)),
        boss_bar_view(Some(true), Some(true)),
    ]);
    assert!(both.darken_sky && both.world_fog);
}

#[test]
fn boss_effects_apply_only_in_air_and_removal_restores_the_exact_frame() {
    use meshing::CameraMedium;

    let clock = WorldClock::default();
    let weather = WeatherState::default();
    let baseline = derive_atmosphere_frame_for_medium(clock, weather, 10.0, CameraMedium::Air);
    let state = BossEnvironmentState {
        darken_sky: true,
        world_fog: true,
    };
    let darkened_air = apply_boss_environment(baseline, CameraMedium::Air, state);
    assert_ne!(darkened_air, baseline);

    // Water and lava media keep their complete medium fog.
    let water = derive_atmosphere_frame_for_medium(clock, weather, 10.0, CameraMedium::Water);
    assert_eq!(
        apply_boss_environment(water, CameraMedium::Water, state),
        water
    );

    // Removal (no active bars) means the next derived frame matches the
    // exact baseline again; modifiers are never stacked across frames.
    let cleared = derive_boss_environment(&[]);
    let fresh = derive_atmosphere_frame_for_medium(clock, weather, 10.0, CameraMedium::Air);
    assert_eq!(
        apply_boss_environment(fresh, CameraMedium::Air, cleared),
        baseline
    );
}

fn bootstrap(
    initial_time: i64,
    day_cycle_lock_time: i32,
    daylight_cycle_enabled: bool,
    rain_level: f32,
    lightning_level: f32,
) -> WorldEnvironmentBootstrap {
    WorldEnvironmentBootstrap {
        initial_time,
        day_cycle_lock_time,
        daylight_cycle_enabled,
        weather_cycle_enabled: true,
        rain_level,
        lightning_level,
    }
}

fn set_time(clock: &mut WorldClock, weather: &mut WeatherState, time: i32, elapsed: f64) {
    assert!(apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 1,
            update: SetTimeEvent { time },
        },
        clock,
        weather,
        elapsed,
    ));
}

#[test]
fn start_game_replacement_resets_time_and_replaces_exact_environment_snapshot() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();

    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 0, true, 0.25, 0.75),
        10.0,
    );
    assert_eq!(clock.session_generation(), 1);
    assert_eq!(clock.server_time(), Some(0.0));
    assert!(clock.daylight_cycle_enabled());
    assert_eq!(visual_world_time(clock, 10.0), 0.0);
    assert_eq!(visual_world_time(clock, 12.5), 50.0);
    set_time(&mut clock, &mut weather, 6_000, 10.0);
    assert_eq!(
        derive_atmosphere_frame(clock, weather, 10.0).sun_direction(),
        [0.0, 1.0, 0.0],
        "the authoritative noon packet, not elapsed world age, places the sun overhead"
    );
    assert_eq!(weather.session_generation(), 1);
    assert_eq!(weather.rain_level(), 0.25);
    assert_eq!(weather.lightning_level(), 0.75);

    assert!(apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 7,
            update: SetTimeEvent { time: i32::MIN },
        },
        &mut clock,
        &mut weather,
        0.0,
    ));
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(12_000, i32::MAX, false, 1.0, 0.0),
        20.0,
    );

    assert_eq!(clock.session_generation(), 2);
    assert_eq!(
        clock.server_time(),
        Some(f64::from(i32::MAX)),
        "a disabled new StartGame anchors its explicit lock tick"
    );
    assert!(!clock.daylight_cycle_enabled());
    assert_eq!(clock.last_update_sequence(), None);
    assert_eq!(weather.session_generation(), 2);
    assert_eq!(weather.rain_level(), 1.0);
    assert_eq!(weather.lightning_level(), 0.0);
    assert_eq!(weather.last_update_sequence(), None);

    bind_session_generation(&mut clock, &mut weather, 8);
    assert_eq!(clock.session_generation(), 8);
    assert_eq!(weather.session_generation(), 8);
}
#[test]
fn committed_updates_preserve_signed_time_channel_targets_and_order() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(1_000, 18_000, false, 0.0, 0.0),
        0.0,
    );

    for control in [
        CommittedControlEvent::Weather {
            sequence: 11,
            update: WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 1.0,
            },
        },
        CommittedControlEvent::SetTime {
            sequence: 12,
            update: SetTimeEvent { time: -24_001 },
        },
        CommittedControlEvent::Weather {
            sequence: 13,
            update: WeatherUpdateEvent {
                channel: WeatherChannel::Lightning,
                level: 0.75,
            },
        },
        CommittedControlEvent::Weather {
            sequence: 14,
            update: WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 0.25,
            },
        },
    ] {
        assert!(apply_environment_control(
            control,
            &mut clock,
            &mut weather,
            0.0,
        ));
    }

    assert_eq!(clock.server_time(), Some(-24_001.0));
    assert_eq!(clock.last_update_sequence(), Some(12));
    assert_eq!(weather.rain_level(), 0.25);
    assert_eq!(weather.lightning_level(), 0.75);
    assert_eq!(weather.last_update_sequence(), Some(14));
}
#[test]
fn dimension_change_is_not_an_environment_session_replacement() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 6_000, false, 0.5, 0.75),
        0.0,
    );
    let before_clock = clock;
    let before_weather = weather;

    assert!(!apply_environment_control(
        CommittedControlEvent::ChangeDimension {
            sequence: 1,
            change: ChangeDimensionEvent {
                dimension: 1,
                position: [0.0, 64.0, 0.0],
                ..Default::default()
            },
            resolved: client_world::ResolvedServerPosition {
                position: [0.0, 64.0, 0.0],
                surface_anchor: None,
            },
        },
        &mut clock,
        &mut weather,
        0.0,
    ));

    assert_eq!(clock, before_clock);
    assert_eq!(weather, before_weather);
}
#[test]
fn running_clock_anchors_each_set_time_and_advances_at_twenty_ticks_per_second() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 0, true, 0.0, 0.0),
        10.0,
    );
    assert_eq!(visual_world_time(clock, 12.5), 50.0);

    assert!(apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 1,
            update: SetTimeEvent { time: 6_000 },
        },
        &mut clock,
        &mut weather,
        10.0,
    ));
    assert_eq!(visual_world_time(clock, 12.5), 6_050.0);

    assert!(apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 2,
            update: SetTimeEvent { time: 12_000 },
        },
        &mut clock,
        &mut weather,
        20.0,
    ));
    assert_eq!(visual_world_time(clock, 20.5), 12_010.0);
    assert_eq!(clock.server_time(), Some(12_000.0));
    assert_eq!(clock.last_update_sequence(), Some(2));
}
#[test]
fn stopped_clock_set_time_replaces_frozen_tick_and_signed_times_use_euclidean_days() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 18_000, false, 0.0, 0.0),
        0.0,
    );
    assert!(apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 3,
            update: SetTimeEvent { time: i32::MIN },
        },
        &mut clock,
        &mut weather,
        5.0,
    ));
    assert_eq!(visual_world_time(clock, 10_000.0), f64::from(i32::MIN));

    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(-1, 0, true, 0.0, 0.0),
        7.0,
    );
    assert!(apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 4,
            update: SetTimeEvent { time: -1 },
        },
        &mut clock,
        &mut weather,
        7.0,
    ));
    let frame = derive_atmosphere_frame(clock, weather, 7.0);
    assert!((frame.day_fraction() - (23_999.0 / 24_000.0)).abs() < 1.0e-6);
    assert_eq!(frame.moon_phase(), 7);
}
#[test]
fn daylight_cycle_changes_freeze_current_tick_and_resume_from_that_anchor() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 0, true, 0.0, 0.0),
        10.0,
    );
    set_time(&mut clock, &mut weather, 6_000, 10.0);
    assert_eq!(visual_world_time(clock, 12.5), 6_050.0);

    assert!(apply_environment_control(
        CommittedControlEvent::DaylightCycle {
            sequence: 10,
            update: DaylightCycleUpdateEvent { enabled: false },
        },
        &mut clock,
        &mut weather,
        12.5,
    ));
    assert_eq!(visual_world_time(clock, 100.0), 6_050.0);

    assert!(apply_environment_control(
        CommittedControlEvent::DaylightCycle {
            sequence: 11,
            update: DaylightCycleUpdateEvent { enabled: true },
        },
        &mut clock,
        &mut weather,
        100.0,
    ));
    assert_eq!(visual_world_time(clock, 101.0), 6_070.0);
    assert_eq!(clock.last_update_sequence(), Some(11));
}
#[test]
fn cardinal_bedrock_times_drive_exact_sun_quadrants_and_moon_phases() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(0, 0, true, 0.0, 0.0),
        100.0,
    );

    for (time, expected) in [(6_000, [0.0, 1.0, 0.0]), (18_000, [0.0, -1.0, 0.0])] {
        assert!(apply_environment_control(
            CommittedControlEvent::SetTime {
                sequence: time as u64 + 1,
                update: SetTimeEvent { time },
            },
            &mut clock,
            &mut weather,
            100.0,
        ));
        let actual = derive_atmosphere_frame(clock, weather, 100.0).sun_direction();
        for axis in 0..3 {
            assert!((actual[axis] - expected[axis]).abs() < 1.0e-6);
        }
    }

    // The eased angle keeps the sun a little above the horizon at both day boundaries.
    for (time, east) in [(0, true), (12_000, false)] {
        assert!(apply_environment_control(
            CommittedControlEvent::SetTime {
                sequence: time as u64 + 100,
                update: SetTimeEvent { time },
            },
            &mut clock,
            &mut weather,
            100.0,
        ));
        let sun = derive_atmosphere_frame(clock, weather, 100.0).sun_direction();
        assert!(sun[1] > 0.1 && sun[1] < 0.3, "{sun:?}");
        assert_eq!(sun[0] > 0.0, east, "{sun:?}");
    }

    for day in 0..8 {
        assert!(apply_environment_control(
            CommittedControlEvent::SetTime {
                sequence: 30_000 + day as u64,
                update: SetTimeEvent {
                    time: day * 24_000 + 6_000,
                },
            },
            &mut clock,
            &mut weather,
            100.0,
        ));
        assert_eq!(
            derive_atmosphere_frame(clock, weather, 100.0).moon_phase(),
            day as u8
        );
    }
}
#[test]
fn atmosphere_bounds_weather_and_session_replacement_resets_the_named_clock() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 0, true, 0.25, 0.75),
        1.0,
    );
    assert!(apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 1,
            update: SetTimeEvent { time: 6_000 },
        },
        &mut clock,
        &mut weather,
        1.0,
    ));
    let frame = derive_atmosphere_frame(clock, weather, 2.0);
    assert_eq!(frame.rain_level(), 0.25);
    assert_eq!(frame.thunder_level(), 0.75);
    assert!(frame.fog_start() >= 0.0);
    assert!(frame.fog_end() > frame.fog_start());

    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(12_000, 0, true, 1.0, 0.0),
        50_000.0,
    );
    assert_eq!(clock.server_time(), Some(0.0));
    assert_eq!(visual_world_time(clock, 50_000.0), 0.0);
    assert_eq!(
        derive_atmosphere_frame(clock, weather, 50_000.0).moon_phase(),
        0
    );
}
#[test]
fn active_camera_medium_is_applied_after_clock_and_weather_derivation() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 0, true, 0.25, 0.75),
        10.0,
    );
    set_time(&mut clock, &mut weather, 6_000, 10.0);

    let frame =
        derive_atmosphere_frame_for_medium(clock, weather, 10.0, meshing::CameraMedium::Water);

    assert_eq!(frame.camera_medium(), meshing::CameraMedium::Water);
    assert_eq!(frame.rain_level(), 0.25);
    assert_eq!(frame.thunder_level(), 0.75);
    assert_eq!(frame.sun_direction(), [0.0, 1.0, 0.0]);
}
#[test]
fn end_dimension_fallback_applies_exact_profile_and_exposes_provisional_lighting_route() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(18_000, 0, true, 0.0, 0.0),
        10.0,
    );
    set_time(&mut clock, &mut weather, 18_000, 10.0);
    let context = EnvironmentContext {
        dimension: 2,
        fog_biomes: None,
        camera_profile: None,
        camera_fog: None,
        profile_route: None,
        default_fog: None,
        precipitation_sample_count: None,
        camera_biome_identifier: None,
        camera_biome_temperature: None,
        render_distance_blocks: Some(256.0),
    };
    let biomes = profiles();
    let fogs = fog_profiles();

    let (frame, route) = derive_profiled_atmosphere_frame(
        clock,
        weather,
        10.0,
        meshing::CameraMedium::Air,
        &context,
        &biomes,
        &fogs,
        None,
        0.0,
    );

    assert_eq!(frame.sky_zenith(), [0.0; 3]);
    assert_eq!(frame.sky_horizon(), frame.fog_color());
    assert_eq!(frame.sky_kind(), render::SkyKind::End);
    assert_eq!(frame.fog_end(), 256.0);
    assert_eq!(frame.rain_level(), 0.0);
    assert_eq!(frame.thunder_level(), 0.0);
    assert_eq!(frame.sun_direction(), [0.0, -1.0, 0.0]);
    assert_eq!(route.biome_identifier.as_deref(), Some("minecraft:the_end"));
    assert_eq!(
        route.atmosphere_identifier.as_deref(),
        Some("minecraft:end_atmospherics")
    );
    assert_eq!(
        route.provisional_lighting_identifier.as_deref(),
        Some("minecraft:end_lighting")
    );
}
#[test]
fn known_camera_biome_takes_precedence_over_dimension_fallback() {
    let context = EnvironmentContext {
        dimension: 1,
        fog_biomes: None,
        camera_profile: None,
        camera_fog: None,
        profile_route: None,
        default_fog: None,
        precipitation_sample_count: None,
        camera_biome_identifier: Some("minecraft:plains".into()),
        camera_biome_temperature: None,
        render_distance_blocks: Some(256.0),
    };
    let (frame, route) = derive_profiled_atmosphere_frame(
        WorldClock::default(),
        WeatherState::default(),
        0.0,
        meshing::CameraMedium::Air,
        &context,
        &profiles(),
        &fog_profiles(),
        None,
        0.0,
    );
    assert_eq!(route.biome_identifier.as_deref(), Some("minecraft:plains"));
    assert_eq!(frame.fog_end(), 256.0);
}
#[test]
fn pinned_shape_plains_air_falls_back_to_the_exact_default_fog_layer() {
    let context = EnvironmentContext {
        dimension: 0,
        fog_biomes: None,
        camera_profile: None,
        camera_fog: None,
        profile_route: None,
        default_fog: None,
        precipitation_sample_count: None,
        camera_biome_identifier: Some("minecraft:plains".into()),
        camera_biome_temperature: None,
        render_distance_blocks: Some(256.0),
    };
    let (frame, _) = derive_profiled_atmosphere_frame(
        WorldClock::default(),
        WeatherState::default(),
        0.0,
        meshing::CameraMedium::Air,
        &context,
        &profiles(),
        &fog_profiles(),
        None,
        0.0,
    );
    assert_eq!(frame.fog_start(), 235.52);
    assert_eq!(frame.fog_end(), 256.0);
}
#[test]
fn biome_specific_medium_takes_precedence_over_the_default_fog_layer() {
    let context = EnvironmentContext {
        dimension: 0,
        fog_biomes: None,
        camera_profile: None,
        camera_fog: None,
        profile_route: None,
        default_fog: None,
        precipitation_sample_count: None,
        camera_biome_identifier: Some("minecraft:plains".into()),
        camera_biome_temperature: None,
        render_distance_blocks: Some(256.0),
    };
    let mut fogs = fog_profiles();
    let default_water = fogs
        .iter_mut()
        .find(|fog| fog.identifier.as_ref() == "minecraft:fog_default")
        .unwrap()
        .distances
        .iter_mut()
        .find(|distance| distance.medium == FogMedium::Water)
        .unwrap();
    default_water.end_bits = 48.0_f32.to_bits();

    let (frame, _) = derive_profiled_atmosphere_frame(
        WorldClock::default(),
        WeatherState::default(),
        0.0,
        meshing::CameraMedium::Water,
        &context,
        &profiles(),
        &fogs,
        None,
        0.0,
    );
    assert_eq!(frame.fog_end(), 60.0);
}
#[test]
fn active_rain_uses_the_exact_weather_fog_endpoint_from_the_default_layer() {
    let mut clock = WorldClock::default();
    let mut weather = WeatherState::default();
    replace_session(
        &mut clock,
        &mut weather,
        bootstrap(6_000, 0, true, 1.0, 0.0),
        0.0,
    );
    let context = EnvironmentContext {
        dimension: 0,
        fog_biomes: None,
        camera_profile: None,
        camera_fog: None,
        profile_route: None,
        default_fog: None,
        precipitation_sample_count: None,
        camera_biome_identifier: Some("minecraft:plains".into()),
        camera_biome_temperature: None,
        render_distance_blocks: Some(256.0),
    };
    let (frame, _) = derive_profiled_atmosphere_frame(
        clock,
        weather,
        0.0,
        meshing::CameraMedium::Air,
        &context,
        &profiles(),
        &fog_profiles(),
        None,
        1.0,
    );
    assert_eq!(frame.fog_start(), 58.88);
    assert_eq!(frame.fog_end(), 179.2);
}

pub(super) fn profiles() -> Vec<BiomeVisualProfile> {
    vec![
        BiomeVisualProfile {
            biome_identifier: "minecraft:hell".into(),
            fog_identifier: "minecraft:fog_hell".into(),
            atmosphere_identifier: "minecraft:hell_atmospherics".into(),
            lighting_identifier: "minecraft:nether_lighting".into(),
            sky_rgb8: None,
        },
        BiomeVisualProfile {
            biome_identifier: "minecraft:plains".into(),
            fog_identifier: "minecraft:fog_plains".into(),
            atmosphere_identifier: "minecraft:default_atmospherics".into(),
            lighting_identifier: "minecraft:default_lighting".into(),
            sky_rgb8: None,
        },
        BiomeVisualProfile {
            biome_identifier: "minecraft:the_end".into(),
            fog_identifier: "minecraft:fog_the_end".into(),
            atmosphere_identifier: "minecraft:end_atmospherics".into(),
            lighting_identifier: "minecraft:end_lighting".into(),
            sky_rgb8: Some(0),
        },
    ]
}

pub(super) fn fog_profiles() -> Vec<FogProfile> {
    let mut profiles = vec![
        FogProfile {
            identifier: "minecraft:fog_default".into(),
            distances: vec![
                fog_distance(
                    FogMedium::Air,
                    FogDistanceMode::RenderRelative,
                    0.92,
                    1.0,
                    0xAB_D2_FF,
                ),
                fog_distance(
                    FogMedium::Water,
                    FogDistanceMode::Fixed,
                    0.0,
                    60.0,
                    0x44_AF_F5,
                ),
                fog_distance(
                    FogMedium::Weather,
                    FogDistanceMode::RenderRelative,
                    0.23,
                    0.7,
                    0x66_66_66,
                ),
                fog_distance(
                    FogMedium::Lava,
                    FogDistanceMode::Fixed,
                    0.0,
                    0.64,
                    0x99_1A_00,
                ),
                fog_distance(
                    FogMedium::LavaResistance,
                    FogDistanceMode::Fixed,
                    2.0,
                    4.0,
                    0x99_1A_00,
                ),
            ]
            .into_boxed_slice(),
        },
        fog_profile(
            "minecraft:fog_hell",
            FogDistanceMode::Fixed,
            10.0,
            96.0,
            0x33_08_08,
        ),
        FogProfile {
            identifier: "minecraft:fog_plains".into(),
            distances: vec![fog_distance(
                FogMedium::Water,
                FogDistanceMode::Fixed,
                0.0,
                60.0,
                0x44_AF_F5,
            )]
            .into_boxed_slice(),
        },
        fog_profile(
            "minecraft:fog_the_end",
            FogDistanceMode::RenderRelative,
            0.92,
            1.0,
            0x0B_08_0C,
        ),
    ];
    profiles.sort_unstable_by(|left, right| left.identifier.cmp(&right.identifier));
    profiles
}

fn fog_profile(
    identifier: &str,
    mode: FogDistanceMode,
    start: f32,
    end: f32,
    rgb8: u32,
) -> FogProfile {
    FogProfile {
        identifier: identifier.into(),
        distances: vec![fog_distance(FogMedium::Air, mode, start, end, rgb8)].into_boxed_slice(),
    }
}

fn fog_distance(
    medium: FogMedium,
    mode: FogDistanceMode,
    start: f32,
    end: f32,
    rgb8: u32,
) -> FogDistance {
    FogDistance {
        transition: None,
        medium,
        mode,
        start_bits: start.to_bits(),
        end_bits: end.to_bits(),
        rgb8,
    }
}
