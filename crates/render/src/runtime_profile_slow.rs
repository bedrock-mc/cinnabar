//! Always-on attribution using bounded counters, with no formatting on fast frames.

use super::runtime_profile::RuntimeStage;
use std::{
    fmt::Write,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const STAGES: usize = RuntimeStage::ALL.len();
const SLOW_FRAME: Duration = Duration::from_millis(20);
const LOG_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Debug)]
pub(super) struct SlowFrameRecorder {
    totals: [AtomicU64; STAGES],
    frame: Mutex<FrameWindow>,
}

impl Default for SlowFrameRecorder {
    /// Allocates only fixed-size timing state when the client starts.
    fn default() -> Self {
        Self {
            totals: std::array::from_fn(|_| AtomicU64::new(0)),
            frame: Mutex::new(FrameWindow::default()),
        }
    }
}

#[derive(Debug)]
struct FrameWindow {
    started: Option<Instant>,
    baseline: [u64; STAGES],
    main: Option<(Duration, [u64; STAGES])>,
    focused: bool,
    occluded: bool,
    last_log: Option<Instant>,
    suppressed: u64,
    sequence: u64,
}

impl Default for FrameWindow {
    fn default() -> Self {
        Self {
            started: None,
            baseline: [0; STAGES],
            main: None,
            focused: false,
            occluded: false,
            last_log: None,
            suppressed: 0,
            sequence: 0,
        }
    }
}

impl SlowFrameRecorder {
    /// Adds a stage duration without a lock; aggregate and trace recording are separate.
    pub(super) fn record(&self, stage: RuntimeStage, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.totals[stage as usize].fetch_add(nanos, Ordering::Relaxed);
        if stage == RuntimeStage::MainFrame {
            let totals = self.snapshot();
            let mut frame = self
                .frame
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            frame.main = Some((elapsed, delta(totals, frame.baseline)));
        }
    }

    /// Returns cumulative totals, allowing render and worker spans to overlap safely.
    fn snapshot(&self) -> [u64; STAGES] {
        std::array::from_fn(|index| self.totals[index].load(Ordering::Relaxed))
    }

    /// Attributes the preceding start-to-start interval before resetting its baseline.
    pub(super) fn begin_frame(&self, now: Instant, focused: bool, occluded: bool) {
        let totals = self.snapshot();
        let mut frame = self
            .frame
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(line) = frame.advance(now, totals, focused, occluded) {
            eprintln!("{line}");
        }
    }
}

impl FrameWindow {
    /// Formats at most one slow interval per second; fast frames touch only fixed-size state.
    fn advance(
        &mut self,
        now: Instant,
        totals: [u64; STAGES],
        focused: bool,
        occluded: bool,
    ) -> Option<String> {
        let line = self.started.zip(self.main.take()).and_then(|(started, (main, stages))| {
            let interval = now.saturating_duration_since(started);
            if interval < SLOW_FRAME && main < SLOW_FRAME {
                return None;
            }
            if self.last_log.is_some_and(|last| now.saturating_duration_since(last) < LOG_INTERVAL) {
                self.suppressed += 1;
                return None;
            }
            self.last_log = Some(now);
            let window = delta(totals, self.baseline);
            let suppressed = std::mem::take(&mut self.suppressed);
            Some(format!(
                "RUST_MCBE_SLOW_FRAME frame={} threshold_ms={} frame_ms={:.3} main_ms={:.3} between_updates_ms={:.3} focused={} occluded={} suppressed={} main_stages={} window_stages={} scope=overlapping",
                self.sequence, SLOW_FRAME.as_millis(), interval.as_secs_f64() * 1e3,
                main.as_secs_f64() * 1e3, interval.saturating_sub(main).as_secs_f64() * 1e3,
                self.focused, self.occluded, suppressed, stage_fields(stages), stage_fields(window),
            ))
        });
        self.sequence += 1;
        self.started = Some(now);
        self.baseline = totals;
        self.focused = focused;
        self.occluded = occluded;
        line
    }
}

