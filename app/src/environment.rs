use assets::{BiomeVisualProfile, FogMedium, FogProfile};
use bevy::{
    prelude::{Res, ResMut, Resource, Time},
    time::Real,
};
use meshing::CameraMedium;
use protocol::{WeatherChannel, WorldEnvironmentBootstrap};
use render::AtmosphereFrame;

use client_world::CommittedControlEvent;

#[derive(Resource, Default)]
pub(crate) struct CameraMediumState(pub(crate) CameraMedium);

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct EnvironmentContext {
    pub(crate) dimension: i32,
    pub(crate) camera_biome_identifier: Option<Box<str>>,
    pub(crate) render_distance_blocks: Option<f32>,
}

/// Active client profile identifiers. Lighting remains routed through the
/// current provisional scalar shader until native RGB calibration is available.
#[derive(Resource, Debug, Clone, Default, Eq, PartialEq)]
pub(crate) struct EnvironmentProfileRoute {
    pub(crate) biome_identifier: Option<Box<str>>,
    pub(crate) fog_identifier: Option<Box<str>>,
    pub(crate) atmosphere_identifier: Option<Box<str>>,
    pub(crate) provisional_lighting_identifier: Option<Box<str>>,
}

/// Server-authored world-clock snapshot for the active StartGame session.
///
/// This stores the latest server-authored or runtime-transition time anchor and
/// advances it monotonically only while the daylight cycle is enabled.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct WorldClock {
    session_generation: u64,
    server_time: Option<f64>,
    server_time_anchor_seconds: Option<f64>,
    daylight_cycle_enabled: bool,
    last_update_sequence: Option<u64>,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "read by the upcoming Phase 2.7 atmosphere systems"
    )
)]
impl WorldClock {
    #[must_use]
    pub(crate) const fn session_generation(self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub(crate) const fn server_time(self) -> Option<f64> {
        self.server_time
    }

    #[must_use]
    pub(crate) const fn daylight_cycle_enabled(self) -> bool {
        self.daylight_cycle_enabled
    }

    #[must_use]
    pub(crate) const fn last_update_sequence(self) -> Option<u64> {
        self.last_update_sequence
    }
}

/// Server-authored weather targets for the active StartGame session.
///
/// The protocol layer already bounds both channels to `0.0..=1.0`; this
/// resource deliberately retains those targets without interpolation.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct WeatherState {
    session_generation: u64,
    rain_level: f32,
    lightning_level: f32,
    last_update_sequence: Option<u64>,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "read by the upcoming Phase 2.7 atmosphere systems"
    )
)]
impl WeatherState {
    #[must_use]
    pub(crate) const fn session_generation(self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub(crate) const fn rain_level(self) -> f32 {
        self.rain_level
    }

    #[must_use]
    pub(crate) const fn lightning_level(self) -> f32 {
        self.lightning_level
    }

    #[must_use]
    pub(crate) const fn last_update_sequence(self) -> Option<u64> {
        self.last_update_sequence
    }
}

/// Replaces the environment snapshot when a new StartGame begins a session.
pub(crate) fn replace_session(
    clock: &mut WorldClock,
    weather: &mut WeatherState,
    bootstrap: WorldEnvironmentBootstrap,
    elapsed_seconds: f64,
) {
    let session_generation = clock
        .session_generation
        .max(weather.session_generation)
        .saturating_add(1);
    *clock = WorldClock {
        session_generation,
        server_time: Some(if bootstrap.daylight_cycle_enabled {
            bedrock_ticks_as_f64(bootstrap.initial_time)
        } else {
            f64::from(bootstrap.day_cycle_lock_time)
        }),
        server_time_anchor_seconds: Some(finite_nonnegative(elapsed_seconds)),
        daylight_cycle_enabled: bootstrap.daylight_cycle_enabled,
        last_update_sequence: None,
    };
    *weather = WeatherState {
        session_generation,
        rain_level: bootstrap.rain_level,
        lightning_level: bootstrap.lightning_level,
        last_update_sequence: None,
    };
}

/// Applies one FIFO-committed environment control.
///
/// Returns `true` when the control was environment-only. Spatial controls,
/// including dimension changes, leave the session snapshot untouched.
pub(crate) fn apply_environment_control(
    control: CommittedControlEvent,
    clock: &mut WorldClock,
    weather: &mut WeatherState,
    elapsed_seconds: f64,
) -> bool {
    match control {
        CommittedControlEvent::SetTime { sequence, update } => {
            clock.server_time = Some(f64::from(update.time));
            clock.server_time_anchor_seconds = Some(finite_nonnegative(elapsed_seconds));
            clock.last_update_sequence = Some(sequence);
            true
        }
        CommittedControlEvent::DaylightCycle { sequence, update } => {
            let elapsed_seconds = finite_nonnegative(elapsed_seconds);
            let current_time = visual_world_time(*clock, elapsed_seconds);
            clock.server_time = Some(current_time);
            clock.server_time_anchor_seconds = Some(elapsed_seconds);
            clock.daylight_cycle_enabled = update.enabled;
            clock.last_update_sequence = Some(sequence);
            true
        }
        CommittedControlEvent::Weather { sequence, update } => {
            match update.channel {
                WeatherChannel::Rain => weather.rain_level = update.level,
                WeatherChannel::Lightning => weather.lightning_level = update.level,
            }
            weather.last_update_sequence = Some(sequence);
            true
        }
        CommittedControlEvent::MovePlayer { .. }
        | CommittedControlEvent::PlayerMovementCorrection { .. }
        | CommittedControlEvent::ChangeDimension { .. } => false,
    }
}

/// Returns the absolute Bedrock tick used for this rendered frame.
///
/// A disabled daylight cycle freezes the current anchor, initially
/// StartGame's explicit lock tick. StartGame current time, SetTime, and runtime
/// daylight-cycle transitions all re-anchor this value. Enabled clocks advance
/// from the anchor at Bedrock's twenty ticks per second.
#[must_use]
pub(crate) fn visual_world_time(clock: WorldClock, elapsed_seconds: f64) -> f64 {
    let Some((server_time, anchor)) = clock.server_time.zip(clock.server_time_anchor_seconds)
    else {
        return 0.0;
    };
    let elapsed_seconds = finite_nonnegative(elapsed_seconds);
    if clock.daylight_cycle_enabled {
        server_time + (elapsed_seconds - anchor).max(0.0) * 20.0
    } else {
        server_time
    }
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Bedrock world ticks are rendered as a continuous f64 timeline"
)]
fn bedrock_ticks_as_f64(ticks: i64) -> f64 {
    ticks as f64
}

