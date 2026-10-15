//! Requested allocations, not live heap usage. Frames and schedule spans count all threads,
//! including unrelated concurrent work. RAII timers count nested work on their calling thread.
//! Global snapshots may straddle concurrent requests; nested spans deliberately overlap.

use crate::RuntimeStage;
use std::{fmt::Write, sync::Mutex};
use test_alloc::Allocations;

const STAGES: usize = RuntimeStage::ALL.len();

/// Calling thread and its allocation baseline; cross-thread finishes are not comparable.
#[derive(Debug, Clone, Copy)]
pub(super) struct ThreadSnapshot {
    thread: std::thread::ThreadId,
    sample: Allocations,
}

/// Reads this thread, including reallocations at their requested new size.
pub(super) fn thread_snapshot() -> ThreadSnapshot {
    ThreadSnapshot {
        thread: std::thread::current().id(),
        sample: Allocations {
            calls: test_alloc::thread_allocations() as usize,
            bytes: test_alloc::thread_allocated_bytes() as usize,
        },
    }
}

impl ThreadSnapshot {
    /// Returns the inclusive delta only if the span closes on its starting thread.
    fn elapsed(self) -> Option<Allocations> {
        let now = thread_snapshot();
        (now.thread == self.thread).then(|| delta(now.sample, self.sample))
    }
}

/// Computes request deltas without resetting counters shared with nested measurements.
pub(super) fn delta(now: Allocations, before: Allocations) -> Allocations {
    Allocations {
        calls: now.calls.wrapping_sub(before.calls),
        bytes: now.bytes.wrapping_sub(before.bytes),
    }
}

/// Inclusive allocation totals for all timed CPU stages in one frame window.
#[derive(Debug, Default)]
pub(super) struct AllocationProfiler(Mutex<Window>);

#[derive(Debug)]
struct Window {
    baseline: Option<Allocations>,
    stages: [Allocations; STAGES],
}

impl Default for Window {
    fn default() -> Self {
        Self {
            baseline: None,
            stages: [Allocations::default(); STAGES],
        }
    }
}

/// Closed process-wide interval and inclusive stage totals from schedule spans and timers.
#[derive(Debug)]
pub(super) struct AllocationFrame {
    total: Option<Allocations>,
    stages: [Allocations; STAGES],
}

impl AllocationProfiler {
    /// Records process-wide work between schedule boundaries, even when their threads differ.
    pub(super) fn record_global_since(&self, stage: RuntimeStage, before: Allocations) {
        self.record(stage, delta(test_alloc::global_allocations(), before));
    }

    /// Records a span's calling-thread work, skipping spans that moved to another thread.
    pub(super) fn record_since(&self, stage: RuntimeStage, before: ThreadSnapshot) {
        if let Some(sample) = before.elapsed() {
            self.record(stage, sample);
        }
    }

    /// Adds one timed CPU span; nested spans deliberately overlap.
    pub(super) fn record(&self, stage: RuntimeStage, sample: Allocations) {
        let mut window = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let total = &mut window.stages[stage as usize];
        total.calls = total.calls.saturating_add(sample.calls);
        total.bytes = total.bytes.saturating_add(sample.bytes);
    }

    /// Starts the next process interval and drains the stage samples exactly once.
    pub(super) fn finish_frame(&self) -> AllocationFrame {
        let now = test_alloc::global_allocations();
        let mut window = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        AllocationFrame {
            total: window
                .baseline
                .replace(now)
                .map(|before| delta(now, before)),
            stages: std::mem::replace(&mut window.stages, [Allocations::default(); STAGES]),
        }
    }
}

impl AllocationFrame {
    /// Appends allocation fields only when the slow-frame recorder emits a line.
    pub(super) fn append_fields(&self, line: &mut String) {
        if let Some(total) = self.total {
            let _ = write!(
                line,
                " alloc_calls={} alloc_bytes={}",
                total.calls, total.bytes
            );
        }
        line.push_str(" stage_allocations=");
        let mut separator = "";
        for (stage, sample) in RuntimeStage::ALL.into_iter().zip(self.stages) {
            if sample.calls == 0 {
                continue;
            }
            let _ = write!(
                line,
                "{separator}{}:{}:{}",
                stage.name(),
                sample.calls,
                sample.bytes
            );
            separator = ",";
        }
        line.push_str(" alloc_scope=process_frame+inclusive_process_spans+inclusive_thread_timers");
    }

    /// Publishes a complete window, including zero samples after previously busy stages.
    pub(super) fn plot(&self) {
        #[cfg(feature = "tracy")]
        if let Some(client) = tracy_client::Client::running() {
            static NAMES: std::sync::OnceLock<[[tracy_client::PlotName; 2]; STAGES]> =
                std::sync::OnceLock::new();
            let names = NAMES.get_or_init(|| {
                std::array::from_fn(|index| {
                    let stage = RuntimeStage::ALL[index].name();
                    [
                        tracy_client::PlotName::new_leak(format!("alloc/{stage}/calls")),
                        tracy_client::PlotName::new_leak(format!("alloc/{stage}/bytes")),
                    ]
                })
            });
            if let Some(total) = self.total {
                client.plot(
                    tracy_client::plot_name!("alloc/frame/calls"),
                    total.calls as f64,
                );
                client.plot(
                    tracy_client::plot_name!("alloc/frame/bytes"),
                    total.bytes as f64,
                );
            }
            for (names, sample) in names.iter().zip(self.stages) {
                client.plot(names[0], sample.calls as f64);
                client.plot(names[1], sample.bytes as f64);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Nested measurements retain inclusive counts, and a second window drains to zero.
    #[test]
    fn stage_counts_include_nested_requests_without_double_draining() {
        let profiler = AllocationProfiler::default();
        let before = thread_snapshot();
        let bytes = std::hint::black_box(vec![0_u8; 64]);
        let inner = before.elapsed().unwrap();
        profiler.record(RuntimeStage::UiPreparation, inner);
        profiler.record(RuntimeStage::MainFrame, before.elapsed().unwrap());
        std::hint::black_box(bytes);
        let frame = profiler.finish_frame();
        assert_eq!(
            frame.stages[RuntimeStage::UiPreparation as usize],
            Allocations {
                calls: 1,
                bytes: 64
            }
        );
        assert_eq!(frame.stages[RuntimeStage::MainFrame as usize], inner);
        assert_eq!(
            profiler.finish_frame().stages,
            [Allocations::default(); STAGES]
        );
        let mut fields = String::new();
        frame.append_fields(&mut fields);
        assert!(fields.contains("ui_preparation:1:64"));
    }
}
