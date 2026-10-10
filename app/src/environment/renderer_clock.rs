//! The level renderer's local tick counter.
//! It advances independently of named daylight clocks and server world age.

use super::numeric::finite_nonnegative;

#[derive(Default)]
pub(crate) struct RendererClock {
    generation: Option<u64>,
    last_elapsed: f64,
    ticks: u64,
    remainder: f64,
}

impl RendererClock {
    pub(super) fn advance(&mut self, generation: Option<u64>, elapsed: f64) -> f64 {
        let elapsed = finite_nonnegative(elapsed);
        if self.generation != generation {
            *self = Self {
                generation,
                last_elapsed: elapsed,
                ..Self::default()
            };
        }
        let delta = (elapsed - self.last_elapsed).max(0.0);
        self.last_elapsed = self.last_elapsed.max(elapsed);
        if generation.is_none() {
            return 0.0;
        }
        self.remainder += delta;
        let tick_seconds = world::TICK_DURATION.as_secs_f64();
        let due = ((self.remainder + f64::EPSILON) / tick_seconds).floor();
        self.remainder = (self.remainder - due * tick_seconds).max(0.0);
        self.ticks = self.ticks.saturating_add(
            (due as u64).min(gameplay::movement::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as u64),
        );
        self.ticks as f64 + self.remainder / tick_seconds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_clock_does_not_import_world_age_or_daylight_state() {
        let mut clock = RendererClock::default();
        assert_eq!(clock.advance(None, 100.0), 0.0);
        assert_eq!(clock.advance(Some(1), 100.0), 0.0);
        assert!((clock.advance(Some(1), 100.125) - 2.5).abs() < 1e-9);
        assert!((clock.advance(Some(1), 100.25) - 5.0).abs() < 1e-9);
        assert_eq!(clock.advance(Some(2), 101.0), 0.0);
    }

    #[test]
    fn renderer_catch_up_is_bounded_and_backward_time_never_reverses_clouds() {
        let mut clock = RendererClock::default();
        clock.advance(Some(1), 0.0);
        let ticks = clock.advance(Some(1), 100.0);
        assert_eq!(
            ticks,
            gameplay::movement::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME as f64
        );
        assert_eq!(clock.advance(Some(1), 99.0), ticks);
        assert_eq!(clock.advance(Some(1), 100.0), ticks);
    }
}
