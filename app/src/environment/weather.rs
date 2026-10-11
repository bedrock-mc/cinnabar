//! Client-side weather presentation: eased levels, lightning flash and the precipitation scene.

use assets::BiomeRule;
use bevy::{
    prelude::{Local, Query, Res, ResMut, Resource, Time, Transform, With},
    time::Real,
};
use chunk_pipeline::WorldStream;
use meshing::CameraMedium;
use render::{
    AtmosphereFrame, ColumnSample, ColumnSampler, LightningScene, OcclusionGrid,
    PRECIPITATION_LEVEL_PER_TICK, PRECIPITATION_SAMPLE_OFFSETS, PRECIPITATION_TICKS_PER_SECOND,
    PrecipitationMix, PrecipitationScene, PrecipitationSim, RainSplashQueue, SkyKind,
    approach_level, average_precipitation, lightning_bolt_segments, lightning_flash_level,
    pick_rain_splashes, precipitation_forward_offset, push_bolt_records,
};

use super::{WeatherState, WeatherTickFrame, weather_fog::WeatherFog};
use {crate::runtime::world::ClientWorld, client_presentation::camera::FlyCamera};

const MAX_FRAME_STEP_SECONDS: f64 = 1.0;
const MAX_QUEUED_SPLASHES: usize = 256;
/// Ticks a bolt stays drawn after it spawns; needs native measurement.
const BOLT_VISIBLE_TICKS: u32 = 8;

const WEATHER_TEXTURES_FILENAME: &str = assets::carriers::WEATHER.output;
const WEATHER_TEXTURES_COMPILE_COMMAND: &str = "make weather-assets";

/// Loads the optional precipitation and End sky carrier next to the world carrier; an absent or
/// invalid carrier logs a notice and leaves the procedural fallbacks in place.
#[must_use]
pub(crate) fn load_optional_weather_textures(
    world_asset_path: &std::path::Path,
) -> render::WeatherTextureAssets {
    let path = world_asset_path.with_file_name(WEATHER_TEXTURES_FILENAME);
    let decoded = std::fs::read(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            assets::decode_weather_textures(&bytes).map_err(|error| error.to_string())
        });
    match decoded {
        Ok((textures, identity)) => {
            eprintln!("loaded weather textures from {}", path.display());
            render::WeatherTextureAssets::new(std::sync::Arc::new(textures), identity)
        }
        Err(error) => {
            eprintln!(
                "weather textures unavailable at {} ({error}); using procedural precipitation and End sky; build with {WEATHER_TEXTURES_COMPILE_COMMAND}",
                path.display()
            );
            render::WeatherTextureAssets::default()
        }
    }
}

/// Time of the latest lightning strike; the sky and lightmap flash for a moment after it.
#[derive(Resource, Debug, Default)]
pub(crate) struct LightningFlashState {
    struck_at: Option<f64>,
}

impl LightningFlashState {
    /// Starts a flash; called when a lightning bolt actor appears.
    pub(crate) fn trigger(&mut self, elapsed_seconds: f64) {
        self.struck_at = Some(elapsed_seconds);
    }

    pub(crate) fn level(&self, elapsed_seconds: f64) -> f32 {
        self.struck_at.map_or(0.0, |struck| {
            lightning_flash_level((elapsed_seconds - struck) as f32)
        })
    }
}

/// Rain and thunder levels eased toward the server targets, plus time spent submerged.
#[derive(Debug, Default)]
pub(crate) struct WeatherDisplay {
    generation: Option<u64>,
    dimension: i32,
    rain: f32,
    thunder: f32,
    previous_rain: f32,
    previous_thunder: f32,
    last_elapsed: Option<f64>,
    step_seconds: f64,
    submerged: f32,
    tick_seconds: f64,
    ticks: WeatherTickFrame,
    fog: WeatherFog,
}

impl WeatherDisplay {
    /// Moves the displayed levels toward `target`; a new session snaps to it.
    #[cfg(test)]
    pub(crate) fn advance(&mut self, target: WeatherState, elapsed_seconds: f64) -> WeatherState {
        self.advance_in_dimension(target, elapsed_seconds, 0)
    }

