//! Monotonic four-timestamp clock estimation and conservative drift correction.

use anyhow::{Result, ensure};
use std::collections::VecDeque;

pub(super) const MAX_PROBE_DELAY_US: u64 = 2_000_000;

#[derive(Clone, Copy, Debug)]
struct Sample {
    offset_us: i64,
    delay_us: u64,
}

#[derive(Default, Debug)]
pub struct Clock {
    samples: VecDeque<Sample>,
    last_local_us: Option<u64>,
    local: bool,
}

impl Clock {
    /// Runs the timeline on the client's monotonic clock until a server probe is answered.
    pub fn local() -> Self {
        Self {
            local: true,
            ..Self::default()
        }
    }

    /// Rejects impossible samples and prefers the least congested recent exchange.
    pub fn observe(&mut self, c0: u64, s1: u64, s2: u64, c3: u64) -> Result<()> {
        ensure!(
            c3 >= c0 && s2 >= s1 && c3 - c0 >= s2 - s1,
            "invalid clock sample"
        );
        let delay_us = (c3 - c0) - (s2 - s1);
        ensure!(delay_us <= MAX_PROBE_DELAY_US, "clock sample too delayed");
        let offset = ((i128::from(s1) - i128::from(c0)) + (i128::from(s2) - i128::from(c3))) / 2;
        let offset_us = i64::try_from(offset)?;
        if self
            .last_local_us
            .is_some_and(|last| c3 < last || c3 - last > 60_000_000)
        {
            self.samples.clear();
        }
        self.last_local_us = Some(c3);
        self.local = false;
        if self.samples.len() == 8 {
            self.samples.pop_front();
        }
        self.samples.push_back(Sample {
            offset_us,
            delay_us,
        });
        Ok(())
    }

    /// Returns server time and the half-RTT uncertainty; stale clocks are unavailable.
    pub fn server_now(&self, local_us: u64) -> Option<(u64, u64)> {
        if self.local {
            return Some((local_us, 0));
        }
        let last = self.last_local_us?;
        if local_us < last || local_us - last > 60_000_000 {
            return None;
        }
        let sample = self.samples.iter().min_by_key(|sample| sample.delay_us)?;
        let now = i128::from(local_us) + i128::from(sample.offset_us);
        Some((u64::try_from(now).ok()?, sample.delay_us / 2))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Correction {
    Hold,
    Rate(f64),
    Seek(u64),
}

/// Small drift uses bounded resampling; large drift rejoins the shared timeline.
pub fn correction(actual_us: u64, desired_us: u64) -> Correction {
    let difference = i128::from(desired_us) - i128::from(actual_us);
    if difference.abs() > 250_000 {
        return Correction::Seek(desired_us);
    }
    if difference.abs() <= 20_000 {
        return Correction::Hold;
    }
    Correction::Rate((1.0 + difference as f64 / 10_000_000.0).clamp(0.995, 1.005))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowest_delay_sample_wins_and_suspend_invalidates_clock() {
        let mut clock = Clock::default();
        clock.observe(1000, 2100, 2200, 1300).unwrap();
        assert_eq!(clock.server_now(1500), Some((2500, 100)));
        clock.observe(2000, 7000, 7100, 5000).unwrap();
        assert_eq!(clock.server_now(6000), Some((7000, 100)));
        assert!(clock.server_now(70_000_000).is_none());
        assert!(clock.observe(10, 10, 20, 11).is_err());
    }

    #[test]
    fn local_clock_never_goes_stale_until_a_server_sample_replaces_it() {
        let mut clock = Clock::local();
        assert_eq!(clock.server_now(90_000_000), Some((90_000_000, 0)));
        clock.observe(1000, 2100, 2200, 1300).unwrap();
        assert_eq!(clock.server_now(1500), Some((2500, 100)));
    }

    #[test]
    fn drift_holds_small_errors_resamples_medium_and_seeks_large() {
        assert_eq!(correction(1_000_000, 1_015_000), Correction::Hold);
        let Correction::Rate(fast) = correction(1_000_000, 1_100_000) else {
            panic!("expected rate correction");
        };
        assert!(fast > 1.0 && fast <= 1.005);
        let Correction::Rate(slow) = correction(1_100_000, 1_000_000) else {
            panic!("expected rate correction");
        };
        assert!((0.995..1.0).contains(&slow));
        assert_eq!(correction(0, 300_000), Correction::Seek(300_000));
    }
}
