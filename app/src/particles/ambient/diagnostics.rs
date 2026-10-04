//! Bounded opt-in observations; these do not change native admission or random draws.

use std::time::Duration;

use particles::ParticleSystem;

const REPORT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(super) struct Diagnostics {
    pub(super) enabled: bool,
    pub(super) samples: u64,
    pub(super) eligible: u64,
    pub(super) roll_hits: u64,
    pub(super) below_admitted: u64,
    pub(super) accepted_requests: u64,
    /// A table eviction can hide an emission in the total-count delta. Never call
    /// this exact per-effect emission or interpret an emitter ID as one particle.
    pub(super) emitted_lower_bound: u64,
    ticks: usize,
    elapsed: Duration,
}

impl Diagnostics {
    pub(super) fn report(
        &mut self,
        elapsed: Duration,
        ticks: usize,
        valid: [bool; 2],
        next_samples_per_tick: u32,
        system: &ParticleSystem,
    ) {
        if !self.enabled {
            *self = Self::default();
            return;
        }
        self.elapsed = self.elapsed.saturating_add(elapsed);
        self.ticks = self.ticks.saturating_add(ticks);
        if !self.report_due() {
            return;
        }
        bevy::log::debug!(target: "bedrock_client::ambient_leaves",
            ticks = self.ticks, samples = self.samples, eligible = self.eligible,
            roll_hits = self.roll_hits, below_admitted = self.below_admitted,
            accepted_requests = self.accepted_requests,
            emitted_lower_bound = self.emitted_lower_bound,
            all_live_particles = system.live_particles(),
            next_samples_per_tick, effect_present = valid[0], view_valid = valid[1],
            "ambient falling-leaf admission witness");
        *self = Self {
            enabled: true,
            ..Self::default()
        };
    }

    fn report_due(&self) -> bool {
        self.elapsed >= REPORT_INTERVAL
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reporting_is_bounded_and_has_no_native_tick_or_random_clock() {
        let mut diagnostics = Diagnostics::default();
        assert!(!diagnostics.report_due());
        diagnostics.elapsed = REPORT_INTERVAL - Duration::from_nanos(1);
        assert!(!diagnostics.report_due());
        diagnostics.elapsed = REPORT_INTERVAL;
        assert!(diagnostics.report_due());
        diagnostics.elapsed = Duration::MAX;
        assert!(diagnostics.report_due());
    }
}