/// Computes window durations from cumulative counters without draining other consumers.
fn delta(totals: [u64; STAGES], baseline: [u64; STAGES]) -> [u64; STAGES] {
    std::array::from_fn(|index| totals[index].wrapping_sub(baseline[index]))
}

/// Lists the largest measured spans first; nested and concurrent spans are not additive.
fn stage_fields(samples: [u64; STAGES]) -> String {
    let mut stages: [_; STAGES] =
        std::array::from_fn(|index| (RuntimeStage::ALL[index], samples[index]));
    stages.sort_unstable_by_key(|(_, nanos)| std::cmp::Reverse(*nanos));
    let mut line = String::new();
    for (stage, nanos) in stages {
        if stage == RuntimeStage::MainFrame || nanos == 0 {
            continue;
        }
        if !line.is_empty() {
            line.push(',');
        }
        let _ = write!(line, "{}:{:.3}", stage.name(), nanos as f64 / 1e6);
    }
    if line.is_empty() {
        line.push_str("none");
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "release benchmark for always-on attribution overhead"]
    fn fast_frame_attribution_cost() {
        for always_on in [false, true] {
            let profiler = if always_on {
                crate::RuntimeStageProfiler::for_gameplay(false, None)
            } else {
                crate::RuntimeStageProfiler::new(false)
            };
            let mut samples = Vec::with_capacity(10_000);
            for _ in 0..10_000 {
                let started = Instant::now();
                profiler.begin_frame(true, false);
                for stage in RuntimeStage::ALL
                    .into_iter()
                    .filter(|stage| *stage != RuntimeStage::MainFrame)
                {
                    drop(profiler.time(stage));
                }
                drop(profiler.time(RuntimeStage::MainFrame));
                samples.push(started.elapsed());
            }
            samples.sort_unstable();
            eprintln!(
                "SLOW_FRAME_OVERHEAD always_on={always_on} n=10000 median_us={:.3} p99_us={:.3}",
                samples[4999].as_secs_f64() * 1e6,
                samples[9899].as_secs_f64() * 1e6
            );
        }
    }

    #[test]
    fn fast_frames_do_not_format_and_slow_frames_are_rate_limited() {
        let now = Instant::now();
        let mut frame = FrameWindow::default();
        assert!(frame.advance(now, [0; STAGES], true, false).is_none());
        frame.main = Some((Duration::from_millis(4), [0; STAGES]));
        assert!(
            frame
                .advance(now + Duration::from_millis(8), [0; STAGES], true, false)
                .is_none()
        );
        let mut stages = [0; STAGES];
        stages[RuntimeStage::UiPublication as usize] = 23_000_000;
        frame.main = Some((Duration::from_millis(25), stages));
        let line = frame
            .advance(now + Duration::from_millis(36), stages, false, true)
            .unwrap();
        assert!(line.contains("main_ms=25.000 between_updates_ms=3.000"));
        assert!(line.contains("ui_publication:23.000"));
        assert!(line.contains("focused=true occluded=false"));
        frame.main = Some((Duration::from_millis(2), [0; STAGES]));
        assert!(
            frame
                .advance(now + Duration::from_millis(66), stages, false, true)
                .is_none()
        );
        frame.main = Some((Duration::from_millis(2), [0; STAGES]));
        let line = frame
            .advance(now + Duration::from_millis(1100), stages, false, true)
            .unwrap();
        assert!(line.contains("suppressed=1"));
        assert!(line.contains("main_ms=2.000 between_updates_ms=1032.000"));
        assert!(line.contains("focused=false occluded=true"));
    }

    #[test]
    fn gameplay_recording_does_not_enable_aggregate_or_trace() {
        let profiler = crate::RuntimeStageProfiler::for_gameplay(false, None);
        profiler.begin_frame(true, false);
        drop(profiler.time(RuntimeStage::UiPublication));
        drop(profiler.time(RuntimeStage::MainFrame));
        assert!(!profiler.enabled());
        assert!(profiler.take_snapshot_if_due(Duration::ZERO).is_none());
    }
}
