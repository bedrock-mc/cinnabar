//! Movement boosts as tick intervals, so live prediction and replay read the same answer.
//!
//! A server `MovementEffect` and a rocket the client fires while gliding each
//! add a half-open interval starting on the first tick they boost. Live ticks
//! and replayed ticks both ask [`BoostTimeline::boosted`].

use super::effects::{BoostSpan, MovementBoost};

/// Ticks of glide boost the client predicts for its own rocket until a server boost restates it.
const PREDICTED_ROCKET_BOOST_TICKS: u64 = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoostInterval {
    boost: MovementBoost,
    start: u64,
    /// First tick no longer boosted; `None` never ends.
    end: Option<u64>,
}

impl BoostInterval {
    fn contains(self, tick: u64) -> bool {
        tick >= self.start && self.end.is_none_or(|end| tick < end)
    }
}

/// Boost intervals retained while any of their ticks can still be replayed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BoostTimeline {
    intervals: Vec<BoostInterval>,
}

impl BoostTimeline {
    pub(super) fn clear(&mut self) {
        self.intervals.clear();
    }

    /// Whether `boost` applies on `tick`.
    pub(super) fn boosted(&self, boost: MovementBoost, tick: u64) -> bool {
        self.intervals
            .iter()
            .any(|interval| interval.boost == boost && interval.contains(tick))
    }

    /// Sets every boost lane of the input simulated on `tick`.
    pub(super) fn apply(&self, tick: u64, effects: &mut sim::MovementEffects) {
        for boost in [MovementBoost::Glide, MovementBoost::Dolphin] {
            *boost.flag(effects) = self.boosted(boost, tick);
        }
    }

    /// Predicts the glide boost of a rocket fired for the tick `start` onward.
    pub(super) fn predict_rocket(&mut self, start: u64) {
        self.intervals.push(BoostInterval {
            boost: MovementBoost::Glide,
            start,
            end: Some(start + PREDICTED_ROCKET_BOOST_TICKS),
        });
    }

    /// Records a server boost from `start`. Earlier server boosts of its kind and
    /// rockets predicted to start no later than it end at `start`, so ticks before
    /// the authoritative boundary keep their predicted boost.
    pub(super) fn record_server(&mut self, boost: MovementBoost, start: u64, span: BoostSpan) {
        self.intervals.retain_mut(|interval| {
            if interval.boost != boost || interval.start > start {
                return true;
            }
            interval.end = Some(interval.end.map_or(start, |end| end.min(start)));
            interval.end.is_some_and(|end| end > interval.start)
        });
        let end = match span {
            BoostSpan::Ticks(ticks) => Some(start + u64::from(ticks)),
            BoostSpan::Unbounded => None,
        };
        self.intervals.push(BoostInterval { boost, start, end });
    }

    /// Drops intervals that end before `tick`, the first tick a replay can still simulate.
    pub(super) fn forget_before(&mut self, tick: u64) {
        self.intervals
            .retain(|interval| interval.end.is_none_or(|end| end > tick));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glide(timeline: &BoostTimeline) -> Vec<u64> {
        (0..200)
            .filter(|tick| timeline.boosted(MovementBoost::Glide, *tick))
            .collect()
    }

    /// A predicted rocket boosts its first tick and the 19 after it.
    #[test]
    fn a_predicted_rocket_boosts_twenty_ticks_from_its_start() {
        let mut timeline = BoostTimeline::default();
        timeline.predict_rocket(100);
        assert_eq!(glide(&timeline), (100..120).collect::<Vec<_>>());
        assert!(!timeline.boosted(MovementBoost::Dolphin, 100));
    }

    /// A server boost restates rockets starting no later than it, but not newer ones.
    #[test]
    fn a_server_boost_restates_only_rockets_that_start_no_later() {
        let mut timeline = BoostTimeline::default();
        timeline.predict_rocket(101);
        timeline.predict_rocket(120);
        timeline.record_server(MovementBoost::Glide, 101, BoostSpan::Ticks(20));
        assert_eq!(glide(&timeline), (101..140).collect::<Vec<_>>());
    }

    /// A later server boost keeps a rocket's predicted ticks before its own start.
    #[test]
    fn a_later_server_boost_keeps_the_rockets_earlier_ticks() {
        let mut timeline = BoostTimeline::default();
        timeline.predict_rocket(103);
        timeline.record_server(MovementBoost::Glide, 106, BoostSpan::Ticks(20));
        assert_eq!(glide(&timeline), (103..126).collect::<Vec<_>>());
    }

    /// A newer server boost replaces an older one of its kind from its own start.
    #[test]
    fn a_newer_server_boost_replaces_the_older_from_its_start() {
        let mut timeline = BoostTimeline::default();
        timeline.record_server(MovementBoost::Glide, 100, BoostSpan::Unbounded);
        timeline.record_server(MovementBoost::Dolphin, 100, BoostSpan::Unbounded);
        timeline.record_server(MovementBoost::Glide, 110, BoostSpan::Ticks(5));
        assert_eq!(glide(&timeline), (100..115).collect::<Vec<_>>());
        assert!(timeline.boosted(MovementBoost::Dolphin, 150));
    }

    /// Intervals stay until a replay can no longer reach their ticks.
    #[test]
    fn intervals_are_forgotten_only_once_no_replayable_tick_needs_them() {
        let mut timeline = BoostTimeline::default();
        timeline.predict_rocket(100);
        timeline.forget_before(119);
        assert!(timeline.boosted(MovementBoost::Glide, 119));
        timeline.forget_before(120);
        assert_eq!(timeline, BoostTimeline::default());
    }
}
