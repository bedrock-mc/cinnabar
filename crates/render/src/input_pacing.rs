//! Admits fresh input after frame handoff using the requested cadence and predicted CPU/GPU work.

use bevy::prelude::IntoScheduleConfigs;
mod frame_time;
#[cfg(any(target_os = "macos", test))]
mod gpu;
mod wait;

use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

#[cfg(test)]
use bevy::{
    app::MainScheduleOrder,
    ecs::schedule::ScheduleLabel,
    prelude::{First, Fixed, FixedUpdate, Last, PreUpdate, Real, ResMut, Time, Update},
};
use bevy::{
    ecs::schedule::MainThreadExecutor,
    prelude::{App, Plugin, Res, Resource, World},
    render::{Render, RenderApp, RenderSystems, pipelined_rendering::RenderExtractApp},
};
use render_model::{Cadence, FrameRate};

use render_scene::{RuntimeStage, RuntimeStageProfiler};

/// `0` keeps measuring input age but never delays an update for render completion.
const PACING_ENV: &str = "CINNABAR_INPUT_PACING";

/// Time the next update reaches the handoff ahead of render completion.
pub(crate) const MARGIN: Duration = Duration::from_micros(1_000);
/// Longest render-completion delay; a stalled render thread never holds input for longer.
const MAX_DELAY: Duration = Duration::from_millis(50);
const HISTORY: usize = 32;
/// Samples of each duration needed before any delay is predicted.
const MIN_SAMPLES: usize = 8;
/// Typical durations; the margin absorbs ordinary jitter on both sides.
const MEDIAN: usize = HISTORY / 2;
/// Longest sleep between checks for render completion and main-thread tasks while waiting;
/// background windows check less often.
const SERVICE_SLICE: Duration = Duration::from_millis(1);
const BACKGROUND_SERVICE_SLICE: Duration = Duration::from_millis(4);
/// Spin before a focused deadline: starts here, then tracks observed sleep overshoot.
const INITIAL_SPIN: Duration = Duration::from_micros(200);
const MIN_SPIN: Duration = Duration::from_micros(50);
const MAX_SPIN: Duration = Duration::from_micros(500);
const SPIN_HEADROOM: Duration = Duration::from_micros(25);

use crate::{FrameEnd, FrameStart};

/// The cadence the next frames are admitted at; the app's presentation policy writes it.
#[derive(Resource, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FramePacing {
    /// At most one frame samples input per slot of this rate; `None` paces on rendering alone.
    pub rate: Option<FrameRate>,
    /// Spins briefly before each deadline for precision; only worth a core while focused.
    pub precise: bool,
    /// Display-synchronized presentation owns GPU backpressure instead of queue prediction.
    pub display_sync: bool,
}

/// Wall time and blocking for the pacer; tests substitute a deterministic clock.
pub(crate) trait PacingClock: Send + Sync + 'static {
    fn now(&self) -> Instant;
    /// Blocks until `deadline` or until `done` returns true; `done` also runs main-thread tasks.
    /// `precise` permits a short spin so the wake lands on the deadline.
    fn wait_until(&self, deadline: Instant, precise: bool, done: &mut dyn FnMut() -> bool);
}

/// Sleeps on the OS's absolute monotonic timer, then spins the stretch its wakes overshoot by.
struct SystemClock {
    /// Current spin allowance in nanoseconds, raised at once on a late wake and relaxed slowly.
    spin_nanos: AtomicU64,
}

impl SystemClock {
    fn new() -> Self {
        Self {
            spin_nanos: AtomicU64::new(INITIAL_SPIN.as_nanos() as u64),
        }
    }

    fn spin(&self) -> Duration {
        Duration::from_nanos(self.spin_nanos.load(Ordering::Relaxed))
    }

    fn record_overshoot(&self, overshoot: Duration) {
        let wanted = (overshoot + SPIN_HEADROOM).clamp(MIN_SPIN, MAX_SPIN);
        let current = self.spin();
        let next = if wanted > current {
            wanted
        } else {
            current.saturating_sub(Duration::from_micros(1)).max(wanted)
        };
        self.spin_nanos
            .store(next.as_nanos() as u64, Ordering::Relaxed);
    }
}