    pub(crate) fn advance_in_dimension(
        &mut self,
        target: WeatherState,
        elapsed_seconds: f64,
        dimension: i32,
    ) -> WeatherState {
        self.ticks.rain.clear();
        self.ticks.dimension = dimension;
        self.ticks.weather_cycle_enabled = target.weather_cycle_enabled;
        let previous = self.last_elapsed.replace(elapsed_seconds);
        self.step_seconds = previous.map_or(0.0, |previous| {
            (elapsed_seconds - previous).clamp(0.0, MAX_FRAME_STEP_SECONDS)
        });
        let new_session = self.generation != Some(target.session_generation);
        if new_session || self.dimension != dimension {
            if new_session {
                self.fog.reset_session();
            }
            self.generation = Some(target.session_generation);
            self.dimension = dimension;
            self.submerged = 0.0;
            self.step_seconds = 0.0;
            self.rain = target.rain_level;
            self.thunder = target.lightning_level;
            self.previous_rain = self.rain;
            self.previous_thunder = self.thunder;
            self.tick_seconds = 0.0;
        } else {
            let tick = world::TICK_DURATION.as_secs_f64();
            self.tick_seconds += self.step_seconds;
            let count = ((self.tick_seconds + f64::EPSILON) / tick).floor() as usize;
            self.tick_seconds = (self.tick_seconds - count as f64 * tick).max(0.0);
            let step = PRECIPITATION_LEVEL_PER_TICK;
            for _ in 0..count {
                self.previous_rain = self.rain;
                self.previous_thunder = self.thunder;
                self.rain = approach_level(self.rain, target.rain_level, step);
                self.thunder = approach_level(self.thunder, target.lightning_level, step);
                self.fog.tick(self.previous_rain, dimension);
                self.ticks.rain.push([self.previous_rain, self.rain]);
            }
        }
        // Vanilla weather consumers interpolate the previous/current tick states;
        // only the seasonal palette rate explicitly uses alpha zero.
        let alpha = (self.tick_seconds / world::TICK_DURATION.as_secs_f64()) as f32;
        WeatherState {
            rain_level: self.previous_rain + (self.rain - self.previous_rain) * alpha,
            lightning_level: self.previous_thunder + (self.thunder - self.previous_thunder) * alpha,
            ..target
        }
    }

    pub(crate) fn publish_ticks(&mut self, output: &mut WeatherTickFrame) {
        std::mem::swap(output, &mut self.ticks);
    }

    pub(crate) fn set_precipitation_count(&mut self, count: Option<usize>) {
        self.fog.set_precipitation_count(count);
    }

    /// Vanilla's weather fog level, independent of the interpolated displayed rain.
    pub(crate) fn fog_level(&self) -> f32 {
        self.fog.level(self.dimension)
    }

    /// Vanilla's current-tick rain level, used without interpolation by the sky's rain admission.
    pub(crate) fn current_rain_level(&self) -> f32 {
        self.rain
    }

    /// Seconds continuously spent in water as of the last `advance`; zero elsewhere.
    pub(crate) fn submerged_seconds(&mut self, medium: CameraMedium) -> f32 {
        if medium == CameraMedium::Water {
            self.submerged += self.step_seconds as f32;
        } else {
            self.submerged = 0.0;
        }
        self.submerged
    }
}

struct StreamColumns<'a> {
    stream: &'a WorldStream,
    rules: &'a [BiomeRule],
}

impl ColumnSampler for StreamColumns<'_> {
    fn sample(&mut self, x: i32, z: i32) -> Option<ColumnSample> {
        let top = self.stream.top_non_air_block_y(x, z)?;
        let biome =
            self.stream
                .camera_biome_id([x as f32 + 0.5, top as f32 + 0.5, z as f32 + 0.5])?;
        let rule = &self.rules[self
            .rules
            .binary_search_by_key(&biome, |rule| rule.id)
            .ok()?];
        Some(ColumnSample {
            surface_y: top.saturating_add(1),
            temperature: rule.temperature(),
            downfall: rule.downfall(),
        })
    }
}

/// Occlusion columns re-sampled per tick; the whole grid refreshes about every 0.8 s.
const OCCLUSION_REFRESH_PER_TICK: usize = 256;
/// Most simulation ticks replayed after a stall.
const MAX_CATCH_UP_TICKS: u64 = 10;
const PRECIPITATION_SEED: u64 = 0x5241_494e;

pub(crate) struct PrecipitationCadence {
    sim: PrecipitationSim,
    grid: OcclusionGrid,
    cursor: usize,
    last_tick: Option<u64>,
}

impl Default for PrecipitationCadence {
    fn default() -> Self {
        Self {
            sim: PrecipitationSim::new(PRECIPITATION_SEED),
            grid: OcclusionGrid::default(),
            cursor: 0,
            last_tick: None,
        }
    }
}

