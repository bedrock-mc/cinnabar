//! Bounded launch diagnostics shared by the main and render threads.

use bevy::prelude::Resource;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use web_time::Instant;

/// Machine-readable marker emitted once the first main-menu frame completes.
pub const MARKER: &str = render_api::STARTUP_TIMING_MARKER;
/// Counts native world shader modules created while menu sampling is open.
pub const WORLD_SHADER_MODULES: &str = "world_shader_modules";
/// Counts native world compute pipelines created while menu sampling is open.
pub const WORLD_COMPUTE_PIPELINES: &str = "world_compute_pipelines";
const MAX_FRAMES: usize = 8;
static LAUNCH: OnceLock<StartupTiming> = OnceLock::new();

/// Launch measurements stop recording once their presentation report is sealed.
#[derive(Clone, Resource)]
pub struct StartupTiming(Arc<Shared>);

struct Shared {
    launched: Instant,
    finished: AtomicBool,
    samples_sealed: AtomicBool,
    offscreen: AtomicBool,
    offscreen_completed: Mutex<Option<Instant>>,
    report: Mutex<Report>,
}

/// Durations can overlap; they describe work, rather than an additive launch timeline.
#[derive(Default, Serialize)]
pub struct Report {
    pub debug_assertions: bool,
    pub offscreen: bool,
    pub menu_ms: f64,
    pub stages_ms: BTreeMap<String, f64>,
    pub counters: BTreeMap<String, u64>,
    pub render_frames: Vec<RenderFrame>,
    pub main_updates_ms: Vec<f64>,
    pub pipeline_count: usize,
    pub pipeline_labels: BTreeMap<String, usize>,
}

/// CPU work from one of the first submitted render frames.
#[derive(Default, Serialize)]
pub struct RenderFrame {
    pub total_ms: f64,
    pub prepare_ms: f64,
    pub pipeline_and_shader_ms: f64,
    pub submit_ms: f64,
    pub pipelines: usize,
    pub pending_pipelines: usize,
    pub pipeline_labels: BTreeMap<String, usize>,
}

impl StartupTiming {
    /// Starts a timeline at the process entry point's monotonic launch anchor.
    pub fn start(launched: Instant) -> Self {
        LAUNCH.get_or_init(|| Self::new(launched)).clone()
    }

    /// Creates an independent timeline so tests never change the process recorder.
    fn new(launched: Instant) -> Self {
        Self(Arc::new(Shared {
            launched,
            finished: AtomicBool::new(false),
            samples_sealed: AtomicBool::new(false),
            offscreen: AtomicBool::new(false),
            offscreen_completed: Mutex::default(),
            report: Mutex::new(Report {
                debug_assertions: cfg!(debug_assertions),
                ..Default::default()
            }),
        }))
    }

    /// Selects menu measurement; direct world launches leave this recorder inactive.
    pub fn configure_menu(&self, expected: bool, offscreen: bool) {
        self.0.finished.store(!expected, Ordering::Release);
        self.0
            .offscreen
            .store(expected && offscreen, Ordering::Release);
        self.0.report.lock().expect("startup timing lock").offscreen = offscreen;
    }

    /// Hidden developer runs measure a completed capture image, separately from native presents.
    pub fn is_offscreen(&self) -> bool {
        self.needs_report() && self.0.offscreen.load(Ordering::Acquire)
    }

    /// Retains the first completed menu image without changing native-surface readiness.
    pub fn record_offscreen_content(&self, completed: Instant) {
        if self.is_offscreen() {
            self.0
                .offscreen_completed
                .lock()
                .expect("startup image lock")
                .get_or_insert(completed);
        }
    }

    /// Returns an offscreen measurement only when that endpoint was explicitly requested.
    pub fn offscreen_content_at(&self) -> Option<Instant> {
        self.is_offscreen()
            .then(|| {
                *self
                    .0
                    .offscreen_completed
                    .lock()
                    .expect("startup image lock")
            })
            .flatten()
    }

    /// Whether launch work still belongs to the unsealed presentation report.
    pub fn is_recording(&self) -> bool {
        self.needs_report() && !self.0.samples_sealed.load(Ordering::Acquire)
    }

    /// Keeps report emission active after GPU completion has stopped work sampling.
    pub fn needs_report(&self) -> bool {
        !self.0.finished.load(Ordering::Acquire)
    }

    /// Stops sampling at the completed menu frame, before any admitted world work can start.
    pub fn seal_samples(&self) {
        self.0.samples_sealed.store(true, Ordering::Release);
    }

    /// Records a completed stage, summing repeated work under its stable name.
    pub fn record(&self, name: &str, duration: Duration) {
        if self.is_recording() {
            *self
                .0
                .report
                .lock()
                .expect("startup timing lock")
                .stages_ms
                .entry(name.to_owned())
                .or_default() += duration.as_secs_f64() * 1000.0;
        }
    }

    /// Counts actual startup work, independently of its execution time.
    pub fn count(&self, name: &str, amount: u64) {
        if self.is_recording() {
            *self
                .0
                .report
                .lock()
                .expect("startup timing lock")
                .counters
                .entry(name.to_owned())
                .or_default() += amount;
        }
    }

