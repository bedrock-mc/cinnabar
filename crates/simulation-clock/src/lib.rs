//! Shared simulation clock values, scope identities and durations, without a world-model dependency.

use std::time::Duration;

pub mod identity;
mod physics_tick;
pub use physics_tick::PhysicsTick;

/// Simulation ticks per second.
pub const TICKS_PER_SECOND: u32 = 20;

/// Whole-tick frame limit; a frame further behind discards the excess ticks.
pub const MAX_TICKS_PER_FRAME: u32 = 10;

/// Maximum elapsed time admitted by the client timer in one rendered frame.
pub const MAX_FRAME_ELAPSED: std::time::Duration = std::time::Duration::from_millis(100);

/// Duration of one simulation tick.
pub const TICK_DURATION: std::time::Duration = std::time::Duration::from_nanos(
    std::time::Duration::from_secs(1).as_nanos() as u64 / TICKS_PER_SECOND as u64,
);

/// Converts nonnegative float seconds to whole simulation ticks, discarding fractions.
/// Non-finite or unrepresentable results remain unresolved instead of saturating.
pub fn whole_ticks_from_seconds(seconds: f32) -> Option<u32> {
    let ticks = seconds * TICKS_PER_SECOND as f32;
    (ticks.is_finite() && ticks >= 0.0 && f64::from(ticks) <= f64::from(u32::MAX))
        .then_some(ticks as u32)
}

/// Time since a host's fixed origin. Observations from different origins must never mix.
/// Native hosts use an `Instant` origin; browser hosts can use their performance clock.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Moment(Duration);

impl Moment {
    /// Records elapsed time from the host's fixed origin.
    pub const fn from_elapsed(elapsed: Duration) -> Self {
        Self(elapsed)
    }

    /// Returns the elapsed time represented by this observation.
    pub const fn elapsed_from_origin(self) -> Duration {
        self.0
    }

    /// Measures elapsed time, treating a backwards observation as no progress.
    pub const fn saturating_duration_since(self, earlier: Self) -> Duration {
        self.0.saturating_sub(earlier.0)
    }

    /// Advances an observation, returning `None` if its duration cannot be represented.
    pub const fn checked_add(self, duration: Duration) -> Option<Self> {
        match self.0.checked_add(duration) {
            Some(elapsed) => Some(Self(elapsed)),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observations_preserve_elapsed_time_and_saturate_backwards() {
        let start = Moment::from_elapsed(Duration::from_secs(7));
        let later = start.checked_add(Duration::from_millis(16)).unwrap();
        assert_eq!(
            later.saturating_duration_since(start),
            Duration::from_millis(16)
        );
        assert_eq!(start.saturating_duration_since(later), Duration::ZERO);
        assert_eq!(later.elapsed_from_origin(), Duration::from_millis(7016));
        assert!(
            Moment::from_elapsed(Duration::MAX)
                .checked_add(Duration::from_nanos(1))
                .is_none()
        );
    }
}