/// Biome (temperature, downfall) at a block position, when its chunk and biome are known.
fn biome_climate(
    stream: &WorldStream,
    rules: &[BiomeRule],
    position: [f32; 3],
) -> Option<(f32, f32)> {
    let id = stream.camera_biome_id(position)?;
    let rule = &rules[rules.binary_search_by_key(&id, |rule| rule.id).ok()?];
    Some((rule.temperature(), rule.downfall()))
}

/// Ticks the vanilla precipitation layers, refreshes the occlusion grid and queues rain splashes.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_precipitation_scene(
    frame: Res<AtmosphereFrame>,
    client_world: Res<ClientWorld>,
    camera: Query<&Transform, With<FlyCamera>>,
    time: Res<Time<Real>>,
    mut scene: ResMut<PrecipitationScene>,
    mut splashes: ResMut<RainSplashQueue>,
    mut mix: ResMut<PrecipitationMix>,
    mut cadence: Local<PrecipitationCadence>,
) {
    let level = frame.rain_level();
    let stream = client_world.stream.as_ref();
    let camera = camera.single().ok();
    let (Some(stream), Some(camera), true) = (
        stream,
        camera,
        level > 0.0 && frame.sky_kind() == SkyKind::Overworld,
    ) else {
        scene.layers.clear();
        *mix = PrecipitationMix::default();
        *cadence = PrecipitationCadence::default();
        return;
    };
    let elapsed = time.elapsed_secs_f64();
    let ticks = elapsed * PRECIPITATION_TICKS_PER_SECOND;
    let tick = ticks as u64;
    let origin = camera.translation.to_array();
    let rules = &client_world.runtime_assets.biome_assets().rules;
    let pending = cadence
        .last_tick
        .map_or(1, |last| tick.saturating_sub(last).min(MAX_CATCH_UP_TICKS));
    if pending > 0 {
        cadence.last_tick = Some(tick);
        let feet = origin.map(f32::floor);
        let samples = PRECIPITATION_SAMPLE_OFFSETS.map(|offset| {
            let position = [
                feet[0] + offset[0] as f32,
                feet[1] + offset[1] as f32,
                feet[2] + offset[2] as f32,
            ];
            biome_climate(stream, rules, position)
                .map(|(temperature, downfall)| (temperature, downfall, position[1] as i32))
        });
        let averaged = average_precipitation(&samples);
        *mix = PrecipitationMix {
            rain: averaged.rain * level,
            snow: averaged.snow * level,
        };
        let weights = averaged.lattice_weights(level);
        for step in 0..pending {
            let seconds = (tick - (pending - 1 - step)) as f64 / PRECIPITATION_TICKS_PER_SECOND;
            cadence.sim.tick(weights, seconds as f32);
        }
        let PrecipitationCadence { grid, cursor, .. } = &mut *cadence;
        let mut columns = StreamColumns { stream, rules };
        let refresh = OCCLUSION_REFRESH_PER_TICK * pending as usize;
        grid.update(
            OcclusionGrid::origin_for(origin),
            &mut columns,
            cursor,
            refresh,
        );
        scene.occlusion = std::sync::Arc::new(grid.clone());
        scene.occlusion_generation = scene.occlusion_generation.wrapping_add(1);
        if splashes.positions.len() > MAX_QUEUED_SPLASHES {
            splashes.positions.clear();
        }
        let mut picked = Vec::new();
        pick_rain_splashes(grid, level, tick, &mut picked);
        splashes.positions.extend(picked);
    }
    scene.forward_offset = precipitation_forward_offset(camera.forward().as_vec3().to_array());
    let PrecipitationScene {
        layers,
        forward_offset,
        ..
    } = &mut *scene;
    cadence.sim.frame(
        camera.translation.as_dvec3().to_array(),
        *forward_offset,
        ticks.fract() as f32,
        layers,
    );
}

