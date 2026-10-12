//! How much of each frame a frame-driven pool's world jobs may take.

use std::time::Duration;
use web_time::Instant;

/// Least time a frame gives world jobs, so terrain keeps loading on a fully loaded frame.
const MIN_JOB_BUDGET: Duration = Duration::from_millis(1);
/// Most time a frame gives world jobs, however idle it is.
const MAX_JOB_BUDGET: Duration = Duration::from_millis(12);
/// Share of the display interval a frame may keep the thread busy; the rest is the browser's.
const BUSY_SHARE_PERCENT: u32 = 80;
/// Frame intervals outside this range, such as a paused tab, say nothing about the display.
const INTERVAL_RANGE: (Duration, Duration) = (Duration::from_millis(4), Duration::from_millis(50));
/// The display interval estimate tracks the shortest recent interval and rises this slowly.
const INTERVAL_RISE_DIVISOR: u32 = 64;

/// Sizes each frame's world-job budget from the display interval and the frame's other work,
/// for a pool whose jobs run on the frame's own thread.
#[derive(Clone, Copy, Debug)]
pub struct FrameJobBudget {
    interval: Duration,
    other_work: Duration,
    last_start: Option<Instant>,
}

impl Default for FrameJobBudget {
    fn default() -> Self {
        Self {
            interval: Duration::from_micros(16_667),
            other_work: Duration::from_millis(8),
            last_start: None,
        }
    }
}

impl FrameJobBudget {
    /// Records that a frame started at `now` and returns how long its world jobs may run.
    pub fn begin_frame(&mut self, now: Instant) -> Duration {
        if let Some(observed) = self
            .last_start
            .replace(now)
            .map(|last| now.saturating_duration_since(last))
            .filter(|observed| (INTERVAL_RANGE.0..=INTERVAL_RANGE.1).contains(observed))
        {
            self.interval = if observed < self.interval {
                observed
            } else {
                self.interval + (observed - self.interval) / INTERVAL_RISE_DIVISOR
            };
        }
        (self.interval * BUSY_SHARE_PERCENT / 100)
            .saturating_sub(self.other_work)
            .clamp(MIN_JOB_BUDGET, MAX_JOB_BUDGET)
    }

    /// Records a finished frame that kept the thread busy for `busy`, `jobs` of it in world jobs.
    /// Growth in the frame's other work counts at once and a drop only gradually, so one quiet
    /// frame does not hand the next a budget it cannot afford.
    pub fn end_frame(&mut self, busy: Duration, jobs: Duration) {
        let other = busy.saturating_sub(jobs);
        self.other_work = if other > self.other_work {
            other
        } else {
            self.other_work - (self.other_work - other) / 8
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// Runs `count` frames `interval` apart, each with `other` work besides its jobs, and returns
    /// the last budget and start.
    fn frames(
        budget: &mut FrameJobBudget,
        mut now: Instant,
        interval: Duration,
        other: Duration,
        count: u32,
    ) -> (Duration, Instant) {
        let mut granted = Duration::ZERO;
        for _ in 0..count {
            now += interval;
            granted = budget.begin_frame(now);
            budget.end_frame(other + granted, granted);
        }
        (granted, now)
    }

    #[test]
    fn jobs_get_the_frame_left_after_other_work_and_none_of_an_overrun_frame() {
        let mut budget = FrameJobBudget::default();
        let (settled, now) = frames(&mut budget, Instant::now(), 16 * MS, 5 * MS, 60);
        // 80% of the 16 ms interval, less 5 ms of other work.
        assert!(settled <= Duration::from_micros(7_800) && settled > 7 * MS);
        budget.end_frame(20 * MS, 4 * MS);
        assert_eq!(budget.begin_frame(now + 16 * MS), MIN_JOB_BUDGET);
    }

    #[test]
    fn a_faster_display_shrinks_the_budget_and_a_paused_tab_changes_nothing() {
        let mut budget = FrameJobBudget::default();
        let (at_60, now) = frames(&mut budget, Instant::now(), 16 * MS, 2 * MS, 60);
        let (at_120, now) = frames(&mut budget, now, 8 * MS, 2 * MS, 60);
        assert!(at_120 < at_60);
        assert_eq!(budget.begin_frame(now + Duration::from_secs(5)), at_120);
    }
}
