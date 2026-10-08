//! Just-in-time input sampling for pipelined rendering.
//!
//! The main thread hands frame N to the render thread at the end of an update, then would start
//! N+1 at once and block at the next handoff until N finishes rendering, so N+1's input goes stale
//! by that whole wait. After each handoff this delays the next update until the predicted render
//! completion less the predicted main-thread time and a safety margin, from recent frame history,
//! or until that frame actually finishes if sooner. Render time includes drawable acquisition, so
//! swapchain backpressure is folded in.

use std::{
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use bevy::{
    ecs::schedule::MainThreadExecutor,
    prelude::*,
    render::{Render, RenderApp, RenderSystems, pipelined_rendering::RenderExtractApp},
};

use crate::{RuntimeStage, RuntimeStageProfiler};

/// `0` keeps measuring input age but never delays an update.
const PACING_ENV: &str = "RUST_MCBE_INPUT_PACING";

/// Time the next update reaches the handoff ahead of render completion.
pub(crate) const MARGIN: Duration = Duration::from_micros(1_000);
/// Longest single delay; a stalled render thread never holds input for longer.
const MAX_DELAY: Duration = Duration::from_millis(50);
const HISTORY: usize = 32;
/// Samples of each duration needed before any delay is predicted.
const MIN_SAMPLES: usize = 8;
/// Typical durations; the margin absorbs ordinary jitter on both sides.
const MEDIAN: usize = HISTORY / 2;
/// Longest sleep between checks for render completion and main-thread tasks while waiting.
const WAIT_SLICE: Duration = Duration::from_micros(250);

/// Wall time and blocking for the pacer; tests substitute a deterministic clock.
pub(crate) trait PacingClock: Send + Sync + 'static {
    fn now(&self) -> Instant;
    /// Blocks until `deadline` or until `done` returns true; `done` also runs main-thread tasks.
    fn wait_until(&self, deadline: Instant, done: &mut dyn FnMut() -> bool);
}

struct SystemClock;

impl PacingClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait_until(&self, deadline: Instant, done: &mut dyn FnMut() -> bool) {
        while !done() {
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            std::thread::sleep((deadline - now).min(WAIT_SLICE));
        }
    }
}

/// A fixed ring of recent durations.
#[derive(Debug, Default)]
struct History {
    samples: [Duration; HISTORY],
    len: usize,
    next: usize,
}

impl History {
    /// Starts over from `sample` when it is under half or over double the median, so stale
    /// history never outlives a step change.
    fn record(&mut self, sample: Duration) {
        if self
            .quantile(MEDIAN)
            .is_some_and(|median| sample * 2 < median || sample > median * 2)
        {
            self.len = 0;
            self.next = 0;
        }
        self.push(sample);
    }

    fn push(&mut self, sample: Duration) {
        self.samples[self.next] = sample;
        self.next = (self.next + 1) % HISTORY;
        self.len = (self.len + 1).min(HISTORY);
    }

    /// The sample at `rank` of a full window, scaled to the samples held.
    fn quantile(&self, rank: usize) -> Option<Duration> {
        if self.len < MIN_SAMPLES {
            return None;
        }
        let mut sorted = self.samples;
        let sorted = &mut sorted[..self.len];
        sorted.sort_unstable();
        Some(sorted[rank * self.len / HISTORY])
    }
}

/// Recent main and render durations and the delay they imply.
#[derive(Debug, Default)]
struct PacingModel {
    main: History,
    render: History,
}

impl PacingModel {
    /// How long after a handoff the next update should start; zero when render is not slower.
    fn delay(&self) -> Duration {
        let (Some(render), Some(main)) = (self.render.quantile(MEDIAN), self.main.quantile(MEDIAN))
        else {
            return Duration::ZERO;
        };
        render.saturating_sub(main + MARGIN).min(MAX_DELAY)
    }
}

#[derive(Debug, Default)]
struct PacerState {
    model: PacingModel,
    /// When the current update began, sampling input.
    update_started: Option<Instant>,
    /// When the render thread began its current frame.
    render_started: Option<Instant>,
    /// Frames handed to the render thread and frames it has finished; it renders them in order.
    handed: u64,
    finished: u64,
}

