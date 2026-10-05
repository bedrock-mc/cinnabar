//! Always-on attribution using bounded counters, with no formatting on fast frames.

use super::runtime_profile::RuntimeStage;
use std::{
    fmt::Write,
    sync::{
        Mutex,
        atomic::{AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

const STAGES: usize = RuntimeStage::ALL.len();
const LOG_INTERVAL: Duration = Duration::from_secs(1);
/// Display rate assumed until the window reports its monitor.
const DEFAULT_REFRESH_HZ: f64 = 60.0;
/// Per-frame (p99) stage budgets at 120 Hz; other rates scale them by 120/H.
const MAIN_CPU_AT_120HZ: Duration = Duration::from_millis(4);
const RENDER_CPU_AT_120HZ: Duration = Duration::from_millis(4);
const GPU_AT_120HZ: Duration = Duration::from_millis(6);

const INTERVAL: u8 = 1;
const MAIN: u8 = 1 << 1;
const RENDER: u8 = 1 << 2;
const GPU: u8 = 1 << 3;
const REASONS: [(u8, &str); 4] = [
    (INTERVAL, "interval"),
    (MAIN, "main"),
    (RENDER, "render"),
    (GPU, "gpu"),
];

/// Per-frame limits for one display interval T.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameBudgets {
    pub interval: Duration,
    /// A start-to-start interval over 1.1T is a slow frame.
    pub slow: Duration,
    /// Over 1.5T is a hitch; 2T or more is a hard hitch.
    pub hitch: Duration,
    pub hard_hitch: Duration,
    pub main_cpu: Duration,
    pub render_cpu: Duration,
    pub gpu: Duration,
}

impl FrameBudgets {
    #[must_use]
    pub fn for_interval(interval: Duration) -> Self {
        let scale = interval.as_secs_f64() * 120.0;
        Self {
            interval,
            slow: interval.mul_f64(1.1),
            hitch: interval.mul_f64(1.5),
            hard_hitch: interval.saturating_mul(2),
            main_cpu: MAIN_CPU_AT_120HZ.mul_f64(scale),
            render_cpu: RENDER_CPU_AT_120HZ.mul_f64(scale),
            gpu: GPU_AT_120HZ.mul_f64(scale),
        }
    }

    /// The paced display interval: the monitor's refresh period, or a slower frame cap.
    #[must_use]
    pub fn display_interval(
        refresh_millihertz: Option<u32>,
        frame_cap: Option<Duration>,
    ) -> Duration {
        let refresh = Duration::from_secs_f64(
            refresh_millihertz
                .filter(|rate| *rate > 0)
                .map_or(1.0 / DEFAULT_REFRESH_HZ, |rate| 1_000.0 / f64::from(rate)),
        );
        frame_cap.map_or(refresh, |cap| cap.max(refresh))
    }

    fn for_nanos(nanos: u64) -> Self {
        Self::for_interval(if nanos == 0 {
            Self::display_interval(None, None)
        } else {
            Duration::from_nanos(nanos)
        })
    }
}

/// Cumulative slow-frame events; text is rate-limited, but every event is counted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SlowFrameCounts {
    pub slow: u64,
    pub interval: u64,
    pub main: u64,
    pub render: u64,
    pub gpu: u64,
    pub hitches: u64,
    pub hard_hitches: u64,
}

impl SlowFrameCounts {
    fn record(&mut self, reasons: u8) {
        self.slow += 1;
        for (bit, count) in [
            (INTERVAL, &mut self.interval),
            (MAIN, &mut self.main),
            (RENDER, &mut self.render),
            (GPU, &mut self.gpu),
        ] {
            *count += u64::from(reasons & bit != 0);
        }
    }
}

/// One slow frame, kept in the bounded trace even when its text is suppressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SlowFrameEvent {
    pub(super) reasons: u8,
    pub(super) frame: Duration,
}

