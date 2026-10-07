//! The fixed-rate recording clock: one rendered frame advances time by exactly 1/fps.

use std::time::Duration;

const NANOS_PER_SECOND: u128 = 1_000_000_000;

/// Hands out per-frame steps whose running total is always `frames / fps` rounded to the
/// nanosecond, so a 60 fps recording never drifts against its timestamps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixedStepClock {
    fps: u32,
    frames: u64,
}

impl FixedStepClock {
    /// `None` for a zero rate.
    pub fn new(fps: u32) -> Option<Self> {
        (fps > 0).then_some(Self { fps, frames: 0 })
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }

    /// Frames stepped so far.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Simulated time after every step taken so far.
    pub fn elapsed(&self) -> Duration {
        self.at(self.frames)
    }

    /// The interval the next rendered frame advances simulation and animation by.
    pub fn step(&mut self) -> Duration {
        let before = self.at(self.frames);
        self.frames += 1;
        self.at(self.frames) - before
    }

    fn at(&self, frames: u64) -> Duration {
        let nanos = u128::from(frames) * NANOS_PER_SECOND / u128::from(self.fps);
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }
}

/// Maps wall-clock capture onto a fixed output rate: how many output frames a capture at
/// `elapsed` fills, duplicating or dropping frames when real time drives the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RealTimePacer {
    fps: u32,
    written: u64,
}

impl RealTimePacer {
    pub fn new(fps: u32) -> Self {
        Self {
            fps: fps.max(1),
            written: 0,
        }
    }

    /// Output frames to emit for a capture taken `elapsed` after recording started.
    pub fn frames_for(&mut self, elapsed: Duration) -> u64 {
        let due = elapsed.as_nanos() * u128::from(self.fps) / NANOS_PER_SECOND + 1;
        let due = u64::try_from(due).unwrap_or(u64::MAX);
        let count = due.saturating_sub(self.written);
        self.written = self.written.max(due);
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_frame_advances_exactly_one_interval() {
        let mut clock = FixedStepClock::new(60).unwrap();
        let steps: Vec<_> = (0..60).map(|_| clock.step()).collect();
        assert_eq!(clock.elapsed(), Duration::from_secs(1));
        for step in &steps {
            let nanos = step.as_nanos();
            assert!(nanos == 16_666_666 || nanos == 16_666_667, "{nanos}");
        }
        assert_eq!(clock.frames(), 60);
    }

    #[test]
    fn integer_rates_step_identically() {
        let mut clock = FixedStepClock::new(50).unwrap();
        assert!((0..1_000).all(|_| clock.step() == Duration::from_millis(20)));
        assert_eq!(clock.elapsed(), Duration::from_secs(20));
    }

    #[test]
    fn long_recordings_do_not_drift() {
        let mut clock = FixedStepClock::new(60).unwrap();
        let total: Duration = (0..60 * 3_600).map(|_| clock.step()).sum();
        assert_eq!(total, Duration::from_secs(3_600));
        assert!(FixedStepClock::new(0).is_none());
    }

    #[test]
    fn real_time_pacing_duplicates_slow_and_drops_fast_captures() {
        let mut pacer = RealTimePacer::new(60);
        assert_eq!(pacer.frames_for(Duration::ZERO), 1);
        assert_eq!(pacer.frames_for(Duration::from_millis(5)), 0);
        assert_eq!(pacer.frames_for(Duration::from_millis(50)), 3);
        assert_eq!(pacer.frames_for(Duration::from_millis(51)), 0);
    }
}