/// Shared between the main-thread handoff and the render thread's frame markers.
#[derive(Resource, Clone)]
pub(crate) struct InputPacer {
    state: Arc<Mutex<PacerState>>,
    clock: Arc<dyn PacingClock>,
    enabled: bool,
}

impl InputPacer {
    pub(crate) fn new(clock: Arc<dyn PacingClock>, enabled: bool) -> Self {
        Self {
            state: Arc::default(),
            clock,
            enabled,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, PacerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn update_started(&self) {
        self.state().update_started = Some(self.clock.now());
    }

    fn render_started(&self) {
        self.state().render_started = Some(self.clock.now());
    }

    fn render_finished(&self) {
        let now = self.clock.now();
        let mut state = self.state();
        if let Some(started) = state.render_started.take() {
            state
                .model
                .render
                .record(now.saturating_duration_since(started));
            state.finished += 1;
        }
    }

    /// Runs Bevy's handoff, then waits until the next update should sample input.
    fn handoff(
        &self,
        main: &mut World,
        render: &mut World,
        inner: &mut dyn FnMut(&mut World, &mut World),
    ) {
        let ready = self.clock.now();
        inner(main, render);
        let handed = self.clock.now();
        let (update_started, delay, handed_frame) = {
            let mut state = self.state();
            state.handed += 1;
            let update_started = state.update_started.take();
            if let Some(started) = update_started {
                state
                    .model
                    .main
                    .record(ready.saturating_duration_since(started));
            }
            (update_started, state.model.delay(), state.handed)
        };
        let profiler = main.get_resource::<RuntimeStageProfiler>().cloned();
        if let (Some(profiler), Some(started)) = (&profiler, update_started) {
            profiler.record(
                RuntimeStage::InputAge,
                started,
                handed.saturating_duration_since(started),
            );
        }
        if !self.enabled || delay.is_zero() {
            return;
        }
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("input_pacing.wait").entered();
        let executor = main
            .get_resource::<MainThreadExecutor>()
            .map(|executor| executor.0.clone());
        let ticker = executor.as_deref().and_then(|executor| executor.ticker());
        let mut done = || {
            while ticker.as_ref().is_some_and(|ticker| ticker.try_tick()) {}
            self.state().finished >= handed_frame
        };
        self.clock.wait_until(handed + delay, &mut done);
        if let Some(profiler) = profiler {
            profiler.record(
                RuntimeStage::InputPacingWait,
                handed,
                self.clock.now().saturating_duration_since(handed),
            );
        }
    }
}

/// Paces updates when rendering is pipelined; otherwise render runs inside the update and
/// there is no handoff to pace.
#[derive(Default)]
pub struct InputPacingPlugin {
    #[cfg(test)]
    pacer: Option<InputPacer>,
}

impl InputPacingPlugin {
    #[cfg(test)]
    pub(crate) fn with_pacer(pacer: InputPacer) -> Self {
        Self { pacer: Some(pacer) }
    }

    fn pacer(&self) -> InputPacer {
        #[cfg(test)]
        if let Some(pacer) = &self.pacer {
            return pacer.clone();
        }
        let enabled = std::env::var_os(PACING_ENV).is_none_or(|value| value != "0");
        InputPacer::new(Arc::new(SystemClock), enabled)
    }
}

impl Plugin for InputPacingPlugin {
    fn build(&self, app: &mut App) {
        let Some(extract_app) = app.get_sub_app_mut(RenderExtractApp) else {
            return;
        };
        let Some(mut inner) = extract_app.take_extract() else {
            return;
        };
        let pacer = self.pacer();
        let handoff = pacer.clone();
        extract_app.set_extract(move |main, render| handoff.handoff(main, render, &mut inner));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(pacer.clone()).add_systems(
                Render,
                (
                    mark_render_start.before(RenderSystems::ExtractCommands),
                    mark_render_end.in_set(RenderSystems::PostCleanup),
                ),
            );
        }
        app.insert_resource(pacer)
            .add_systems(First, mark_update_start);
    }
}

fn mark_update_start(pacer: Res<InputPacer>) {
    pacer.update_started();
}

fn mark_render_start(pacer: Res<InputPacer>) {
    pacer.render_started();
}

fn mark_render_end(pacer: Res<InputPacer>) {
    pacer.render_finished();
}

#[cfg(test)]
#[path = "input_pacing/tests.rs"]
mod tests;
