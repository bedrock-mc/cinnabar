//! Frame admission: the single wait between one frame's handoff and the next frame's input.
//!
//! The main thread hands frame N to the render thread at the end of an update, then would start
//! N+1 at once and block at the next handoff until N finishes rendering, so N+1's input goes stale
//! by that whole wait. After each handoff this delays the next update until the later of two
//! deadlines: the next slot of the requested frame-rate cadence, and the predicted render
//! completion less the predicted main-thread time and a safety margin, from recent frame history
//! (or that frame's actual completion if sooner). Render time includes drawable acquisition, so
//! swapchain backpressure is folded in. The wait ends before the event loop collects the next
//! frame's input, so events arriving meanwhile are read fresh, and since every update passes
//! through it, input events can wake the loop but never admit an extra frame.

mod frame_time;
mod wait;

use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::{
    app::MainScheduleOrder,
    ecs::schedule::{MainThreadExecutor, ScheduleLabel},
    prelude::*,
    render::{Render, RenderApp, RenderSystems, pipelined_rendering::RenderExtractApp},
};
use render_model::{Cadence, FrameRate};

use crate::{RuntimeStage, RuntimeStageProfiler};

/// `0` keeps measuring input age but never delays an update for render completion.
const PACING_ENV: &str = "RUST_MCBE_INPUT_PACING";

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

/// Starts frame timing before reclaiming background work and running `First`.
#[derive(ScheduleLabel, Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameStart;

impl FrameStart {
    /// Installs the timing boundary once, preserving schedules already placed after it.
    pub fn install(app: &mut App) {
        app.init_schedule(Self);
        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        if !order.labels.contains(&Self.intern()) {
            order.insert_before(First, Self);
        }
    }
}

/// The cadence the next frames are admitted at; the app's presentation policy writes it.
#[derive(Resource, Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FramePacing {
    /// At most one frame samples input per slot of this rate; `None` paces on rendering alone.
    pub rate: Option<FrameRate>,
    /// Spins briefly before each deadline for precision; only worth a core while focused.
    pub precise: bool,
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

    /// Marks this update's input sample against the cadence; a new rate starts a new epoch.
    fn update_started(&self, pacing: FramePacing) {
        let now = self.clock.now();
        let at = self.nanos(now);
        let mut state = self.state();
        state.update_started = Some(now);
        state.precise = pacing.precise;
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
        let (update_started, delay, handed_frame, cadence, precise) = {
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
            (
                update_started,
                state.model.delay(),
                state.handed,
                cadence,
                state.precise,
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
        let Some(deadline) = cadence.max(render_ready) else {
            return;
        };
        let executor = main
            .get_resource::<MainThreadExecutor>()
            .map(|executor| executor.0.clone());
        self.wait(
            deadline,
            precise,
            profiler.as_ref(),
            || {
                // Rendering may only cut the wait short once the cadence slot has opened.
                render_ready.is_some()
                    && cadence.is_none_or(|slot| self.clock.now() >= slot)
                    && self.state().finished >= handed_frame
            },
            executor.as_deref(),
        );
    }

    /// Without pipelined rendering only the cadence applies, at the end of the update.
    fn pace_unpipelined(&self, main: &World) {
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

/// Paces updates at the handoff when rendering is pipelined, otherwise at the end of `Last`.
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
        InputPacer::new(Arc::new(SystemClock::new()), enabled)
    }
}

impl Plugin for InputPacingPlugin {
    fn build(&self, app: &mut App) {
        FrameStart::install(app);
        let pacer = self.pacer();
        app.init_resource::<FramePacing>()
            .insert_resource(pacer.clone())
            .add_systems(FrameStart, mark_update_start);
        frame_time::install(app);
        let inner = app
            .get_sub_app_mut(RenderExtractApp)
            .and_then(|extract_app| extract_app.take_extract());
        let Some(mut inner) = inner else {
            app.add_systems(Last, pace_unpipelined);
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
                ),
            );
        }
    }
}

fn mark_update_start(pacer: Res<InputPacer>, pacing: Res<FramePacing>) {
    pacer.update_started(*pacing);
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

#[cfg(test)]
#[path = "input_pacing/tests.rs"]
mod tests;
