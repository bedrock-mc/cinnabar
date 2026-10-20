use std::{
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use web_time::Instant;

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
    /// Summed between-frames service polls, received as a sample when the stream returns.
    WorldService,
    /// Frame-thread wait for the world-stream service to hand the stream back.
    WorldServiceReclaim,
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
    /// Main-world wall time from frame-start timing to `Last`, including stream reclaim.
    MainFrame,
    /// `MainFrame` split by main-schedule phase, so work outside every named stage still names
    /// the phase that spent it: `PreUpdate`, the fixed-update loop, `Update`, and `PostUpdate`.
    MainPreUpdate,
    MainFixedUpdate,
    MainUpdate,
    MainPostUpdate,
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
    /// Update start, when input is sampled, to the end of that frame's render extraction.
    InputAge,
    /// Main-thread wait that delays the next input sample to its cadence slot or toward
    /// render-thread completion.
    InputPacingWait,
    /// How far past its deadline the pacing wait woke.
    FramePacingLateness,
    /// First to last sampled GPU timestamp; on Metal, the graph submission's marker span.
    GpuFrame,
    GpuShadows,
    GpuOpaque,
    GpuTransparent,
    GpuUi,
    /// Owned UI passes measured only during explicit UI profiling.
    GpuUiRaster,
    GpuUiModel,
    GpuUiComposite,
    GpuUiInvert,
    /// The shared scene blur and mod-surface backdrop passes.
    GpuUiBackdrop,
    GpuHand,
    GpuPost,
    GpuTonemapping,
    GpuFxaa,
    GpuBlit,
    /// Owned GPU-cull passes, measured during per-pass profiling outside Metal.
    GpuCullEarly,
    GpuCullHiz,
    GpuCullLate,
    GpuTerrainLate,
    /// Draw categories timed inside passes; only with aggregate profiling on capable adapters.
    GpuTerrainOpaque,
    GpuTerrainTransparent,
    /// Opaque draw categories measured only during explicit profiling.
    GpuTerrainSolid,
    GpuTerrainCutout,
    GpuTerrainModel,
    GpuTerrainDepthLiquid,
    GpuOpaqueOther,
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
    GpuFarTerrain,
    /// CPU time from submission admission to its completion callback, including queue and poll delay.
    GpuFrameCompletionLatency,
}

impl RuntimeStage {
    pub const ALL: [Self; 83] = [
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
        Self::WorldService,
        Self::WorldServiceReclaim,
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
        Self::MainPreUpdate,
        Self::MainFixedUpdate,
        Self::MainUpdate,
        Self::MainPostUpdate,
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
        Self::InputAge,
        Self::InputPacingWait,
        Self::FramePacingLateness,
        Self::GpuFrame,
        Self::GpuShadows,
        Self::GpuOpaque,
        Self::GpuTransparent,
        Self::GpuUi,
        Self::GpuUiRaster,
        Self::GpuUiModel,
        Self::GpuUiComposite,
        Self::GpuUiInvert,
        Self::GpuUiBackdrop,
        Self::GpuHand,
        Self::GpuPost,
        Self::GpuTonemapping,
        Self::GpuFxaa,
        Self::GpuBlit,
        Self::GpuCullEarly,
        Self::GpuCullHiz,
        Self::GpuCullLate,
        Self::GpuTerrainLate,
        Self::GpuTerrainOpaque,
        Self::GpuTerrainTransparent,
        Self::GpuTerrainSolid,
        Self::GpuTerrainCutout,
        Self::GpuTerrainModel,
        Self::GpuTerrainDepthLiquid,
        Self::GpuOpaqueOther,
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
        Self::GpuFarTerrain,
        Self::GpuFrameCompletionLatency,
    ];