/// Flashes the sky when a lightning-bolt actor first appears and draws live bolts.
pub(crate) fn update_lightning(
    client_world: Res<ClientWorld>,
    time: Res<Time<Real>>,
    mut flash: ResMut<LightningFlashState>,
    mut scene: ResMut<LightningScene>,
    mut seen: Local<std::collections::HashSet<i64>>,
) {
    scene.records.clear();
    let Some(stream) = client_world.stream.as_ref() else {
        seen.clear();
        return;
    };
    let bolts = stream.authority().lightning_bolts();
    seen.retain(|id| bolts.iter().any(|bolt| bolt.unique_id == *id));
    for bolt in bolts {
        if seen.insert(bolt.unique_id) {
            flash.trigger(time.elapsed_secs_f64());
        }
        if bolt.age_ticks >= BOLT_VISIBLE_TICKS {
            continue;
        }
        let intensity = if bolt.age_ticks % 2 == 0 { 1.0 } else { 0.6 };
        push_bolt_records(
            &lightning_bolt_segments(bolt.unique_id as u64, bolt.position),
            intensity,
            &mut scene.records,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{WorldClock, replace_session};
    use protocol::WorldEnvironmentBootstrap;

    fn target(rain: f32, thunder: f32, generation: u64) -> WeatherState {
        let mut weather = WeatherState::default();
        let mut clock = WorldClock::default();
        replace_session(
            &mut clock,
            &mut weather,
            WorldEnvironmentBootstrap {
                initial_time: 0,
                day_cycle_lock_time: 0,
                daylight_cycle_enabled: true,
                weather_cycle_enabled: true,
                rain_level: rain,
                lightning_level: thunder,
            },
            0.0,
        );
        weather.session_generation = generation;
        weather
    }

    #[test]
    fn first_frame_snaps_then_levels_ease_at_a_fixed_rate() {
        let mut display = WeatherDisplay::default();
        assert_eq!(display.advance(target(1.0, 0.0, 1), 10.0).rain_level(), 1.0);
        let fading = display.advance(target(0.0, 0.0, 1), 11.0);
        let expected = 1.0 - PRECIPITATION_LEVEL_PER_TICK * (world::TICKS_PER_SECOND - 1) as f32;
        assert!((fading.rain_level() - expected).abs() < 1.0e-6);
        let mut done = fading;
        for second in 12..20 {
            done = display.advance(target(0.0, 0.0, 1), f64::from(second));
        }
        assert_eq!(done.rain_level(), 0.0);
    }

    #[test]
    fn a_new_session_snaps_instead_of_fading() {
        let mut display = WeatherDisplay::default();
        display.advance(target(1.0, 1.0, 1), 0.0);
        let next = display.advance(target(0.0, 0.0, 2), 0.1);
        assert_eq!((next.rain_level(), next.lightning_level()), (0.0, 0.0));
    }

    #[test]
    fn seasonal_weather_samples_are_tick_states_not_interpolated_atmosphere() {
        let tick = world::TICK_DURATION.as_secs_f64();
        let mut display = WeatherDisplay::default();
        display.advance(target(0.0, 0.0, 1), 0.0);
        let shown = display.advance(target(1.0, 1.0, 1), tick);
        assert_eq!(
            shown.rain_level(),
            0.0,
            "tick boundary starts at previous state"
        );
        assert_eq!(display.ticks.rain, [[0.0, PRECIPITATION_LEVEL_PER_TICK]]);
        let shown = display.advance(target(1.0, 1.0, 1), tick * 1.5);
        assert!(display.ticks.rain.is_empty());
        assert!((shown.rain_level() - PRECIPITATION_LEVEL_PER_TICK * 0.5).abs() < 1.0e-7);
        assert_eq!(shown.rain_level(), shown.lightning_level());
        display.advance(target(1.0, 1.0, 1), tick * 2.0);
        assert_eq!(
            display.ticks.rain,
            [[
                PRECIPITATION_LEVEL_PER_TICK,
                PRECIPITATION_LEVEL_PER_TICK * 2.0
            ]]
        );
    }

    #[test]
    fn weather_fog_uses_previous_rain_only_on_fixed_renderer_ticks() {
        let tick = world::TICK_DURATION.as_secs_f64();
        let mut display = WeatherDisplay::default();
        display.set_precipitation_count(Some(PRECIPITATION_SAMPLE_OFFSETS.len()));
        display.advance(target(0.0, 0.0, 1), 0.0);
        assert_eq!(display.fog_level(), 0.0);
        display.advance(target(1.0, 0.0, 1), tick);
        assert_eq!(
            display.fog_level(),
            0.0,
            "current rain must not leak into alpha-zero sampling"
        );
        display.advance(target(1.0, 0.0, 1), tick * 1.5);
        assert_eq!(
            display.fog_level(),
            0.0,
            "a fractional render frame is not a weather tick"
        );
        display.advance(target(1.0, 0.0, 1), tick * 2.0);
        let mut expected = WeatherFog::default();
        expected.set_precipitation_count(Some(PRECIPITATION_SAMPLE_OFFSETS.len()));
        expected.tick(PRECIPITATION_LEVEL_PER_TICK, 0);
        assert_eq!(display.fog_level(), expected.level(0));
        assert!(display.fog_level() > 0.0);
    }

    #[test]
    fn frozen_weather_cycle_does_not_freeze_renderer_fog() {
        let mut weather = target(1.0, 0.0, 1);
        weather.weather_cycle_enabled = false;
        let mut display = WeatherDisplay::default();
        display.set_precipitation_count(Some(PRECIPITATION_SAMPLE_OFFSETS.len()));
        display.advance(weather, 0.0);
        display.advance(weather, world::TICK_DURATION.as_secs_f64());
        assert!(display.fog_level() > 0.0);
        assert!(!display.ticks.weather_cycle_enabled);
    }

    #[test]
    fn weather_fog_resets_on_new_client_level_not_dimension_switch() {
        let tick = world::TICK_DURATION.as_secs_f64();
        let mut display = WeatherDisplay::default();
        display.set_precipitation_count(Some(PRECIPITATION_SAMPLE_OFFSETS.len()));
        display.advance(target(1.0, 0.0, 1), 0.0);
        display.advance(target(1.0, 0.0, 1), tick);
        let retained = display.fog_level();
        display.advance_in_dimension(target(1.0, 0.0, 1), tick * 2.0, 1);
        assert_eq!(display.fog_level(), 0.0);
        display.advance_in_dimension(target(1.0, 0.0, 1), tick * 3.0, 0);
        assert_eq!(display.fog_level(), retained);
        display.advance(target(1.0, 0.0, 2), tick * 4.0);
        assert_eq!(display.fog_level(), 0.0);
    }

    #[test]
    fn weather_sample_batches_are_bounded_and_clear_after_session_or_dimension_reset() {
        let mut display = WeatherDisplay::default();
        display.advance(target(0.0, 0.0, 1), 0.0);
        display.advance(target(1.0, 1.0, 1), 1_000.0);
        assert_eq!(display.ticks.rain.len(), world::TICKS_PER_SECOND as usize);
        display.advance_in_dimension(target(1.0, 1.0, 1), 1_001.0, 1);
        assert!(display.ticks.rain.is_empty());
        assert_eq!(display.tick_seconds, 0.0);
        display.advance(target(0.0, 0.0, 2), 1_002.0);
        assert!(display.ticks.rain.is_empty());
        assert_eq!((display.rain, display.previous_rain), (0.0, 0.0));
    }

    #[test]
    fn weather_cycle_control_is_published_with_tick_batch_and_replaced_on_reconnect() {
        let mut weather = target(1.0, 0.0, 1);
        let mut clock = WorldClock::default();
        assert!(super::super::apply_environment_control(
            client_world::CommittedControlEvent::WeatherCycle {
                sequence: 5,
                enabled: false
            },
            &mut clock,
            &mut weather,
            0.0,
        ));
        let mut display = WeatherDisplay::default();
        display.advance(weather, 0.0);
        display.advance(weather, world::TICK_DURATION.as_secs_f64());
        assert!(!display.ticks.weather_cycle_enabled);
        assert_eq!(weather.last_update_sequence(), Some(5));
        display.advance(target(1.0, 0.0, 2), 1.0);
        assert!(display.ticks.weather_cycle_enabled);
        assert!(display.ticks.rain.is_empty());
    }

    #[test]
    fn submerged_time_accumulates_only_in_water() {
        let mut display = WeatherDisplay::default();
        display.advance(target(0.0, 0.0, 1), 0.0);
        display.advance(target(0.0, 0.0, 1), 1.0);
        assert_eq!(display.submerged_seconds(CameraMedium::Water), 1.0);
        display.advance(target(0.0, 0.0, 1), 2.0);
        assert_eq!(display.submerged_seconds(CameraMedium::Water), 2.0);
        assert_eq!(display.submerged_seconds(CameraMedium::Air), 0.0);
    }

    #[test]
    fn new_session_restarts_the_water_transition() {
        let mut display = WeatherDisplay::default();
        display.advance(target(0.0, 0.0, 1), 0.0);
        display.advance(target(0.0, 0.0, 1), 1.0);
        assert_eq!(display.submerged_seconds(CameraMedium::Water), 1.0);
        display.advance(target(0.0, 0.0, 2), 2.0);
        assert_eq!(display.submerged_seconds(CameraMedium::Water), 0.0);
    }

    #[test]
    fn lightning_flash_starts_bright_and_expires() {
        let mut flash = LightningFlashState::default();
        assert_eq!(flash.level(5.0), 0.0);
        flash.trigger(5.0);
        assert_eq!(flash.level(5.0), 1.0);
        assert_eq!(flash.level(6.0), 0.0);
    }
}
