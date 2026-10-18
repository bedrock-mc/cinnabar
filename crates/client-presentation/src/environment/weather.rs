//! Fixed-tick weather presentation, lightning flashes, and shared weather tick samples.

use bevy::prelude::Resource;
use meshing::CameraMedium;
use render_model::{PRECIPITATION_LEVEL_PER_TICK, approach_level};
use render_scene::lightning_flash_level;

use super::{WeatherState, weather_fog::WeatherFog};

const MAX_FRAME_STEP_SECONDS: f64 = 1.0;

/// Bounded per-frame weather tick samples, previous/current rain in vanilla order.
#[derive(Debug, Default, Resource)]
pub struct WeatherTickFrame {
    pub rain: Vec<[f32; 2]>,
    pub dimension: i32,
    pub weather_cycle_enabled: bool,
}

impl WeatherTickFrame {
    /// Visits current rain levels once per completed weather tick, in simulation order.
    pub fn current_rain_levels(&self) -> impl Iterator<Item = f32> + '_ {
        self.rain.iter().map(|rain| rain[1])
    }
}

/// Time of the latest lightning strike; the sky and lightmap flash for a moment after it.
#[derive(Resource, Debug, Default)]
pub struct LightningFlashState {
    struck_at: Option<f64>,
}

impl LightningFlashState {
    /// Starts a flash; called when a lightning bolt actor appears.
    pub fn trigger(&mut self, elapsed_seconds: f64) {
        self.struck_at = Some(elapsed_seconds);
    }

    /// Returns the flash intensity at the supplied host elapsed time.
    pub fn level(&self, elapsed_seconds: f64) -> f32 {
        self.struck_at.map_or(0.0, |struck| {
            lightning_flash_level((elapsed_seconds - struck) as f32)
        })
    }
}

/// Rain and thunder levels eased toward the server targets, plus time spent submerged.
#[derive(Debug, Default)]
pub struct WeatherDisplay {
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
    fn advance(&mut self, target: WeatherState, elapsed_seconds: f64) -> WeatherState {
        self.advance_in_dimension(target, elapsed_seconds, 0)
    }

    /// Advances weather and fog in fixed-tick order, resetting interpolation on session or dimension changes.
    pub fn advance_in_dimension(
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
            let tick = simulation_clock::TICK_DURATION.as_secs_f64();
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
        let alpha = (self.tick_seconds / simulation_clock::TICK_DURATION.as_secs_f64()) as f32;
        WeatherState {
            rain_level: self.previous_rain + (self.rain - self.previous_rain) * alpha,
            lightning_level: self.previous_thunder + (self.thunder - self.previous_thunder) * alpha,
            ..target
        }
    }

    /// Transfers completed tick samples into the shared frame without copying them.
    pub fn publish_ticks(&mut self, output: &mut WeatherTickFrame) {
        std::mem::swap(output, &mut self.ticks);
    }

    /// Sets the current lattice admission count for subsequent weather-fog ticks.
    pub fn set_precipitation_count(&mut self, count: Option<usize>) {
        self.fog.set_precipitation_count(count);
    }

    /// Vanilla's weather fog level, independent of the interpolated displayed rain.
    pub fn fog_level(&self) -> f32 {
        self.fog.level(self.dimension)
    }

    /// Vanilla's current-tick rain level, used without interpolation by the sky's rain admission.
    pub fn current_rain_level(&self) -> f32 {
        self.rain
    }

    /// Seconds continuously spent in water as of the last `advance`; zero elsewhere.
    pub fn submerged_seconds(&mut self, medium: CameraMedium) -> f32 {
        if medium == CameraMedium::Water {
            self.submerged += self.step_seconds as f32;
        } else {
            self.submerged = 0.0;
        }
        self.submerged
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::environment::{WorldClock, replace_session};
    use protocol::WorldEnvironmentBootstrap;
    use render_model::PRECIPITATION_SAMPLE_OFFSETS;

    /// Builds server weather targets for one synthetic session.
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
        let expected =
            1.0 - PRECIPITATION_LEVEL_PER_TICK * (simulation_clock::TICKS_PER_SECOND - 1) as f32;
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
        let tick = simulation_clock::TICK_DURATION.as_secs_f64();
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
        let tick = simulation_clock::TICK_DURATION.as_secs_f64();
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
        display.advance(weather, simulation_clock::TICK_DURATION.as_secs_f64());
        assert!(display.fog_level() > 0.0);
        assert!(!display.ticks.weather_cycle_enabled);
    }

    #[test]
    fn weather_fog_resets_on_new_client_level_not_dimension_switch() {
        let tick = simulation_clock::TICK_DURATION.as_secs_f64();
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
        assert_eq!(
            display.ticks.rain.len(),
            simulation_clock::TICKS_PER_SECOND as usize
        );
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
        display.advance(weather, simulation_clock::TICK_DURATION.as_secs_f64());
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
