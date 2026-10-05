use bevy::prelude::Resource;
use meshing::CameraMedium;
use protocol::{WeatherChannel, WorldEnvironmentBootstrap};

use client_world::CommittedControlEvent;

mod atmosphere;
mod diagnostics;
pub(crate) use diagnostics::log_world_lighting;
mod time_override;
pub(crate) use time_override::VisualTimeOverride;
mod fog;
pub(crate) use fog::{FogPrecipitationSamples, fog_biome_samples};
mod numeric;
mod profile_lookup;
mod renderer_clock;
mod seasonal_foliage;
mod weather;
mod weather_fog;
mod world_clocks;
pub(crate) use atmosphere::update_atmosphere_frame;
use numeric::finite_nonnegative;
pub(crate) use seasonal_foliage::{WeatherTickFrame, update_seasonal_foliage};
pub(crate) use weather::{
    LightningFlashState, WeatherDisplay, load_optional_weather_textures, update_lightning,
    update_precipitation_scene,
};

#[derive(Resource, Default)]
pub(crate) struct CameraMediumState(pub(crate) CameraMedium);

#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub(crate) struct EnvironmentContext {
    pub(crate) dimension: i32,
    pub(crate) camera_biome_identifier: Option<Box<str>>,
    pub(crate) camera_biome_temperature: Option<f32>,
    pub(crate) fog_biomes: Vec<Option<Box<str>>>,
    /// Known precipitation-admitting cells in the native weather/fog lattice.
    pub(crate) precipitation_sample_count: Option<usize>,
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
/// advances it only while the daylight cycle is enabled and its named clock is
/// not paused.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct WorldClock {
    session_generation: u64,
    server_time: Option<f64>,
    server_time_anchor_seconds: Option<f64>,
    daylight_cycle_enabled: bool,
    /// Independent pause state from SyncWorldClocks, not doDaylightCycle.
    paused: bool,
    /// Native built-in hashed clock ID, pre-registered with the client level.
    overworld_clock_id: Option<u64>,
    ignored_clock_records: u64,
    last_diagnostic_seconds: Option<f64>,
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
    weather_cycle_enabled: bool,
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
            // Current native ClientLevel creates its clock module;
            // registerWorldClock initializes the
            // daylight clock to zero. StartGame current tick instead initializes
            // LevelData's elapsed tick counter. SetTime buffered
            // during loading is applied by onPlayerReady.
            0.0
        } else {
            f64::from(bootstrap.day_cycle_lock_time)
        }),
        server_time_anchor_seconds: Some(finite_nonnegative(elapsed_seconds)),
        daylight_cycle_enabled: bootstrap.daylight_cycle_enabled,
        paused: false,
        overworld_clock_id: Some(protocol::OVERWORLD_CLOCK_ID),
        ignored_clock_records: 0,
        last_diagnostic_seconds: None,
        last_update_sequence: None,
    };
    *weather = WeatherState {
        session_generation,
        rain_level: bootstrap.rain_level,
        lightning_level: bootstrap.lightning_level,
        weather_cycle_enabled: bootstrap.weather_cycle_enabled,
        last_update_sequence: None,
    };
}

/// Rebinds an accepted StartGame snapshot to its transport session identity.
pub(crate) fn bind_session_generation(
    clock: &mut WorldClock,
    weather: &mut WeatherState,
    generation: u64,
) {
    clock.session_generation = generation;
    weather.session_generation = generation;
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
            world_clocks::apply_legacy_time(clock, update.time, elapsed_seconds);
            clock.last_update_sequence = Some(sequence);
            world_clocks::trace_clock(clock, "SetTime", elapsed_seconds);
            true
        }
        CommittedControlEvent::WorldClocks { sequence, update } => {
            world_clocks::apply_clock_update(clock, update, sequence, elapsed_seconds);
            world_clocks::trace_clock(clock, "SyncWorldClocks", elapsed_seconds);
            true
        }
        CommittedControlEvent::DaylightCycle { sequence, update } => {
            let elapsed_seconds = finite_nonnegative(elapsed_seconds);
            let current_time = visual_world_time(*clock, elapsed_seconds);
            clock.server_time = Some(current_time);
            clock.server_time_anchor_seconds = Some(elapsed_seconds);
            clock.daylight_cycle_enabled = update.enabled;
            clock.last_update_sequence = Some(sequence);
            world_clocks::trace_clock(clock, "GameRulesChanged", elapsed_seconds);
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
        CommittedControlEvent::WeatherCycle { sequence, enabled } => {
            weather.weather_cycle_enabled = enabled;
            weather.last_update_sequence = Some(sequence);
            true
        }
        CommittedControlEvent::MovePlayer { .. }
        | CommittedControlEvent::PlayerMovementCorrection { .. }
        | CommittedControlEvent::ChangeDimension { .. }
        | CommittedControlEvent::DimensionChangeAck { .. }
        | CommittedControlEvent::Respawn { .. }
        | CommittedControlEvent::LocalMovementEffect { .. }
        | CommittedControlEvent::LocalMovementSpeed { .. }
        | CommittedControlEvent::LocalMovementFlags { .. }
        | CommittedControlEvent::NetworkStackLatency { .. }
        | CommittedControlEvent::LocalActorMotion { .. }
        | CommittedControlEvent::LocalHurt { .. }
        | CommittedControlEvent::PlayerListChanged { .. } => false,
    }
}

/// Returns the absolute Bedrock tick used for this rendered frame.
///
/// A disabled daylight cycle freezes the current anchor, initially
/// StartGame's explicit lock tick. Named clock initialization, SetTime, and
/// runtime daylight-cycle transitions re-anchor this value; StartGame elapsed
/// world ticks do not. Enabled, unpaused clocks advance at Bedrock's tick rate.
#[must_use]
pub(crate) fn visual_world_time(clock: WorldClock, elapsed_seconds: f64) -> f64 {
    let Some((server_time, anchor)) = clock.server_time.zip(clock.server_time_anchor_seconds)
    else {
        return 0.0;
    };
    let elapsed_seconds = finite_nonnegative(elapsed_seconds);
    if clock.daylight_cycle_enabled && !clock.paused {
        server_time + (elapsed_seconds - anchor).max(0.0) * f64::from(world::TICKS_PER_SECOND)
    } else {
        server_time
    }
}

#[cfg(test)]
mod tests;