impl PacingClock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn wait_until(&self, deadline: Instant, precise: bool, done: &mut dyn FnMut() -> bool) {
        let (spin, slice) = if precise {
            (self.spin(), SERVICE_SLICE)
        } else {
            (Duration::ZERO, BACKGROUND_SERVICE_SLICE)
        };
        let spin_from = deadline.checked_sub(spin).unwrap_or(deadline);
        while !done() {
            let now = Instant::now();
            if now >= deadline {
                return;
            }
            if now < spin_from {
                let target = spin_from.min(now + slice);
                wait::sleep_until(target);
                if precise && target == spin_from {
                    self.record_overshoot(Instant::now().saturating_duration_since(target));
                }
            } else {
                std::hint::spin_loop();
            }
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
    render_work: History,
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
    #[cfg(any(target_os = "macos", test))]
    gpu: gpu::GpuPacing,
    rendering: u64,
    /// When the current update began, sampling input.
    update_started: Option<Instant>,
    /// When the render thread began its current frame.
    render_started: Option<Instant>,
    surface_started: Option<Instant>,
    surface_elapsed: Duration,
    /// Frames handed to the render thread and frames it has finished; it renders them in order.
    handed: u64,
    finished: u64,
    cadence: Option<Cadence>,
    precise: bool,
}

/// Shared between the main-thread handoff and the render thread's frame markers.
#[derive(Resource, Clone)]
pub(crate) struct InputPacer {
    state: Arc<Mutex<PacerState>>,
    clock: Arc<dyn PacingClock>,
    /// Origin of the cadence's nanosecond timeline.
    epoch: Instant,
    enabled: bool,
}

impl InputPacer {
    pub(crate) fn new(clock: Arc<dyn PacingClock>, enabled: bool) -> Self {
        Self {
            state: Arc::default(),
            epoch: clock.now(),
            clock,
            enabled,
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, PacerState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn nanos(&self, at: Instant) -> u64 {
        u64::try_from(at.saturating_duration_since(self.epoch).as_nanos()).unwrap_or(u64::MAX)
    }

    fn instant(&self, nanos: u64) -> Instant {
        self.epoch + Duration::from_nanos(nanos)
    }

    /// Records input sampling before frame work, without retaining the previous focus policy.
    fn update_started(&self) {
        self.state().update_started = Some(self.clock.now());
    }

    /// Applies this frame's policy before waiting, anchoring cadence to its input sample.
    fn prepare_wait(&self, pacing: FramePacing) {
        let mut state = self.state();
        let at = self.nanos(state.update_started.unwrap_or_else(|| self.clock.now()));
        state.precise = pacing.precise;
        #[cfg(any(target_os = "macos", test))]
        state.gpu.set_display_sync(pacing.display_sync);
        state.cadence = pacing.rate.map(|rate| {
            let mut cadence = match state.cadence {
                Some(cadence) if cadence.rate() == rate => cadence,
                _ => Cadence::new(rate, at),
            };
            cadence.admit(at);
            cadence
        });
    }

    fn render_started(&self) {
        let mut state = self.state();
        state.rendering += 1;
        state.render_started = Some(self.clock.now());
        state.surface_elapsed = Duration::ZERO;
    }

    /// Separates drawable backpressure from CPU work needed to feed the GPU.
    fn surface_started(&self) {
        self.state().surface_started = Some(self.clock.now());
    }

    /// Accounts for surface preparation without retaining it as GPU feeding work.
    fn surface_finished(&self) {
        let now = self.clock.now();
        let mut state = self.state();
        if let Some(started) = state.surface_started.take() {
            state.surface_elapsed += now.saturating_duration_since(started);
        }
    }

    /// Predicts queue completion for a submission, without waiting on the GPU.
    #[cfg(target_os = "macos")]
    pub(crate) fn gpu_submitted(&self) -> u64 {
        let mut state = self.state();
        let frame = state.rendering;
        state.gpu.submit(frame, self.clock.now())
    }

    /// Corrects the queue prediction with completed Metal host timestamps.
    #[cfg(target_os = "macos")]
    pub(crate) fn gpu_completed(&self, submission: u64, end: Instant, elapsed: Duration) {
        self.state().gpu.complete(submission, end, elapsed);
    }

    fn render_finished(&self) {
        let now = self.clock.now();
        let mut state = self.state();
        if let Some(started) = state.render_started.take() {
            let elapsed = now.saturating_duration_since(started);
            let work = elapsed.saturating_sub(state.surface_elapsed);
            state.model.render.record(elapsed);
            state.model.render_work.record(work);
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
        self.prepare_wait(*main.resource::<FramePacing>());
        let (update_started, delay, handed_frame, cadence, precise, gpu_ready) = {
            let mut state = self.state();
            state.handed += 1;
            let update_started = state.update_started.take();
            if let Some(started) = update_started {
                state
                    .model
                    .main
                    .record(ready.saturating_duration_since(started));
            }
            let cadence = state
                .cadence
                .map(|cadence| self.instant(cadence.next_admission_nanos()));
            #[cfg(any(target_os = "macos", test))]
            let gpu_ready = self
                .enabled
                .then(|| state.gpu.deadline(state.handed, handed, &state.model))
                .flatten();
            #[cfg(not(any(target_os = "macos", test)))]
            let gpu_ready: Option<Instant> = None;
            (
                update_started,
                state.model.delay(),
                state.handed,
                cadence,
                state.precise,
                gpu_ready,
            )
        };
        let profiler = main.get_resource::<RuntimeStageProfiler>().cloned();
        if let (Some(profiler), Some(started)) = (&profiler, update_started) {
            profiler.record(
                RuntimeStage::InputAge,
                started,
                handed.saturating_duration_since(started),
            );
        }
        let render_ready = (self.enabled && !delay.is_zero()).then(|| handed + delay);
        let Some(deadline) = cadence.max(render_ready).max(gpu_ready) else {
            return;
        };
        let executor = main
            .get_resource::<MainThreadExecutor>()
            .map(|executor| executor.0.clone());
        // The cadence may open before the predicted render deadline. Wake precisely there
        // so an already completed render can release input without another service slice.
        if let Some(admission) = cadence
            .max(gpu_ready)
            .filter(|admission| *admission < deadline)
        {
            self.wait(
                admission,
                precise,
                profiler.as_ref(),
                || false,
                executor.as_deref(),
            );
        }
        self.wait(
            deadline,
            precise,
            profiler.as_ref(),
            || {
                // Rendering may only cut the wait short once the cadence slot has opened.
                render_ready.is_some()
                    && cadence.is_none_or(|slot| self.clock.now() >= slot)
                    && gpu_ready.is_none_or(|slot| self.clock.now() >= slot)
                    && self.state().finished >= handed_frame
            },
            executor.as_deref(),
        );
    }

    /// Without pipelined rendering only the cadence applies, at the end of the update.
    fn pace_unpipelined(&self, main: &World) {
        self.prepare_wait(*main.resource::<FramePacing>());
        let (cadence, precise) = {
            let state = self.state();
            (state.cadence, state.precise)
        };
        if let Some(cadence) = cadence {
            let profiler = main.get_resource::<RuntimeStageProfiler>().cloned();
            let deadline = self.instant(cadence.next_admission_nanos());
            self.wait(deadline, precise, profiler.as_ref(), || false, None);
        }
    }

    fn wait(
        &self,
        deadline: Instant,
        precise: bool,
        profiler: Option<&RuntimeStageProfiler>,
        mut finished: impl FnMut() -> bool,
        executor: Option<&bevy::tasks::ThreadExecutor<'static>>,
    ) {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!("input_pacing.wait").entered();
        let started = self.clock.now();
        let ticker = executor.and_then(|executor| executor.ticker());
        let mut done = || {
            while ticker.as_ref().is_some_and(|ticker| ticker.try_tick()) {}
            finished()
        };
        self.clock.wait_until(deadline, precise, &mut done);
        if let Some(profiler) = profiler {
            let woke = self.clock.now();
            profiler.record(
                RuntimeStage::InputPacingWait,
                started,
                woke.saturating_duration_since(started),
            );
            if woke >= deadline {
                profiler.record(
                    RuntimeStage::FramePacingLateness,
                    deadline,
                    woke.saturating_duration_since(deadline),
                );
            }
        }
    }
}

/// Paces updates at the render handoff or the final frame boundary.
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
        let enabled = env_compat::var_os(PACING_ENV).is_none_or(|value| value != "0");
        InputPacer::new(Arc::new(SystemClock::new()), enabled)
    }
}

impl Plugin for InputPacingPlugin {
    fn build(&self, app: &mut App) {
        FrameStart::install(app);
        FrameEnd::install(app);
        let pacer = self.pacer();
        app.init_resource::<FramePacing>()
            .insert_resource(pacer.clone())
            .add_systems(FrameStart, mark_update_start);
        frame_time::install(app);
        let inner = app
            .get_sub_app_mut(RenderExtractApp)
            .and_then(|extract_app| extract_app.take_extract());
        let Some(mut inner) = inner else {
            app.add_systems(FrameEnd, pace_unpipelined);
            return;
        };
        let handoff = pacer.clone();
        app.sub_app_mut(RenderExtractApp)
            .set_extract(move |main, render| handoff.handoff(main, render, &mut inner));
        if let Some(render_app) = app.get_sub_app_mut(RenderApp) {
            render_app.insert_resource(pacer).add_systems(
                Render,
                (
                    mark_render_start.before(RenderSystems::ExtractCommands),
                    mark_render_end.in_set(RenderSystems::PostCleanup),
                    (
                        mark_surface_start.before(bevy::render::view::window::prepare_windows),
                        mark_surface_end.after(bevy::render::view::window::prepare_windows),
                    )
                        .in_set(RenderSystems::PrepareViews),
                ),
            );
        }
    }
}

fn mark_update_start(pacer: Res<InputPacer>) {
    pacer.update_started();
}

fn pace_unpipelined(world: &mut World) {
    if let Some(pacer) = world.get_resource::<InputPacer>().cloned() {
        pacer.pace_unpipelined(world);
    }
}

fn mark_render_start(pacer: Res<InputPacer>) {
    pacer.render_started();
}

fn mark_render_end(pacer: Res<InputPacer>) {
    pacer.render_finished();
}

fn mark_surface_start(pacer: Res<InputPacer>) {
    pacer.surface_started();
}

fn mark_surface_end(pacer: Res<InputPacer>) {
    pacer.surface_finished();
}

#[cfg(test)]
#[path = "input_pacing/tests.rs"]
mod tests;
