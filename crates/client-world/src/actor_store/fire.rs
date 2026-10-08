//! Records the actor tick of each fire-state transition.

use super::{ActorSnapshot, ActorStatus};

pub(super) const ACTOR_FLAG_ON_FIRE: u32 = 0;
/// Vanilla fire-state retention and default on-fire color ramp/fade duration.
pub const FIRE_FADE_TICKS: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct FireAnimation {
    burning: bool,
    started: Option<u32>,
}

impl FireAnimation {
    pub(super) fn observe(&mut self, burning: bool, age: u32) {
        if self.burning != burning {
            self.burning = burning;
            self.started = Some(age);
        }
    }

    pub(super) fn tick(&mut self, age: u32) {
        // Retain the extinguishing transition until the fire tint finishes fading.
        if !self.burning
            && self
                .elapsed(age)
                .is_some_and(|ticks| ticks >= FIRE_FADE_TICKS)
        {
            self.started = None;
        }
    }

    fn elapsed(self, age: u32) -> Option<u32> {
        self.started.map(|start| age.saturating_sub(start))
    }
}

impl ActorSnapshot {
    #[must_use]
    pub fn is_on_fire(&self) -> bool {
        self.flag(ACTOR_FLAG_ON_FIRE)
    }
}

impl ActorStatus {
    /// Completed ticks since the last fire-state change, including the native fade after
    /// extinguishing. The native `query.on_fire_time` does not interpolate render frames.
    #[must_use]
    pub fn on_fire_time(&self) -> Option<u32> {
        self.fire.elapsed(self.age_ticks)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fire_clock_resets_only_on_transitions_and_expires_after_extinguishing() {
        let mut fire = FireAnimation::default();
        assert_eq!(fire.elapsed(8), None);
        fire.observe(true, 8);
        fire.observe(true, 10);
        assert_eq!(fire.elapsed(12), Some(4));
        fire.tick(20);
        assert_eq!(fire.elapsed(20), Some(12));
        fire.observe(false, 20);
        fire.tick(24);
        assert_eq!(fire.elapsed(24), Some(4));
        fire.tick(25);
        assert_eq!(fire.elapsed(25), None);
        fire.observe(true, 26);
        assert_eq!(fire.elapsed(27), Some(1));
    }
}