#[must_use]
#[cfg(test)]
pub(crate) fn derive_atmosphere_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
) -> AtmosphereFrame {
    derive_atmosphere_frame_for_medium(clock, weather, elapsed_seconds, CameraMedium::Air)
}

#[must_use]
pub(crate) fn derive_atmosphere_frame_for_medium(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
) -> AtmosphereFrame {
    AtmosphereFrame::from_bedrock_time(
        visual_world_time(clock, elapsed_seconds),
        weather.rain_level,
        weather.lightning_level,
    )
    .with_camera_medium(medium)
}

#[must_use]
pub(crate) fn derive_profiled_atmosphere_frame(
    clock: WorldClock,
    weather: WeatherState,
    elapsed_seconds: f64,
    medium: CameraMedium,
    context: &EnvironmentContext,
    biome_profiles: &[BiomeVisualProfile],
    fog_profiles: &[FogProfile],
) -> (AtmosphereFrame, EnvironmentProfileRoute) {
    let base = derive_atmosphere_frame_for_medium(clock, weather, elapsed_seconds, medium);
    let profile = context
        .camera_biome_identifier
        .as_deref()
        .and_then(|identifier| find_biome_profile(biome_profiles, identifier))
        .or_else(|| {
            dimension_fallback_biome(context.dimension)
                .and_then(|identifier| find_biome_profile(biome_profiles, identifier))
        });
    let Some(profile) = profile else {
        return (base, EnvironmentProfileRoute::default());
    };
    let resolved_fog = context.render_distance_blocks.and_then(|render_distance| {
        let fog = fog_profiles
            .binary_search_by(|fog| fog.identifier.cmp(&profile.fog_identifier))
            .ok()
            .map(|index| &fog_profiles[index])?;
        let default_fog = fog_profiles
            .binary_search_by(|fog| fog.identifier.as_ref().cmp("minecraft:fog_default"))
            .ok()
            .map(|index| &fog_profiles[index]);
        let distance = |medium| {
            fog.distance(medium)
                .or_else(|| default_fog.and_then(|fallback| fallback.distance(medium)))
        };
        let requested = match medium {
            CameraMedium::Air
                if weather.rain_level > 0.0 && distance(FogMedium::Weather).is_some() =>
            {
                FogMedium::Weather
            }
            CameraMedium::Air => FogMedium::Air,
            CameraMedium::Water => FogMedium::Water,
            CameraMedium::Lava => FogMedium::Lava,
        };
        distance(requested)?.resolve(render_distance)
    });
    (
        base.with_environment_profile(profile.sky_rgb8, resolved_fog),
        EnvironmentProfileRoute {
            biome_identifier: Some(profile.biome_identifier.clone()),
            fog_identifier: Some(profile.fog_identifier.clone()),
            atmosphere_identifier: Some(profile.atmosphere_identifier.clone()),
            provisional_lighting_identifier: Some(profile.lighting_identifier.clone()),
        },
    )
}

fn find_biome_profile<'a>(
    profiles: &'a [BiomeVisualProfile],
    identifier: &str,
) -> Option<&'a BiomeVisualProfile> {
    profiles
        .binary_search_by(|profile| profile.biome_identifier.as_ref().cmp(identifier))
        .ok()
        .map(|index| &profiles[index])
}

