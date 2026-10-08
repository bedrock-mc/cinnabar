//! Vanilla weather fog accumulator.
//!
//! The native renderer accumulates separate rain/snow lattice counters before
//! summing them. Their combined count gives the same target mathematically;
//! mixed-biome f32 addition order remains incomplete until positional temperature
//! is available. No base-temperature classification is substituted here.

use render::PRECIPITATION_SAMPLE_OFFSETS;

const RAIN_SAMPLE_WEIGHT: f32 = 0.5;
const FOG_COUNTER_WEIGHT: f32 = 0.2;
const PREVIOUS_FOG_WEIGHT: f32 = 0.99;
const TARGET_FOG_WEIGHT: f32 = 0.01;

#[derive(Debug, Default)]
pub(super) struct WeatherFog {
    precipitation_count: usize,
    smoothed: f32,
}

impl WeatherFog {
    pub(super) fn set_precipitation_count(&mut self, count: Option<usize>) {
        self.precipitation_count = count.unwrap_or(0).min(PRECIPITATION_SAMPLE_OFFSETS.len());
    }

    /// Weather and the player’s weather renderer start with zeroed smoothing state.
    pub(super) fn reset_session(&mut self) {
        self.smoothed = 0.0;
    }

    /// Called once per renderer tick, not once per rendered frame and not
    /// gated by doWeatherCycle. Vanilla's rain update reads rain at alpha
    /// zero (the previous tick), multiplies by .5, then accumulates each cell.
    pub(super) fn tick(&mut self, previous_rain: f32, dimension: i32) {
        // The built-in Overworld admits weather.
        // Custom dimension weather admission is not represented by our protocol.
        let rain = if dimension == 0 && previous_rain.is_finite() {
            previous_rain.clamp(0.0, 1.0) * RAIN_SAMPLE_WEIGHT
        } else {
            0.0
        };
        let mut counter = 0.0;
        for _ in 0..self.precipitation_count {
            counter += rain;
        }
        let target = (counter * FOG_COUNTER_WEIGHT).clamp(0.0, 1.0);
        self.smoothed = self.smoothed * PREVIOUS_FOG_WEIGHT + target * TARGET_FOG_WEIGHT;
    }

    pub(super) fn level(&self, dimension: i32) -> f32 {
        if dimension == 0 { self.smoothed } else { 0.0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_is_a_lattice_sum_not_a_normalized_rain_level() {
        let mut fog = WeatherFog::default();
        fog.set_precipitation_count(Some(PRECIPITATION_SAMPLE_OFFSETS.len()));
        fog.tick(1.0, 0);
        assert_eq!(fog.level(0), TARGET_FOG_WEIGHT);

        let mut partial = WeatherFog::default();
        partial.set_precipitation_count(Some(1));
        partial.tick(1.0, 0);
        assert_eq!(
            partial.level(0),
            RAIN_SAMPLE_WEIGHT * FOG_COUNTER_WEIGHT * TARGET_FOG_WEIGHT
        );
        assert!(partial.level(0) < fog.level(0));
    }

    #[test]
    fn unknown_samples_decay_existing_fog_and_do_not_normalize_survivors() {
        let mut fog = WeatherFog::default();
        fog.set_precipitation_count(Some(1));
        fog.tick(1.0, 0);
        let previous = fog.level(0);
        fog.set_precipitation_count(None);
        fog.tick(1.0, 0);
        assert_eq!(fog.level(0), previous * PREVIOUS_FOG_WEIGHT);
    }

    #[test]
    fn count_and_non_finite_rain_are_bounded() {
        let mut bounded = WeatherFog::default();
        bounded.set_precipitation_count(Some(usize::MAX));
        let mut native = WeatherFog::default();
        native.set_precipitation_count(Some(PRECIPITATION_SAMPLE_OFFSETS.len()));
        for _ in 0..world::TICKS_PER_SECOND {
            bounded.tick(1.0, 0);
            native.tick(1.0, 0);
        }
        assert_eq!(bounded.level(0), native.level(0));
        let previous = bounded.level(0);
        bounded.tick(f32::NAN, 0);
        assert_eq!(bounded.level(0), previous * PREVIOUS_FOG_WEIGHT);
    }

    #[test]
    fn non_weather_dimensions_hide_but_do_not_reset_the_accumulator() {
        let mut fog = WeatherFog::default();
        fog.set_precipitation_count(Some(PRECIPITATION_SAMPLE_OFFSETS.len()));
        fog.tick(1.0, 0);
        let previous = fog.level(0);
        for dimension in [1, 2, -1] {
            assert_eq!(fog.level(dimension), 0.0);
        }
        fog.tick(1.0, 1);
        assert_eq!(fog.level(0), previous * PREVIOUS_FOG_WEIGHT);
        fog.reset_session();
        assert_eq!(fog.level(0), 0.0);
    }
}