impl SlowFrameEvent {
    pub(super) fn reasons(&self) -> String {
        reason_list(self.reasons)
    }
}

#[derive(Debug)]
pub(super) struct SlowFrameRecorder {
    totals: [AtomicU64; STAGES],
    /// Display interval in nanoseconds; zero until the window reports one.
    interval_nanos: AtomicU64,
    /// Render and GPU budget violations since the last frame boundary.
    violations: AtomicU8,
    frame: Mutex<FrameWindow>,
}

impl Default for SlowFrameRecorder {
    /// Allocates only fixed-size timing state when the client starts.
    fn default() -> Self {
        Self {
            totals: std::array::from_fn(|_| AtomicU64::new(0)),
            interval_nanos: AtomicU64::new(0),
            violations: AtomicU8::new(0),
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
    counts: SlowFrameCounts,
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
            counts: SlowFrameCounts::default(),
        }
    }
}

impl SlowFrameRecorder {
    /// Adds a stage duration without a lock; aggregate and trace recording are separate.
    pub(super) fn record(&self, stage: RuntimeStage, elapsed: Duration) {
        let nanos = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX);
        self.totals[stage as usize].fetch_add(nanos, Ordering::Relaxed);
        match stage {
            RuntimeStage::MainFrame => {
                let totals = self.snapshot();
                let mut frame = self
                    .frame
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                frame.main = Some((elapsed, delta(totals, frame.baseline)));
            }
            RuntimeStage::RenderFrame if elapsed > self.budgets().render_cpu => {
                self.violations.fetch_or(RENDER, Ordering::Relaxed);
            }
            RuntimeStage::GpuFrame if elapsed > self.budgets().gpu => {
                self.violations.fetch_or(GPU, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    pub(super) fn set_interval(&self, interval: Duration) {
        let nanos = u64::try_from(interval.as_nanos()).unwrap_or(u64::MAX);
        self.interval_nanos.store(nanos, Ordering::Relaxed);
    }

    fn budgets(&self) -> FrameBudgets {
        FrameBudgets::for_nanos(self.interval_nanos.load(Ordering::Relaxed))
    }

    pub(super) fn counts(&self) -> SlowFrameCounts {
        self.frame
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .counts
    }

    /// Returns cumulative totals, allowing render and worker spans to overlap safely.
    fn snapshot(&self) -> [u64; STAGES] {
        std::array::from_fn(|index| self.totals[index].load(Ordering::Relaxed))
    }

    /// Attributes the preceding start-to-start interval before resetting its baseline.
    pub(super) fn begin_frame(
        &self,
        now: Instant,
        focused: bool,
        occluded: bool,
    ) -> Option<SlowFrameEvent> {
        let totals = self.snapshot();
        let budgets = self.budgets();
        let violations = self.violations.swap(0, Ordering::Relaxed);
        let mut frame = self
            .frame
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (event, line) = frame.advance(now, totals, focused, occluded, &budgets, violations)?;
        if let Some(line) = line {
            eprintln!("{line}");
        }
        Some(event)
    }
}

impl FrameWindow {
    /// Counts every slow interval and formats at most one per second; fast frames touch only
    /// fixed-size state.
    fn advance(
        &mut self,
        now: Instant,
        totals: [u64; STAGES],
        focused: bool,
        occluded: bool,
        budgets: &FrameBudgets,
        violations: u8,
    ) -> Option<(SlowFrameEvent, Option<String>)> {
        let outcome = self.started.zip(self.main.take()).and_then(|(started, (main, stages))| {
            let interval = now.saturating_duration_since(started);
            self.counts.hitches += u64::from(interval > budgets.hitch);
            self.counts.hard_hitches += u64::from(interval >= budgets.hard_hitch);
            let mut reasons = violations;
            if interval > budgets.slow {
                reasons |= INTERVAL;
            }
            if main > budgets.main_cpu {
                reasons |= MAIN;
            }
            if reasons == 0 {
                return None;
            }
            self.counts.record(reasons);
            let event = SlowFrameEvent { reasons, frame: interval };
            if self.last_log.is_some_and(|last| now.saturating_duration_since(last) < LOG_INTERVAL) {
                self.suppressed += 1;
                return Some((event, None));
            }
            self.last_log = Some(now);
            let window = delta(totals, self.baseline);
            let suppressed = std::mem::take(&mut self.suppressed);
            Some((event, Some(format!(
                "RUST_MCBE_SLOW_FRAME frame={} refresh_hz={:.2} threshold_ms={:.2} frame_ms={:.3} main_ms={:.3} between_updates_ms={:.3} violations={} focused={} occluded={} suppressed={} slow_frames={} hitches={} hard_hitches={} main_stages={} window_stages={} scope=overlapping",
                self.sequence, 1.0 / budgets.interval.as_secs_f64(), budgets.slow.as_secs_f64() * 1e3,
                interval.as_secs_f64() * 1e3, main.as_secs_f64() * 1e3,
                interval.saturating_sub(main).as_secs_f64() * 1e3, reason_list(reasons),
                self.focused, self.occluded, suppressed, self.counts.slow, self.counts.hitches,
                self.counts.hard_hitches, stage_fields(stages), stage_fields(window),
            ))))
        });
        self.sequence += 1;
        self.started = Some(now);
        self.baseline = totals;
        self.focused = focused;
        self.occluded = occluded;
        outcome
    }
}

/// Joins the violated budgets, for example `interval+gpu`.
fn reason_list(reasons: u8) -> String {
    let mut list = String::new();
    for (bit, name) in REASONS {
        if reasons & bit != 0 {
            if !list.is_empty() {
                list.push('+');
            }
            list.push_str(name);
        }
    }
    list
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

    fn at_hz(hz: f64) -> FrameBudgets {
        FrameBudgets::for_interval(Duration::from_secs_f64(1.0 / hz))
    }

    fn ms(duration: Duration) -> String {
        format!("{:.2}", duration.as_secs_f64() * 1e3)
    }

    #[test]
    fn thresholds_scale_with_refresh_rate() {
        let (high, low, fast) = (at_hz(120.0), at_hz(60.0), at_hz(240.0));
        assert_eq!(ms(high.slow), "9.17");
        assert_eq!(ms(low.slow), "18.33");
        assert_eq!(ms(fast.slow), "4.58");
        assert_eq!(
            [high.main_cpu, high.render_cpu, high.gpu].map(ms),
            ["4.00", "4.00", "6.00"]
        );
        assert_eq!(
            [low.main_cpu, low.render_cpu, low.gpu].map(ms),
            ["8.00", "8.00", "12.00"]
        );
        assert_eq!(
            [fast.main_cpu, fast.render_cpu, fast.gpu].map(ms),
            ["2.00", "2.00", "3.00"]
        );
        assert_eq!([ms(high.hitch), ms(high.hard_hitch)], ["12.50", "16.67"]);
    }

    #[test]
    fn display_interval_uses_monitor_or_slower_cap() {
        let interval = FrameBudgets::display_interval;
        assert_eq!(ms(interval(Some(120_000), None)), "8.33");
        assert_eq!(ms(interval(None, None)), "16.67");
        assert_eq!(ms(interval(Some(0), None)), "16.67");
        let cap = Duration::from_secs_f64(1.0 / 30.0);
        assert_eq!(interval(Some(144_000), Some(cap)), cap);
        assert_eq!(
            ms(interval(Some(60_000), Some(Duration::from_millis(5)))),
            "16.67"
        );
    }

    #[test]
    fn interval_trigger_follows_refresh_rate() {
        let now = Instant::now();
        for (budgets, interval, slow) in [
            (at_hz(120.0), 9.0, false),
            (at_hz(120.0), 9.3, true),
            (at_hz(60.0), 18.0, false),
            (at_hz(60.0), 18.5, true),
        ] {
            let mut frame = FrameWindow::default();
            assert!(
                frame
                    .advance(now, [0; STAGES], true, false, &budgets, 0)
                    .is_none()
            );
            frame.main = Some((Duration::from_millis(1), [0; STAGES]));
            let event = frame.advance(
                now + Duration::from_secs_f64(interval / 1e3),
                [0; STAGES],
                true,
                false,
                &budgets,
                0,
            );
            assert_eq!(
                event.is_some(),
                slow,
                "{interval} ms at {:?}",
                budgets.interval
            );
            assert_eq!(frame.counts.interval, u64::from(slow));
        }
    }

    #[test]
    fn stage_budget_violations_trigger_within_the_interval() {
        let recorder = SlowFrameRecorder::default();
        recorder.set_interval(Duration::from_secs_f64(1.0 / 120.0));
        let now = Instant::now();
        assert!(recorder.begin_frame(now, true, false).is_none());
        recorder.record(RuntimeStage::RenderFrame, Duration::from_millis(3));
        recorder.record(RuntimeStage::GpuFrame, Duration::from_millis(7));
        recorder.record(RuntimeStage::MainFrame, Duration::from_millis(5));
        let event = recorder
            .begin_frame(now + Duration::from_millis(6), true, false)
            .unwrap();
        assert_eq!(event.reasons(), "main+gpu");
        recorder.record(RuntimeStage::RenderFrame, Duration::from_millis(5));
        recorder.record(RuntimeStage::MainFrame, Duration::from_millis(1));
        let event = recorder
            .begin_frame(now + Duration::from_millis(12), true, false)
            .unwrap();
        assert_eq!(event.reasons(), "render");
        recorder.record(RuntimeStage::MainFrame, Duration::from_millis(1));
        assert!(
            recorder
                .begin_frame(now + Duration::from_millis(18), true, false)
                .is_none()
        );
        let counts = recorder.counts();
        assert_eq!(
            [
                counts.slow,
                counts.main,
                counts.render,
                counts.gpu,
                counts.interval
            ],
            [2, 1, 1, 1, 0]
        );
    }

    #[test]
    fn fast_frames_do_not_format_and_slow_frames_are_rate_limited() {
        let budgets = at_hz(60.0);
        let now = Instant::now();
        let mut frame = FrameWindow::default();
        let advance = |frame: &mut FrameWindow, at: u64, stages| {
            frame.advance(
                now + Duration::from_millis(at),
                stages,
                at % 2 == 0,
                false,
                &budgets,
                0,
            )
        };
        assert!(advance(&mut frame, 0, [0; STAGES]).is_none());
        frame.main = Some((Duration::from_millis(4), [0; STAGES]));
        assert!(advance(&mut frame, 8, [0; STAGES]).is_none());
        let mut stages = [0; STAGES];
        stages[RuntimeStage::UiPublication as usize] = 23_000_000;
        frame.main = Some((Duration::from_millis(25), stages));
        let (_, line) = advance(&mut frame, 36, stages).unwrap();
        let line = line.unwrap();
        assert!(line.contains("main_ms=25.000 between_updates_ms=3.000"));
        assert!(line.contains("violations=interval+main"));
        assert!(line.contains("ui_publication:23.000"));
        frame.main = Some((Duration::from_millis(2), [0; STAGES]));
        let (event, line) = advance(&mut frame, 66, stages).unwrap();
        assert_eq!(event.reasons, INTERVAL);
        assert!(line.is_none());
        frame.main = Some((Duration::from_millis(2), [0; STAGES]));
        let (_, line) = advance(&mut frame, 1100, stages).unwrap();
        let line = line.unwrap();
        assert!(line.contains("suppressed=1"));
        assert!(line.contains("slow_frames=3"));
        assert!(line.contains("main_ms=2.000 between_updates_ms=1032.000"));
        assert_eq!([frame.counts.hitches, frame.counts.hard_hitches], [3, 1]);
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
