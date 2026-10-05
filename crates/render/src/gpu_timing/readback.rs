//! Allocation-free readback bookkeeping and timestamp decoding, independent of the GPU.

use crate::RuntimeStage;
use std::time::Duration;

/// Frames that may be recorded or awaiting readback at once; a full ring skips timing.
pub(super) const SLOTS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SlotState {
    Free,
    Recording,
    /// Submitted for readback; the sequence orders completion oldest first.
    InFlight(u64),
}

/// Readback slot lifecycle: free, recording one frame, then in flight until mapped.
#[derive(Debug)]
pub(super) struct ReadbackRing {
    states: [SlotState; SLOTS],
    next: usize,
    submitted: u64,
}

impl Default for ReadbackRing {
    fn default() -> Self {
        Self {
            states: [SlotState::Free; SLOTS],
            next: 0,
            submitted: 0,
        }
    }
}

impl ReadbackRing {
    /// Claims a free slot round-robin; `None` while every slot awaits readback.
    pub(super) fn acquire(&mut self) -> Option<usize> {
        let slot = (0..SLOTS)
            .map(|offset| (self.next + offset) % SLOTS)
            .find(|slot| self.states[*slot] == SlotState::Free)?;
        self.states[slot] = SlotState::Recording;
        self.next = (slot + 1) % SLOTS;
        Some(slot)
    }

    pub(super) fn submit(&mut self, slot: usize) {
        debug_assert_eq!(self.states[slot], SlotState::Recording);
        self.submitted += 1;
        self.states[slot] = SlotState::InFlight(self.submitted);
    }

    /// Returns a recording or completed slot to the free pool.
    pub(super) fn release(&mut self, slot: usize) {
        self.states[slot] = SlotState::Free;
    }

    /// The earliest submitted slot still awaiting readback.
    pub(super) fn oldest_in_flight(&self) -> Option<usize> {
        (0..SLOTS)
            .filter_map(|slot| match self.states[slot] {
                SlotState::InFlight(sequence) => Some((sequence, slot)),
                _ => None,
            })
            .min()
            .map(|(_, slot)| slot)
    }
}

/// Converts a tick delta with the queue's nanoseconds-per-tick period.
#[must_use]
pub(super) fn ticks_to_duration(ticks: u64, period_ns: f32) -> Duration {
    let nanos = (ticks as f64 * f64::from(period_ns)).round();
    Duration::from_nanos(if nanos >= u64::MAX as f64 {
        u64::MAX
    } else {
        nanos as u64
    })
}

/// One frame's GPU durations per [`RuntimeStage::GPU`] stage; repeated passes are summed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GpuFrameTimes {
    stages: [Option<Duration>; RuntimeStage::GPU.len()],
}

impl GpuFrameTimes {
    /// `None` when the stage is not GPU-timed or did not run in this frame.
    #[must_use]
    pub fn get(&self, stage: RuntimeStage) -> Option<Duration> {
        stage.gpu_index().and_then(|index| self.stages[index])
    }

    /// Measured stages in [`RuntimeStage::GPU`] order.
    pub fn iter(&self) -> impl Iterator<Item = (RuntimeStage, Duration)> + '_ {
        RuntimeStage::GPU
            .into_iter()
            .zip(self.stages)
            .filter_map(|(stage, duration)| duration.map(|duration| (stage, duration)))
    }

    fn add(&mut self, stage: RuntimeStage, duration: Duration) {
        if let Some(index) = stage.gpu_index() {
            let total = self.stages[index].get_or_insert(Duration::ZERO);
            *total = total.saturating_add(duration);
        }
    }
}