    /// GPU diagnostics, including labelled completion latency, in the tail of [`Self::ALL`].
    pub const GPU: [Self; 41] = [
        Self::GpuFrame,
        Self::GpuShadows,
        Self::GpuOpaque,
        Self::GpuTransparent,
        Self::GpuUi,
        Self::GpuUiRaster,
        Self::GpuUiModel,
        Self::GpuUiComposite,
        Self::GpuUiInvert,
        Self::GpuUiBackdrop,
        Self::GpuHand,
        Self::GpuPost,
        Self::GpuTonemapping,
        Self::GpuFxaa,
        Self::GpuBlit,
        Self::GpuCullEarly,
        Self::GpuCullHiz,
        Self::GpuCullLate,
        Self::GpuTerrainLate,
        Self::GpuTerrainOpaque,
        Self::GpuTerrainTransparent,
        Self::GpuTerrainSolid,
        Self::GpuTerrainCutout,
        Self::GpuTerrainModel,
        Self::GpuTerrainDepthLiquid,
        Self::GpuOpaqueOther,
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
        Self::GpuFarTerrain,
        Self::GpuFrameCompletionLatency,
    ];

    /// Owned UI passes separated from the enclosing UI node when explicitly requested.
    pub const GPU_UI: [Self; 5] = [
        Self::GpuUiRaster,
        Self::GpuUiModel,
        Self::GpuUiComposite,
        Self::GpuUiInvert,
        Self::GpuUiBackdrop,
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
            Self::WorldService => "world_service",
            Self::WorldServiceReclaim => "world_service_reclaim",
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
            Self::MainPreUpdate => "main_pre_update",
            Self::MainFixedUpdate => "main_fixed_update",
            Self::MainUpdate => "main_update",
            Self::MainPostUpdate => "main_post_update",
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
            Self::InputAge => "input_age",
            Self::InputPacingWait => "input_pacing_wait",
            Self::FramePacingLateness => "frame_pacing_lateness",
            Self::GpuFrame => "gpu_frame",
            Self::GpuFrameCompletionLatency => "gpu_frame_completion_latency",
            Self::GpuShadows => "gpu_shadows",
            Self::GpuOpaque => "gpu_opaque",
            Self::GpuTransparent => "gpu_transparent",
            Self::GpuUi => "gpu_ui",
            Self::GpuUiRaster => "gpu_ui_raster",
            Self::GpuUiModel => "gpu_ui_model",
            Self::GpuUiComposite => "gpu_ui_composite",
            Self::GpuUiInvert => "gpu_ui_invert",
            Self::GpuUiBackdrop => "gpu_ui_backdrop",
            Self::GpuHand => "gpu_hand",
            Self::GpuPost => "gpu_post",
            Self::GpuTonemapping => "gpu_tonemapping",
            Self::GpuFxaa => "gpu_fxaa",
            Self::GpuBlit => "gpu_blit",
            Self::GpuCullEarly => "gpu_cull_early",
            Self::GpuCullHiz => "gpu_cull_hiz",
            Self::GpuCullLate => "gpu_cull_late",
            Self::GpuTerrainLate => "gpu_terrain_late",
            Self::GpuTerrainOpaque => "gpu_terrain_opaque",
            Self::GpuTerrainTransparent => "gpu_terrain_transparent",
            Self::GpuTerrainSolid => "gpu_terrain_solid",
            Self::GpuTerrainCutout => "gpu_terrain_cutout",
            Self::GpuTerrainModel => "gpu_terrain_model",
            Self::GpuTerrainDepthLiquid => "gpu_terrain_depth_liquid",
            Self::GpuOpaqueOther => "gpu_opaque_other",
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
            Self::GpuFarTerrain => "gpu_far_terrain",
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

/// Creates the native lock during setup so its first timed use cannot allocate.
pub(super) fn initialized_mutex<T>(value: T) -> Mutex<T> {
    let mutex = Mutex::new(value);
    drop(
        mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    mutex
}

#[derive(Debug)]
struct StageSampleAccumulator {
    sample: Mutex<RuntimeStageSample>,
}

impl Default for StageSampleAccumulator {
    /// Prepares the sample and its lock before any timed stage runs.
    fn default() -> Self {
        Self {
            sample: initialized_mutex(RuntimeStageSample::default()),
        }
    }
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
    frame_id: AtomicU64,
    diagnostic_sink: OnceLock<DiagnosticSink>,
    slow: Option<crate::runtime_profile_slow::SlowFrameRecorder>,
    trace: Option<crate::runtime_profile_trace::FrameTrace>,
    started: Instant,
    last_snapshot_nanos: AtomicU64,
    stages: [StageSampleAccumulator; RuntimeStage::ALL.len()],
    latest_gpu: Mutex<Option<crate::GpuFrameTimes>>,
    #[cfg(any(test, feature = "alloc-stats"))]
    allocations: crate::runtime_profile_alloc::AllocationProfiler,
}

/// Queues an owned report for background formatting without adding a crate dependency.
pub type DiagnosticSink = fn(Box<dyn std::fmt::Display + Send>);

#[derive(Resource, Debug, Clone)]
pub struct RuntimeStageProfiler {
    state: Arc<RuntimeStageProfileState>,
    /// Render-world copies keep the input identity from their extraction.
    frame_id: Option<u64>,
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

    /// Keeps bounded rolling attribution on during normal play; file exports remain opt-in.
    pub fn for_gameplay(enabled: bool, path: Option<std::path::PathBuf>) -> Self {
        Self::build(enabled, path, true)
    }

    /// Constructs aggregate profiling and the independent slow-frame recorder.
    fn build(enabled: bool, path: Option<std::path::PathBuf>, slow: bool) -> Self {
        let started = Instant::now();
        Self {
            state: Arc::new(RuntimeStageProfileState {
                enabled,
                frame_id: AtomicU64::new(0),
                diagnostic_sink: OnceLock::new(),
                slow: slow.then(crate::runtime_profile_slow::SlowFrameRecorder::default),
                trace: (slow || enabled && path.is_some()).then(|| {
                    crate::runtime_profile_trace::FrameTrace::new(path.filter(|_| enabled), started)
                }),
                started,
                last_snapshot_nanos: AtomicU64::new(0),
                stages: std::array::from_fn(|_| StageSampleAccumulator::default()),
                latest_gpu: initialized_mutex(None),
                #[cfg(any(test, feature = "alloc-stats"))]
                allocations: crate::runtime_profile_alloc::AllocationProfiler::default(),
            }),
            frame_id: None,
        }
    }

    /// Installs the process diagnostics queue before the profiler is shared with rendering.
    pub fn with_diagnostic_sink(self, sink: DiagnosticSink) -> Self {
        let _ = self.state.diagnostic_sink.set(sink);
        self
    }

    /// Current input identity, or the identity retained by an extracted render-world copy.
    pub fn frame_id(&self) -> u64 {
        self.frame_id
            .unwrap_or_else(|| self.state.frame_id.load(Ordering::Relaxed))
    }

    /// Binds a shared recorder to this input frame before render-world work starts.
    pub fn for_frame(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
            frame_id: Some(self.frame_id()),
        }
    }

    /// Hands an owned snapshot to the process queue after releasing recorder locks.
    fn report(&self, report: impl std::fmt::Display + Send + 'static) {
        if let Some(sink) = self.state.diagnostic_sink.get() {
            sink(Box::new(report));
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.state.enabled
    }

    /// Times a stage without allocation, retaining its input identity until it closes.
    pub fn time(&self, stage: RuntimeStage) -> RuntimeStageTimer<'_> {
        RuntimeStageTimer {
            state: self.active().then_some(&*self.state),
            stage,
            frame_id: self.frame_id(),
            started: self.active().then(Instant::now),
            #[cfg(any(test, feature = "alloc-stats"))]
            allocations: self
                .active()
                .then(crate::runtime_profile_alloc::thread_snapshot),
        }
    }

    /// Reports the preceding slow frame and starts a new attribution window.
    pub fn begin_frame(&self, focused: bool, occluded: bool) {
        crate::runtime_profile_uploads::finish_frame();
        #[cfg(any(test, feature = "alloc-stats"))]
        let allocations = self.state.allocations.finish_frame();
        #[cfg(any(test, feature = "alloc-stats"))]
        allocations.plot();
        self.state.frame_id.fetch_add(1, Ordering::Relaxed);
        if let Some(slow) = &self.state.slow {
            let (event, report) = slow.begin_frame(Instant::now(), focused, occluded);
            if let Some(event) = event
                && let Some(trace) = &self.state.trace
            {
                trace.slow_frame(event);
            }
            if let Some(report) = report
                && self.state.diagnostic_sink.get().is_some()
            {
                self.report(diagnostic::SlowFrameDiagnostic {
                    timing: report,
                    #[cfg(any(test, feature = "alloc-stats"))]
                    allocations,
                    uploads: crate::runtime_profile_uploads::snapshot(),
                });
            }
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
        if frame.iter().any(|(stage, _)| {
            !matches!(
                stage,
                RuntimeStage::GpuFrame | RuntimeStage::GpuFrameCompletionLatency
            )
        }) {
            *self
                .state
                .latest_gpu
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(*frame);
        }
    }

    /// The most recent read-back GPU frame that timed individual passes, if the adapter
    /// supports timestamps. Frames that timed only the whole frame leave it unchanged.
    #[must_use]
    pub fn latest_pass_frame(&self) -> Option<crate::GpuFrameTimes> {
        *self
            .state
            .latest_gpu
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Records a span measured outside a system, such as across the render handoff.
    pub fn record(&self, stage: RuntimeStage, started: Instant, elapsed: Duration) {
        if self.active() {
            record_elapsed(&self.state, self.frame_id(), stage, started, elapsed);
        }
    }

    /// Records accumulated worker time as a counter received on the calling thread.
    pub fn record_background_sample(&self, stage: RuntimeStage, elapsed: Duration) {
        if self.state.enabled {
            self.state.stages[stage as usize].record(elapsed);
        }
        if let Some(slow) = &self.state.slow {
            slow.record(stage, elapsed);
        }
        if let Some(trace) = &self.state.trace {
            trace.background_sample(stage, elapsed);
        }
    }

    /// Whether either aggregate profiling or gameplay attribution needs stage spans.
    fn active(&self) -> bool {
        self.state.enabled || self.state.slow.is_some()
    }

    /// Marks the main update boundary and its current window focus.
    pub fn trace_frame(&self, focused: bool, occluded: bool, game_seconds: f64) {
        if let Some(trace) = &self.state.trace {
            for report in trace.frame(self.frame_id(), focused, occluded, game_seconds) {
                self.report(report);
            }
        }
    }

    /// Starts optional trace export on a worker without blocking the update loop.
    pub fn flush_trace(&self) {
        if let Some(trace) = &self.state.trace {
            for report in trace.take_hitches(None) {
                self.report(report);
            }
            trace.flush();
        }
    }

    /// Waits for a requested export during teardown or before consuming the saved trace.
    pub fn wait_for_trace_export(&self) {
        if let Some(trace) = &self.state.trace {
            trace.wait_for_export();
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
    open: [Option<(Instant, u64)>; RuntimeStage::ALL.len()],
    /// Each stage's most recently closed span.
    last: [Duration; RuntimeStage::ALL.len()],
    #[cfg(any(test, feature = "alloc-stats"))]
    allocations: [Option<test_alloc::Allocations>; RuntimeStage::ALL.len()],
}

#[cfg(any(test, feature = "publication-test-support"))]
impl RuntimeStageSpans {
    /// Reports whether a stage has started but has not yet closed.
    pub fn is_open(&self, stage: RuntimeStage) -> bool {
        self.open[stage as usize].is_some()
    }
}

impl Default for RuntimeStageSpans {
    fn default() -> Self {
        Self {
            open: [None; RuntimeStage::ALL.len()],
            last: [Duration::ZERO; RuntimeStage::ALL.len()],
            #[cfg(any(test, feature = "alloc-stats"))]
            allocations: [None; RuntimeStage::ALL.len()],
        }
    }
}

/// Opens the span of stage `S` (a `RuntimeStage as usize`); pair with [`end_stage_span`].
pub fn begin_stage_span<const S: usize>(
    spans: Option<ResMut<RuntimeStageSpans>>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    if let Some(mut spans) = spans {
        spans.open[S] = Some((
            Instant::now(),
            profiler.as_ref().map_or(0, |profiler| profiler.frame_id()),
        ));
        #[cfg(any(test, feature = "alloc-stats"))]
        {
            spans.allocations[S] = Some(test_alloc::global_allocations());
        }
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
    let Some((started, frame_id)) = spans.open[S].take() else {
        return;
    };
    let elapsed = started.elapsed();
    spans.last[S] = elapsed;
    if let Some(profiler) = profiler
        && profiler.active()
    {
        #[cfg(any(test, feature = "alloc-stats"))]
        if let Some(before) = spans.allocations[S].take() {
            profiler
                .state
                .allocations
                .record_global_since(RuntimeStage::ALL[S], before);
        }
        record_elapsed(
            &profiler.state,
            frame_id,
            RuntimeStage::ALL[S],
            started,
            elapsed,
        );
    }
}

/// Closes the render-frame span, excluding the drawable-acquisition wait inside it.
pub fn end_render_frame_span(
    profiler: Option<Res<RuntimeStageProfiler>>,
    spans: Option<ResMut<RuntimeStageSpans>>,
) {
    const FRAME: usize = RuntimeStage::RenderFrame as usize;
    const SURFACE: usize = RuntimeStage::SurfacePreparation as usize;
    let Some(mut spans) = spans else {
        return;
    };
    let Some((started, _)) = spans.open[FRAME].take() else {
        return;
    };
    let wait = std::mem::take(&mut spans.last[SURFACE]);
    let elapsed = render_frame_elapsed(started, Instant::now(), wait);
    spans.last[FRAME] = elapsed;
    if let Some(profiler) = profiler
        && profiler.active()
    {
        #[cfg(any(test, feature = "alloc-stats"))]
        if let Some(before) = spans.allocations[FRAME].take() {
            profiler
                .state
                .allocations
                .record_global_since(RuntimeStage::RenderFrame, before);
        }
        record_elapsed(
            &profiler.state,
            profiler.frame_id(),
            RuntimeStage::RenderFrame,
            started,
            elapsed,
        );
    }
}

#[must_use]
pub struct RuntimeStageTimer<'a> {
    state: Option<&'a RuntimeStageProfileState>,
    stage: RuntimeStage,
    frame_id: u64,
    started: Option<Instant>,
    #[cfg(any(test, feature = "alloc-stats"))]
    allocations: Option<crate::runtime_profile_alloc::ThreadSnapshot>,
}

impl Drop for RuntimeStageTimer<'_> {
    fn drop(&mut self) {
        if let (Some(state), Some(started)) = (self.state, self.started) {
            #[cfg(any(test, feature = "alloc-stats"))]
            if let Some(before) = self.allocations {
                state.allocations.record_since(self.stage, before);
            }
            record_elapsed(state, self.frame_id, self.stage, started, started.elapsed());
        }
    }
}

/// Records aggregate timing and an optional timestamped span together.
/// Time from `started` to `now`, less the acquisition wait measured inside it.
fn render_frame_elapsed(started: Instant, now: Instant, wait: Duration) -> Duration {
    now.saturating_duration_since(started).saturating_sub(wait)
}

/// Records a completed span in each enabled diagnostic consumer with its originating frame.
fn record_elapsed(
    state: &RuntimeStageProfileState,
    frame_id: u64,
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
        trace.record(frame_id, stage, started, elapsed);
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;

mod diagnostic;

#[cfg(test)]
mod allocation_tests;
