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
}

impl RuntimeStage {
    pub const ALL: [Self; 32] = [
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
    ];

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
        if let Some(slow) = &self.state.slow {
            slow.begin_frame(Instant::now(), focused, occluded);
        }
    }

    /// Whether either aggregate profiling or gameplay attribution needs stage spans.
    fn active(&self) -> bool {
        self.state.enabled || self.state.slow.is_some()
    }

    /// Marks the main update boundary and its current window focus.
    pub fn trace_frame(&self, focused: bool, occluded: bool) {
        if let Some(trace) = &self.state.trace {
            trace.frame(focused, occluded);
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
#[derive(Resource, Debug, Default)]
pub struct RuntimeStageSpans([Option<Instant>; RuntimeStage::ALL.len()]);

/// Opens the span of stage `S` (a `RuntimeStage as usize`); pair with [`end_stage_span`].
pub fn begin_stage_span<const S: usize>(spans: Option<ResMut<RuntimeStageSpans>>) {
    if let Some(mut spans) = spans {
        spans.0[S] = Some(Instant::now());
    }
}

/// Closes the span of stage `S` and records its wall time.
pub fn end_stage_span<const S: usize>(
    profiler: Option<Res<RuntimeStageProfiler>>,
    spans: Option<ResMut<RuntimeStageSpans>>,
) {
    if let (Some(profiler), Some(started)) =
        (profiler, spans.and_then(|mut spans| spans.0[S].take()))
        && profiler.active()
    {
        record_stage(&profiler.state, RuntimeStage::ALL[S], started);
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
fn record_stage(state: &RuntimeStageProfileState, stage: RuntimeStage, started: Instant) {
    let elapsed = started.elapsed();
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