/// Sums `(stage, begin, end)` tick spans; unwritten (zero) or reversed spans are skipped.
#[must_use]
pub(crate) fn decode_spans(
    spans: impl IntoIterator<Item = (RuntimeStage, u64, u64)>,
    period_ns: f32,
) -> GpuFrameTimes {
    let mut times = GpuFrameTimes::default();
    let mut bounds: Option<(u64, u64)> = None;
    for (stage, begin, end) in spans {
        if begin == 0 || end < begin {
            continue;
        }
        times.add(stage, ticks_to_duration(end - begin, period_ns));
        bounds = Some(bounds.map_or((begin, end), |(first, last)| {
            (first.min(begin), last.max(end))
        }));
    }
    if let Some((first, last)) = bounds {
        times.add(
            RuntimeStage::GpuFrame,
            ticks_to_duration(last - first, period_ns),
        );
    }
    times
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_ring_skips_frames_instead_of_waiting() {
        let mut ring = ReadbackRing::default();
        let slots: Vec<_> = (0..SLOTS).map(|_| ring.acquire().unwrap()).collect();
        assert_eq!(slots, [0, 1, 2]);
        assert_eq!(ring.acquire(), None);
        for slot in slots {
            ring.submit(slot);
        }
        assert_eq!(ring.acquire(), None);
        assert_eq!(ring.oldest_in_flight(), Some(0));
        ring.release(0);
        assert_eq!(ring.oldest_in_flight(), Some(1));
        assert_eq!(ring.acquire(), Some(0));
    }

    #[test]
    fn readback_completes_in_submission_order_across_wraparound() {
        let mut ring = ReadbackRing::default();
        for _ in 0..SLOTS {
            let slot = ring.acquire().unwrap();
            ring.submit(slot);
        }
        ring.release(0);
        let reused = ring.acquire().unwrap();
        ring.submit(reused);
        assert_eq!(ring.oldest_in_flight(), Some(1));
        ring.release(1);
        ring.release(2);
        assert_eq!(ring.oldest_in_flight(), Some(reused));
    }

    #[test]
    fn released_recording_slot_is_reused_without_readback() {
        let mut ring = ReadbackRing::default();
        let slot = ring.acquire().unwrap();
        ring.release(slot);
        assert_eq!(ring.oldest_in_flight(), None);
        assert!((0..SLOTS).all(|_| ring.acquire().is_some()));
    }

    #[test]
    fn period_converts_ticks_to_nanoseconds() {
        assert_eq!(ticks_to_duration(1_500, 1.0), Duration::from_nanos(1_500));
        // An 83.333 ns period is typical of 12 MHz counters.
        assert_eq!(
            ticks_to_duration(12_000, 83.333_336),
            Duration::from_micros(1_000)
        );
        assert_eq!(
            ticks_to_duration(u64::MAX, 2.0),
            Duration::from_nanos(u64::MAX)
        );
    }

    #[test]
    fn spans_sum_per_stage_and_frame_covers_first_to_last() {
        let times = decode_spans(
            [
                (RuntimeStage::GpuOpaque, 100, 400),
                (RuntimeStage::GpuUi, 450, 500),
                (RuntimeStage::GpuUi, 600, 700),
                (RuntimeStage::GpuTransparent, 0, 0),
                (RuntimeStage::GpuFxaa, 900, 800),
            ],
            2.0,
        );
        assert_eq!(
            times.get(RuntimeStage::GpuOpaque),
            Some(Duration::from_nanos(600))
        );
        assert_eq!(
            times.get(RuntimeStage::GpuUi),
            Some(Duration::from_nanos(300))
        );
        assert_eq!(times.get(RuntimeStage::GpuTransparent), None);
        assert_eq!(times.get(RuntimeStage::GpuFxaa), None);
        assert_eq!(
            times.get(RuntimeStage::GpuFrame),
            Some(Duration::from_nanos(1_200))
        );
        assert_eq!(times.get(RuntimeStage::MainFrame), None);
        assert_eq!(times.iter().count(), 3);
    }

    #[test]
    fn unwritten_frame_reports_nothing() {
        let times = decode_spans([(RuntimeStage::GpuOpaque, 0, 0)], 1.0);
        assert_eq!(times, GpuFrameTimes::default());
    }
}
