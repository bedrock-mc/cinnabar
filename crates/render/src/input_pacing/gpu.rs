//! Predicts GPU queue completion from nonblocking frame markers, keeping input ahead of work.

use super::{HISTORY, History, MARGIN, MAX_DELAY, MEDIAN, PacingModel};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Debug)]
struct Submission {
    id: u64,
    frame: u64,
    at: Instant,
}

/// A bounded queue model; measured completion reanchors predictions instead of accumulating drift.
#[derive(Debug, Default)]
pub(super) struct GpuPacing {
    execution: History,
    pending: VecDeque<Submission>,
    next: u64,
    completed: Option<Instant>,
    display_sync: bool,
}

impl GpuPacing {
    /// Records whether the display owns frame admission and drawable backpressure.
    pub(super) fn set_display_sync(&mut self, display_sync: bool) {
        if self.display_sync != display_sync {
            self.execution = History::default();
            self.pending.clear();
            self.completed = None;
        }
        self.display_sync = display_sync;
    }

    /// Records submission order with bounded storage even if completion feedback stops.
    pub(super) fn submit(&mut self, frame: u64, at: Instant) -> u64 {
        self.next += 1;
        if self.pending.len() == HISTORY {
            self.pending.pop_front();
        }
        self.pending.push_back(Submission {
            id: self.next,
            frame,
            at,
        });
        self.next
    }

    /// Settles in-order work and reanchors remaining predictions to actual GPU completion.
    pub(super) fn complete(&mut self, id: u64, end: Instant, elapsed: Duration) {
        if !self.pending.iter().any(|entry| entry.id == id) {
            return;
        }
        while self.pending.front().is_some_and(|entry| entry.id <= id) {
            self.pending.pop_front();
        }
        self.completed = Some(end);
        if !elapsed.is_zero() && elapsed <= MAX_DELAY {
            self.execution.record(elapsed);
        }
    }

    /// Starts the next update early enough to feed the GPU when the handed frame finishes.
    pub(super) fn deadline(
        &self,
        handed_frame: u64,
        handed: Instant,
        cpu: &PacingModel,
    ) -> Option<Instant> {
        // FIFO markers include display-dependent gaps, not just work that feeds the GPU.
        // Extrapolating those gaps postpones input across additional refreshes.
        if self.display_sync {
            return None;
        }
        let execution = self.execution.quantile(MEDIAN)?;
        let main = cpu.main.quantile(MEDIAN)?;
        let render = cpu.render_work.quantile(MEDIAN)?;
        let mut finish = self.completed?;
        if handed.saturating_duration_since(finish) > MAX_DELAY {
            return None;
        }
        for entry in self
            .pending
            .iter()
            .filter(|entry| entry.frame < handed_frame)
        {
            finish = finish.max(entry.at) + execution;
        }
        // The handed frame may already have submitted on the other thread. Predict it once.
        finish = finish.max(handed + render) + execution;
        let deadline = finish.checked_sub(main + render + MARGIN)?;
        (deadline > handed).then_some(deadline.min(handed + MAX_DELAY))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Seeds both CPU and GPU histories without a wall-clock wait.
    fn model(now: Instant) -> (GpuPacing, PacingModel) {
        let mut gpu = GpuPacing::default();
        let mut cpu = PacingModel::default();
        for frame in 1..=HISTORY as u64 {
            cpu.main.record(Duration::from_millis(1));
            cpu.render.record(Duration::from_millis(1));
            cpu.render_work.record(Duration::from_millis(1));
            let id = gpu.submit(frame, now);
            gpu.complete(id, now, Duration::from_millis(4));
        }
        (gpu, cpu)
    }

    #[test]
    fn cpu_completion_does_not_hide_pending_gpu_work() {
        let now = Instant::now();
        let (mut gpu, cpu) = model(now);
        gpu.submit(33, now);
        assert_eq!(
            gpu.deadline(34, now, &cpu),
            Some(now + Duration::from_millis(5))
        );
        // Racing submission of the handed frame must not count it twice.
        gpu.submit(34, now + Duration::from_millis(1));
        assert_eq!(
            gpu.deadline(34, now, &cpu),
            Some(now + Duration::from_millis(5))
        );
    }

    #[test]
    fn measured_completion_reanchors_without_repeated_correction() {
        let now = Instant::now();
        let (mut gpu, cpu) = model(now);
        let first = gpu.submit(33, now);
        let second = gpu.submit(34, now + Duration::from_millis(1));
        gpu.complete(
            first,
            now + Duration::from_millis(5),
            Duration::from_millis(4),
        );
        let deadline = gpu.deadline(35, now + Duration::from_millis(2), &cpu);
        gpu.complete(
            second,
            now + Duration::from_millis(9),
            Duration::from_millis(4),
        );
        assert_eq!(
            deadline,
            gpu.deadline(35, now + Duration::from_millis(2), &cpu)
        );
        assert_eq!(deadline, Some(now + Duration::from_millis(10)));
    }

    #[test]
    fn stale_or_missing_feedback_never_blocks_input() {
        let now = Instant::now();
        let (mut gpu, cpu) = model(now);
        assert_eq!(gpu.deadline(33, now + MAX_DELAY * 2, &cpu), None);
        for frame in 33..333 {
            gpu.submit(frame, now);
        }
        assert_eq!(gpu.pending.len(), HISTORY);
        let empty = GpuPacing::default();
        assert_eq!(empty.deadline(1, now, &cpu), None);
    }

    #[test]
    fn fifo_feedback_never_adds_a_gpu_admission_deadline() {
        let now = Instant::now();
        let (mut gpu, cpu) = model(now);
        gpu.set_display_sync(true);
        for frame in 33..65 {
            let id = gpu.submit(frame, now);
            gpu.complete(id, now, Duration::from_millis(20));
        }
        gpu.submit(65, now);
        assert_eq!(gpu.deadline(66, now, &cpu), None);
    }

    #[test]
    fn changing_display_sync_drops_prediction_history_and_stale_feedback() {
        let now = Instant::now();
        let (mut gpu, cpu) = model(now);
        gpu.set_display_sync(true);
        for frame in 33..65 {
            let id = gpu.submit(frame, now);
            gpu.complete(id, now, Duration::from_millis(20));
        }
        let stale = gpu.submit(65, now);
        gpu.set_display_sync(false);
        let fresh = gpu.submit(66, now);
        assert!(
            fresh > stale,
            "old feedback must not alias a new submission"
        );
        gpu.complete(stale, now, Duration::from_millis(20));
        assert_eq!(gpu.pending.len(), 1);
        assert_eq!(gpu.deadline(67, now, &cpu), None);
        for frame in 67..99 {
            let id = gpu.submit(frame, now);
            gpu.complete(id, now, Duration::from_millis(4));
        }
        assert_eq!(
            gpu.deadline(99, now, &cpu),
            Some(now + Duration::from_millis(2))
        );
    }
}