const fn dimension_fallback_biome(dimension: i32) -> Option<&'static str> {
    match dimension {
        0 => Some("minecraft:plains"),
        1 => Some("minecraft:hell"),
        2 => Some("minecraft:the_end"),
        _ => None,
    }
}

pub(crate) fn update_atmosphere_frame(
    clock: Res<WorldClock>,
    weather: Res<WeatherState>,
    medium: Res<CameraMediumState>,
    context: Res<EnvironmentContext>,
    atmosphere_assets: Res<render::AtmosphereTextureAssets>,
    time: Res<Time<Real>>,
    outputs: (ResMut<AtmosphereFrame>, ResMut<EnvironmentProfileRoute>),
) {
    let (mut frame, mut route) = outputs;
    let Some(assets) = atmosphere_assets.runtime() else {
        *frame =
            derive_atmosphere_frame_for_medium(*clock, *weather, time.elapsed_secs_f64(), medium.0);
        *route = EnvironmentProfileRoute::default();
        return;
    };
    let (next_frame, next_route) = derive_profiled_atmosphere_frame(
        *clock,
        *weather,
        time.elapsed_secs_f64(),
        medium.0,
        &context,
        assets.biome_profiles(),
        assets.fog_profiles(),
    );
    *frame = next_frame;
    *route = next_route;
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use assets::{BiomeVisualProfile, FogDistance, FogDistanceMode, FogMedium, FogProfile};
    use protocol::{
        ChangeDimensionEvent, DaylightCycleUpdateEvent, SetTimeEvent, WeatherChannel,
        WeatherUpdateEvent, WorldEnvironmentBootstrap,
    };

    use super::{
        EnvironmentContext, WeatherState, WorldClock, apply_environment_control,
        derive_atmosphere_frame, derive_atmosphere_frame_for_medium,
        derive_profiled_atmosphere_frame, replace_session, visual_world_time,
    };
    use client_world::CommittedControlEvent;

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
            rain_level,
            lightning_level,
        }
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
        assert_eq!(clock.server_time(), Some(6_000.0));
        assert!(clock.daylight_cycle_enabled());
        assert_eq!(visual_world_time(clock, 10.0), 6_000.0);
        assert_eq!(visual_world_time(clock, 12.5), 6_050.0);
        assert_eq!(
            derive_atmosphere_frame(clock, weather, 10.0).sun_direction(),
            [0.0, 1.0, 0.0],
            "StartGame noon must begin at full overhead daylight"
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
                change: ChangeDimensionEvent {
                    dimension: 1,
                    position: [0.0, 64.0, 0.0],
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
        assert_eq!(visual_world_time(clock, 12.5), 6_050.0);

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

        for (time, expected) in [
            (0, [1.0, 0.0, 0.0]),
            (6_000, [0.0, 1.0, 0.0]),
            (12_000, [-1.0, 0.0, 0.0]),
            (18_000, [0.0, -1.0, 0.0]),
        ] {
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
    fn atmosphere_bounds_weather_and_session_replacement_reanchors_initial_time() {
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
        assert_eq!(clock.server_time(), Some(12_000.0));
        assert_eq!(visual_world_time(clock, 50_000.0), 12_000.0);
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
        let context = EnvironmentContext {
            dimension: 2,
            camera_biome_identifier: None,
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
        );

        assert_eq!(frame.sky_zenith(), [0.0; 3]);
        assert_eq!(frame.sky_horizon(), [0.0; 3]);
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
            camera_biome_identifier: Some("minecraft:plains".into()),
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
        );
        assert_eq!(route.biome_identifier.as_deref(), Some("minecraft:plains"));
        assert_eq!(frame.fog_end(), 256.0);
    }

    #[test]
    fn pinned_shape_plains_air_falls_back_to_the_exact_default_fog_layer() {
        let context = EnvironmentContext {
            dimension: 0,
            camera_biome_identifier: Some("minecraft:plains".into()),
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
        );
        assert_eq!(frame.fog_start(), 235.52);
        assert_eq!(frame.fog_end(), 256.0);
    }

    #[test]
    fn biome_specific_medium_takes_precedence_over_the_default_fog_layer() {
        let context = EnvironmentContext {
            dimension: 0,
            camera_biome_identifier: Some("minecraft:plains".into()),
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
            camera_biome_identifier: Some("minecraft:plains".into()),
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
        );
        assert_eq!(frame.fog_start(), 58.88);
        assert_eq!(frame.fog_end(), 179.2);
    }

    fn profiles() -> Vec<BiomeVisualProfile> {
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

    fn fog_profiles() -> Vec<FogProfile> {
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
            distances: vec![fog_distance(FogMedium::Air, mode, start, end, rgb8)]
                .into_boxed_slice(),
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
            medium,
            mode,
            start_bits: start.to_bits(),
            end_bits: end.to_bits(),
            rgb8,
        }
    }
}