    /// Retains the initial main updates so a first-update hitch stays visible in the report.
    pub fn main_update(&self, duration: Duration) {
        if self.is_recording() {
            let mut report = self.0.report.lock().expect("startup timing lock");
            if report.main_updates_ms.len() < MAX_FRAMES {
                report.main_updates_ms.push(duration.as_secs_f64() * 1000.0);
            }
        }
    }

    /// Retains only the initial render frames, keeping diagnostics bounded on failed startup.
    pub fn frame(&self, mut frame: RenderFrame) {
        if self.is_recording() {
            let mut report = self.0.report.lock().expect("startup timing lock");
            report.pipeline_count = frame.pipelines;
            report.pipeline_labels = std::mem::take(&mut frame.pipeline_labels);
            if report.render_frames.len() < MAX_FRAMES {
                report.render_frames.push(frame);
            }
        }
    }

    /// Seals the first presented menu report exactly once, using the completion timestamp.
    pub fn finish(&self, presented: Instant) -> Option<String> {
        if self.0.finished.swap(true, Ordering::AcqRel) {
            return None;
        }
        let mut report = self.0.report.lock().expect("startup timing lock");
        report.menu_ms = presented
            .saturating_duration_since(self.0.launched)
            .as_secs_f64()
            * 1000.0;
        Some(serde_json::to_string(&*report).expect("finite startup measurements"))
    }
}

/// Returns the launch recorder without creating a timeline for tests or offline renderers.
pub fn current() -> Option<&'static StartupTiming> {
    LAUNCH.get()
}

/// Times one startup boundary and exposes it to the existing Tracy subscriber.
pub fn measure<T>(name: &'static str, work: impl FnOnce() -> T) -> T {
    let Some(timing) = current().filter(|timing| timing.is_recording()) else {
        return work();
    };
    let _span = bevy::log::info_span!("startup.stage", stage = name).entered();
    let started = Instant::now();
    let result = work();
    timing.record(name, started.elapsed());
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offscreen_completion_is_explicit_and_direct_joins_do_not_record_menu_work() {
        let now = Instant::now();
        let timing = StartupTiming::new(now);
        timing.record_offscreen_content(now);
        assert_eq!(timing.offscreen_content_at(), None);
        timing.configure_menu(true, true);
        timing.record_offscreen_content(now);
        timing.record_offscreen_content(now + Duration::from_millis(1));
        assert_eq!(timing.offscreen_content_at(), Some(now));
        let report: serde_json::Value = serde_json::from_str(&timing.finish(now).unwrap()).unwrap();
        assert_eq!(report["offscreen"], true);
        let direct = StartupTiming::new(now);
        direct.configure_menu(false, false);
        assert!(!direct.is_recording());
    }

    #[test]
    fn completed_menu_stops_world_counters_but_keeps_its_report_and_latest_pipeline_set() {
        let now = Instant::now();
        let timing = StartupTiming::new(now);
        timing.configure_menu(true, true);
        for pipelines in 0..MAX_FRAMES + 2 {
            timing.frame(RenderFrame {
                pipelines,
                ..Default::default()
            });
        }
        timing.record_offscreen_content(now);
        timing.seal_samples();
        timing.count(WORLD_COMPUTE_PIPELINES, 1);
        timing.frame(RenderFrame {
            pipelines: 999,
            ..Default::default()
        });
        assert!(!timing.is_recording());
        assert!(timing.needs_report());
        assert_eq!(timing.offscreen_content_at(), Some(now));
        let report: serde_json::Value = serde_json::from_str(&timing.finish(now).unwrap()).unwrap();
        assert_eq!(report["pipeline_count"], MAX_FRAMES + 1);
        assert!(report["counters"].get(WORLD_COMPUTE_PIPELINES).is_none());
    }

    #[test]
    fn presentation_seals_one_bounded_report_and_stops_counting() {
        let launched = Instant::now();
        let timing = StartupTiming::new(launched);
        timing.record("carriers", Duration::from_millis(3));
        timing.record("carriers", Duration::from_millis(2));
        timing.count("uploads", 4);
        for _ in 0..MAX_FRAMES + 3 {
            timing.frame(RenderFrame::default());
            timing.main_update(Duration::from_millis(1));
        }
        let report: serde_json::Value =
            serde_json::from_str(&timing.finish(launched + Duration::from_millis(7)).unwrap())
                .unwrap();
        assert_eq!(report["menu_ms"], 7.0);
        assert_eq!(report["stages_ms"]["carriers"], 5.0);
        assert_eq!(report["counters"]["uploads"], 4);
        assert_eq!(
            report["render_frames"].as_array().unwrap().len(),
            MAX_FRAMES
        );
        assert_eq!(
            report["main_updates_ms"].as_array().unwrap().len(),
            MAX_FRAMES
        );
        assert!(!timing.is_recording());
        timing.count("uploads", 9);
        assert!(timing.finish(launched).is_none());
        assert_eq!(timing.0.report.lock().unwrap().counters["uploads"], 4);
    }
}
