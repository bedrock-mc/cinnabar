use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::prelude::{Res, ResMut, Resource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum RuntimeStage {
    ActorSessionSetup,
    PackReload,
    WorldPoll,
    SurfacePreparation,
    RenderSubmission,
    ActorGeometrySetup,
    ActorArtworkSetup,
    ActorEquipmentSetup,
    NetworkIngestion,
    WorldStream,
    CaveVisibility,
    RenderQueueApplication,
    ChunkExtraction,
    GpuPreparation,
    IndirectPreparation,
    TransparentPreparation,
    TransparentWorker,
    OpaqueQueue,
    OpaqueDiagnostics,
    OpaqueBatchPlanning,
    TransparentQueue,
    AcceptanceTelemetry,
    /// Main-world wall time from `First` to `Last`.
    MainFrame,
    ActorPublication,
    /// Fixed-tick actor animation and Molang, inside `ActorPublication`.
    ActorAnimation,
    /// Per-frame actor presentation, culling and layers, inside `ActorPublication`.
    ActorPreparation,
    /// Bone matrices and instance arena build, inside `ActorPublication`.
    ActorRigBuild,
    UiPublication,
    UiPreparation,
    Particles,
    Audio,
    BlockEntities,
    /// Render-world wall time for one frame, excluding the drawable-acquisition wait.
    RenderFrame,
    /// First to last sampled GPU timestamp; absent when coverage includes only owned passes.
    GpuFrame,
    GpuShadows,
    GpuOpaque,
    GpuTransparent,
    GpuUi,
    GpuHand,
    GpuPost,
    GpuTonemapping,
    GpuFxaa,
    GpuBlit,
    /// Draw categories timed inside passes; only with aggregate profiling on capable adapters.
    GpuTerrainOpaque,
    GpuTerrainTransparent,
    GpuActors,
    GpuParticles,
    GpuSky,
    GpuPanorama,
    /// Personal-mod post passes by execution slot, and mod world primitives.
    GpuModPass0,
    GpuModPass1,
    GpuModPass2,
    GpuModPass3,
    GpuModPass4,
    GpuModPass5,
    GpuModPass6,
    GpuModPass7,
    GpuModPrimitives,
}

impl RuntimeStage {
    pub const ALL: [Self; 58] = [
        Self::ActorSessionSetup,
        Self::PackReload,
        Self::WorldPoll,
        Self::SurfacePreparation,
        Self::RenderSubmission,
        Self::ActorGeometrySetup,
        Self::ActorArtworkSetup,
        Self::ActorEquipmentSetup,
        Self::NetworkIngestion,
        Self::WorldStream,
        Self::CaveVisibility,
        Self::RenderQueueApplication,
        Self::ChunkExtraction,
        Self::GpuPreparation,
        Self::IndirectPreparation,
        Self::TransparentPreparation,
        Self::TransparentWorker,
        Self::OpaqueQueue,
        Self::OpaqueDiagnostics,
        Self::OpaqueBatchPlanning,
        Self::TransparentQueue,
        Self::AcceptanceTelemetry,
        Self::MainFrame,
        Self::ActorPublication,
        Self::ActorAnimation,
        Self::ActorPreparation,
        Self::ActorRigBuild,
        Self::UiPublication,
        Self::UiPreparation,
        Self::Particles,
        Self::Audio,
        Self::BlockEntities,
        Self::RenderFrame,
        Self::GpuFrame,
        Self::GpuShadows,
        Self::GpuOpaque,
        Self::GpuTransparent,
        Self::GpuUi,
        Self::GpuHand,
        Self::GpuPost,
        Self::GpuTonemapping,
        Self::GpuFxaa,
        Self::GpuBlit,
        Self::GpuTerrainOpaque,
        Self::GpuTerrainTransparent,
        Self::GpuActors,
        Self::GpuParticles,
        Self::GpuSky,
        Self::GpuPanorama,
        Self::GpuModPass0,
        Self::GpuModPass1,
        Self::GpuModPass2,
        Self::GpuModPass3,
        Self::GpuModPass4,
        Self::GpuModPass5,
        Self::GpuModPass6,
        Self::GpuModPass7,
        Self::GpuModPrimitives,
    ];

    /// GPU-timed stages, the contiguous tail of [`Self::ALL`].
    pub const GPU: [Self; 25] = [
        Self::GpuFrame,
        Self::GpuShadows,
        Self::GpuOpaque,
        Self::GpuTransparent,
        Self::GpuUi,
        Self::GpuHand,
        Self::GpuPost,
        Self::GpuTonemapping,
        Self::GpuFxaa,
        Self::GpuBlit,
        Self::GpuTerrainOpaque,
        Self::GpuTerrainTransparent,
        Self::GpuActors,
        Self::GpuParticles,
        Self::GpuSky,
        Self::GpuPanorama,
        Self::GpuModPass0,
        Self::GpuModPass1,
        Self::GpuModPass2,
        Self::GpuModPass3,
        Self::GpuModPass4,
        Self::GpuModPass5,
        Self::GpuModPass6,
        Self::GpuModPass7,
        Self::GpuModPrimitives,
    ];

    /// Mod post-pass slots; [`mod_api::MAX_RENDER_PASSES`] long.
    pub const GPU_MOD_PASSES: [Self; mod_api::MAX_RENDER_PASSES] = [
        Self::GpuModPass0,
        Self::GpuModPass1,
        Self::GpuModPass2,
        Self::GpuModPass3,
        Self::GpuModPass4,
        Self::GpuModPass5,
        Self::GpuModPass6,
        Self::GpuModPass7,
    ];

    /// Position within [`Self::GPU`], or `None` for CPU stages.
    #[must_use]
    pub const fn gpu_index(self) -> Option<usize> {
        let index = self as usize;
        let first = Self::GpuFrame as usize;
        if index >= first {
            Some(index - first)
        } else {
            None
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ActorSessionSetup => "actor_session_setup",
            Self::PackReload => "pack_reload",
            Self::WorldPoll => "world_poll",
            Self::SurfacePreparation => "surface_preparation",
            Self::RenderSubmission => "render_submission",
            Self::ActorGeometrySetup => "actor_geometry_setup",
            Self::ActorArtworkSetup => "actor_artwork_setup",
            Self::ActorEquipmentSetup => "actor_equipment_setup",
            Self::NetworkIngestion => "network_ingestion",
            Self::WorldStream => "world_stream",
            Self::CaveVisibility => "cave_visibility",
            Self::RenderQueueApplication => "render_queue_application",
            Self::ChunkExtraction => "chunk_extraction",
            Self::GpuPreparation => "gpu_preparation",
            Self::IndirectPreparation => "indirect_preparation",
            Self::TransparentPreparation => "transparent_preparation",
            Self::TransparentWorker => "transparent_worker",
            Self::OpaqueQueue => "opaque_queue",
            Self::OpaqueDiagnostics => "opaque_diagnostics",
            Self::OpaqueBatchPlanning => "opaque_batch_planning",
            Self::TransparentQueue => "transparent_queue",
            Self::AcceptanceTelemetry => "acceptance_telemetry",
            Self::MainFrame => "main_frame",
            Self::ActorPublication => "actor_publication",
            Self::ActorAnimation => "actor_animation",
            Self::ActorPreparation => "actor_preparation",
            Self::ActorRigBuild => "actor_rig_build",
            Self::UiPublication => "ui_publication",
            Self::UiPreparation => "ui_preparation",
            Self::Particles => "particles",
            Self::Audio => "audio",
            Self::BlockEntities => "block_entities",
            Self::RenderFrame => "render_frame",
            Self::GpuFrame => "gpu_frame",
            Self::GpuShadows => "gpu_shadows",
            Self::GpuOpaque => "gpu_opaque",
            Self::GpuTransparent => "gpu_transparent",
            Self::GpuUi => "gpu_ui",
            Self::GpuHand => "gpu_hand",
            Self::GpuPost => "gpu_post",
            Self::GpuTonemapping => "gpu_tonemapping",
            Self::GpuFxaa => "gpu_fxaa",
            Self::GpuBlit => "gpu_blit",
            Self::GpuTerrainOpaque => "gpu_terrain_opaque",
            Self::GpuTerrainTransparent => "gpu_terrain_transparent",
            Self::GpuActors => "gpu_actors",
            Self::GpuParticles => "gpu_particles",
            Self::GpuSky => "gpu_sky",
            Self::GpuPanorama => "gpu_panorama",
            Self::GpuModPass0 => "gpu_mod_pass_0",
            Self::GpuModPass1 => "gpu_mod_pass_1",
            Self::GpuModPass2 => "gpu_mod_pass_2",
            Self::GpuModPass3 => "gpu_mod_pass_3",
            Self::GpuModPass4 => "gpu_mod_pass_4",
            Self::GpuModPass5 => "gpu_mod_pass_5",
            Self::GpuModPass6 => "gpu_mod_pass_6",
            Self::GpuModPass7 => "gpu_mod_pass_7",
            Self::GpuModPrimitives => "gpu_mod_primitives",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeStageSample {
    pub count: u64,
    pub total: Duration,
    pub maximum: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeStageProfileSnapshot {
    pub interval: Duration,
    pub samples: [RuntimeStageSample; RuntimeStage::ALL.len()],
}

#[derive(Debug, Default)]
struct StageSampleAccumulator {
    sample: Mutex<RuntimeStageSample>,
}

impl StageSampleAccumulator {
    fn record(&self, elapsed: Duration) {
        let mut sample = self
            .sample
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sample.count = sample.count.saturating_add(1);
        sample.total = sample.total.saturating_add(elapsed);
        sample.maximum = sample.maximum.max(elapsed);
    }

    fn take(&self) -> RuntimeStageSample {
        let mut sample = self
            .sample
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        std::mem::take(&mut *sample)
    }
}

#[derive(Debug)]
struct RuntimeStageProfileState {
    enabled: bool,
    slow: Option<crate::runtime_profile_slow::SlowFrameRecorder>,
    trace: Option<crate::runtime_profile_trace::FrameTrace>,
    started: Instant,
    last_snapshot_nanos: AtomicU64,
    stages: [StageSampleAccumulator; RuntimeStage::ALL.len()],
    latest_gpu: Mutex<Option<crate::GpuFrameTimes>>,
}

#[derive(Resource, Debug, Clone)]
pub struct RuntimeStageProfiler {
    state: Arc<RuntimeStageProfileState>,
}

impl Default for RuntimeStageProfiler {
    fn default() -> Self {
        Self::new(false)
    }
}

impl RuntimeStageProfiler {
    #[must_use]
    pub fn new(enabled: bool) -> Self {
        Self::with_trace(enabled, None)
    }

    /// Enables bounded frame spans saved at shutdown when a trace path is supplied.
    pub fn with_trace(enabled: bool, path: Option<std::path::PathBuf>) -> Self {
        Self::build(enabled, path, false)
    }

    /// Keeps cheap slow-frame attribution on during normal play; full traces remain opt-in.
    pub fn for_gameplay(enabled: bool, path: Option<std::path::PathBuf>) -> Self {
        Self::build(enabled, path, true)
    }

    /// Constructs aggregate profiling and the independent slow-frame recorder.
    fn build(enabled: bool, path: Option<std::path::PathBuf>, slow: bool) -> Self {
        let started = Instant::now();
        Self {
            state: Arc::new(RuntimeStageProfileState {
                enabled,
                slow: slow.then(crate::runtime_profile_slow::SlowFrameRecorder::default),
                trace: path
                    .filter(|_| enabled)
                    .map(|path| crate::runtime_profile_trace::FrameTrace::new(path, started)),
                started,
                last_snapshot_nanos: AtomicU64::new(0),
                stages: std::array::from_fn(|_| StageSampleAccumulator::default()),
                latest_gpu: Mutex::new(None),
            }),
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.state.enabled
    }

    /// Times a stage without allocation; normal play records only atomic nanosecond totals.
    pub fn time(&self, stage: RuntimeStage) -> RuntimeStageTimer<'_> {
        RuntimeStageTimer {
            state: self.active().then_some(&*self.state),
            stage,
            started: self.active().then(Instant::now),
        }
    }

    /// Reports the preceding slow frame and starts a new attribution window.
    pub fn begin_frame(&self, focused: bool, occluded: bool) {
        let Some(slow) = &self.state.slow else {
            return;
        };
        if let (Some(event), Some(trace)) = (
            slow.begin_frame(Instant::now(), focused, occluded),
            &self.state.trace,
        ) {
            trace.slow_frame(event);
        }
    }

    /// Sets the display interval that slow-frame thresholds and stage budgets derive from.
    pub fn set_frame_interval(&self, interval: Duration) {
        if let Some(slow) = &self.state.slow {
            slow.set_interval(interval);
        }
    }

    /// The display interval last set, or zero before the first window check.
    #[must_use]
    pub fn frame_interval(&self) -> Duration {
        self.state
            .slow
            .as_ref()
            .map_or(Duration::ZERO, |slow| slow.interval())
    }

    /// Cumulative slow frames and hitches, including those whose text was rate-limited.
    #[must_use]
    pub fn slow_frame_counts(&self) -> Option<crate::SlowFrameCounts> {
        self.state.slow.as_ref().map(|slow| slow.counts())
    }

    /// Records one read-back GPU frame, which typically trails the CPU by a few frames.
    pub fn record_gpu_frame(&self, frame: &crate::GpuFrameTimes) {
        if let Some(trace) = &self.state.trace {
            trace.gpu_frame(frame);
        }
        for (stage, elapsed) in frame.iter() {
            if self.state.enabled {
                self.state.stages[stage as usize].record(elapsed);
            }
            if let Some(event) = self
                .state
                .slow
                .as_ref()
                .and_then(|slow| slow.record(stage, elapsed))
                && let Some(trace) = &self.state.trace
            {
                trace.slow_frame(event);
            }
        }
        *self
            .state
            .latest_gpu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(*frame);
    }

    /// The most recent GPU frame read back, if the adapter supports timestamps.
    #[must_use]
    pub fn latest_gpu_frame(&self) -> Option<crate::GpuFrameTimes> {
        *self
            .state
            .latest_gpu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Whether either aggregate profiling or gameplay attribution needs stage spans.
    fn active(&self) -> bool {
        self.state.enabled || self.state.slow.is_some()
    }

    /// Marks the main update boundary and its current window focus.
    pub fn trace_frame(&self, focused: bool, occluded: bool, game_seconds: f64) {
        if let Some(trace) = &self.state.trace {
            trace.frame(focused, occluded, game_seconds);
        }
    }

    /// Saves the optional frame trace after the update loop has stopped.
    pub fn flush_trace(&self) {
        if let Some(trace) = &self.state.trace {
            trace.flush();
        }
    }

    pub fn take_snapshot_if_due(
        &self,
        minimum_interval: Duration,
    ) -> Option<RuntimeStageProfileSnapshot> {
        if !self.state.enabled {
            return None;
        }
        let now = duration_nanos(self.state.started.elapsed());
        let minimum = duration_nanos(minimum_interval);
        let previous = self.state.last_snapshot_nanos.load(Ordering::Acquire);
        if now.saturating_sub(previous) < minimum
            || self
                .state
                .last_snapshot_nanos
                .compare_exchange(previous, now, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return None;
        }
        Some(RuntimeStageProfileSnapshot {
            interval: Duration::from_nanos(now.saturating_sub(previous)),
            samples: std::array::from_fn(|index| self.state.stages[index].take()),
        })
    }
}

/// Open spans timed across several systems, indexed by [`RuntimeStage`].
#[derive(Resource, Debug)]
pub struct RuntimeStageSpans {
    open: [Option<Instant>; RuntimeStage::ALL.len()],
    /// Each stage's most recently closed span.
    last: [Duration; RuntimeStage::ALL.len()],
}

impl Default for RuntimeStageSpans {
    fn default() -> Self {
        Self {
            open: [None; RuntimeStage::ALL.len()],
            last: [Duration::ZERO; RuntimeStage::ALL.len()],
        }
    }
}

/// Opens the span of stage `S` (a `RuntimeStage as usize`); pair with [`end_stage_span`].
pub fn begin_stage_span<const S: usize>(spans: Option<ResMut<RuntimeStageSpans>>) {
    if let Some(mut spans) = spans {
        spans.open[S] = Some(Instant::now());
    }
}

/// Closes the span of stage `S` and records its wall time.
pub fn end_stage_span<const S: usize>(
    profiler: Option<Res<RuntimeStageProfiler>>,
    spans: Option<ResMut<RuntimeStageSpans>>,
) {
    let Some(mut spans) = spans else {
        return;
    };
    let Some(started) = spans.open[S].take() else {
        return;
    };
    let elapsed = started.elapsed();
    spans.last[S] = elapsed;
    if let Some(profiler) = profiler
        && profiler.active()
    {
        record_elapsed(&profiler.state, RuntimeStage::ALL[S], started, elapsed);
    }
}

/// Closes the render-frame span, excluding the drawable-acquisition wait inside it.
pub(crate) fn end_render_frame_span(
    profiler: Option<Res<RuntimeStageProfiler>>,
    spans: Option<ResMut<RuntimeStageSpans>>,
) {
    const FRAME: usize = RuntimeStage::RenderFrame as usize;
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    let Some(mut spans) = spans else {
        return;
    };
    let Some(started) = spans.open[FRAME].take() else {
        return;
    };
    let wait = std::mem::take(&mut spans.last[SURFACE]);
    let elapsed = render_frame_elapsed(started, Instant::now(), wait);
    spans.last[FRAME] = elapsed;
    if let Some(profiler) = profiler
        && profiler.active()
    {
        record_elapsed(&profiler.state, RuntimeStage::RenderFrame, started, elapsed);
    }
}

#[must_use]
pub struct RuntimeStageTimer<'a> {
    state: Option<&'a RuntimeStageProfileState>,
    stage: RuntimeStage,
    started: Option<Instant>,
}

impl Drop for RuntimeStageTimer<'_> {
    fn drop(&mut self) {
        if let (Some(state), Some(started)) = (self.state, self.started) {
            record_stage(state, self.stage, started);
        }
    }
}

/// Records aggregate timing and an optional timestamped span together.
/// Time from `started` to `now`, less the acquisition wait measured inside it.
fn render_frame_elapsed(started: Instant, now: Instant, wait: Duration) -> Duration {
    now.saturating_duration_since(started).saturating_sub(wait)
}

fn record_stage(state: &RuntimeStageProfileState, stage: RuntimeStage, started: Instant) {
    record_elapsed(state, stage, started, started.elapsed());
}

fn record_elapsed(
    state: &RuntimeStageProfileState,
    stage: RuntimeStage,
    started: Instant,
    elapsed: Duration,
) {
    if state.enabled {
        state.stages[stage as usize].record(elapsed);
    }
    if let Some(slow) = &state.slow {
        slow.record(stage, elapsed);
    }
    if let Some(trace) = &state.trace {
        trace.record(stage, started, elapsed);
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_table_indices_match_and_gpu_stages_form_the_tail() {
        for (index, stage) in RuntimeStage::ALL.into_iter().enumerate() {
            assert_eq!(stage as usize, index);
        }
        let tail = &RuntimeStage::ALL[RuntimeStage::ALL.len() - RuntimeStage::GPU.len()..];
        assert_eq!(tail, RuntimeStage::GPU);
        assert!(
            RuntimeStage::GPU
                .iter()
                .all(|stage| stage.name().starts_with("gpu_"))
        );
        assert_eq!(RuntimeStage::RenderFrame.gpu_index(), None);
    }

    #[test]
    fn gpu_frames_feed_aggregates_and_the_latest_snapshot() {
        let profiler = RuntimeStageProfiler::for_gameplay(true, None);
        assert_eq!(profiler.latest_gpu_frame(), None);
        let frame = crate::gpu_timing::decode_spans([(RuntimeStage::GpuOpaque, 10, 30)], 1.0, true);
        profiler.record_gpu_frame(&frame);
        assert_eq!(profiler.latest_gpu_frame(), Some(frame));
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        for stage in [RuntimeStage::GpuOpaque, RuntimeStage::GpuFrame] {
            assert_eq!(snapshot.samples[stage as usize].count, 1);
        }
    }

    #[test]
    fn every_gpu_overrun_is_marked_in_the_trace() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("trace.json");
        let profiler = RuntimeStageProfiler::for_gameplay(true, Some(path.clone()));
        profiler.set_frame_interval(Duration::from_secs_f64(1.0 / 120.0));
        let over =
            crate::gpu_timing::decode_spans([(RuntimeStage::GpuOpaque, 1, 7_000_001)], 1.0, true);
        profiler.record_gpu_frame(&over);
        profiler.record_gpu_frame(&over);
        profiler.flush_trace();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let markers = saved["traceEvents"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|event| event["name"] == "slow_frame")
            .count();
        assert_eq!(markers, 2);
        assert_eq!(profiler.slow_frame_counts().unwrap().gpu, 2);
    }

    #[test]
    fn render_frame_excludes_only_the_acquisition_wait() {
        let started = Instant::now();
        let now = started + Duration::from_millis(80);
        let elapsed = render_frame_elapsed(started, now, Duration::from_millis(50));
        assert_eq!(elapsed, Duration::from_millis(30));
        assert_eq!(
            render_frame_elapsed(started, now, Duration::from_millis(90)),
            Duration::ZERO
        );
    }

    /// The render-frame span is open from asset preparation through rendering, closed by Cleanup.
    #[test]
    fn render_frame_span_covers_asset_preparation_through_cleanup() {
        use bevy::prelude::*;
        use bevy::render::{Render, RenderSystems};
        const FRAME: usize = RuntimeStage::RenderFrame as usize;
        #[derive(Resource, Default)]
        struct Open(Vec<bool>);
        fn probe(spans: Res<RuntimeStageSpans>, mut open: ResMut<Open>) {
            open.0.push(spans.open[FRAME].is_some());
        }
        let profiler = RuntimeStageProfiler::new(true);
        let mut app = App::new();
        app.add_schedule(Render::base_schedule())
            .insert_resource(profiler.clone())
            .init_resource::<Open>();
        crate::runtime_profile_trace::install_surface_trace(app.main_mut());
        app.add_systems(
            Render,
            (
                probe.in_set(RenderSystems::PrepareAssets),
                probe.in_set(RenderSystems::Render),
                probe.in_set(RenderSystems::PostCleanup),
            )
                .chain(),
        );
        app.world_mut().run_schedule(Render);
        assert_eq!(app.world().resource::<Open>().0, [true, true, false]);
        let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
        assert_eq!(snapshot.samples[FRAME].count, 1);
    }

    #[test]
    fn disabled_profiler_records_nothing() {
        let profiler = RuntimeStageProfiler::new(false);
        drop(profiler.time(RuntimeStage::WorldStream));
        assert_eq!(profiler.take_snapshot_if_due(Duration::ZERO), None);
    }

    #[test]
    fn snapshot_drains_each_stage_interval() {
        let profiler = RuntimeStageProfiler::new(true);
        drop(profiler.time(RuntimeStage::WorldStream));
        let first = profiler
            .take_snapshot_if_due(Duration::ZERO)
            .expect("enabled profiler emits a due snapshot");
        assert_eq!(first.samples[RuntimeStage::WorldStream as usize].count, 1);

        let second = profiler
            .take_snapshot_if_due(Duration::ZERO)
            .expect("zero interval permits another snapshot");
        assert_eq!(second.samples[RuntimeStage::WorldStream as usize].count, 0);
    }

    #[test]
    fn concurrent_records_are_drained_as_complete_samples() {
        let profiler = RuntimeStageProfiler::new(true);
        let workers = (0..4)
            .map(|_| {
                let profiler = profiler.clone();
                std::thread::spawn(move || {
                    for _ in 0..1_000 {
                        drop(profiler.time(RuntimeStage::TransparentWorker));
                    }
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }

        let snapshot = profiler
            .take_snapshot_if_due(Duration::ZERO)
            .expect("enabled profiler emits a due snapshot");
        assert_eq!(
            snapshot.samples[RuntimeStage::TransparentWorker as usize].count,
            4_000
        );
    }
}
