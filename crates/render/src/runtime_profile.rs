use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bevy::prelude::Resource;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
pub enum RuntimeStage {
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
}

impl RuntimeStage {
    pub const ALL: [Self; 14] = [
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
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
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
        Self {
            state: Arc::new(RuntimeStageProfileState {
                enabled,
                started: Instant::now(),
                last_snapshot_nanos: AtomicU64::new(0),
                stages: std::array::from_fn(|_| StageSampleAccumulator::default()),
            }),
        }
    }

    #[must_use]
    pub fn enabled(&self) -> bool {
        self.state.enabled
    }

    pub fn time(&self, stage: RuntimeStage) -> RuntimeStageTimer<'_> {
        RuntimeStageTimer {
            sample: self
                .state
                .enabled
                .then_some(&self.state.stages[stage as usize]),
            started: self.state.enabled.then(Instant::now),
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

#[must_use]
pub struct RuntimeStageTimer<'a> {
    sample: Option<&'a StageSampleAccumulator>,
    started: Option<Instant>,
}

impl Drop for RuntimeStageTimer<'_> {
    fn drop(&mut self) {
        if let (Some(sample), Some(started)) = (self.sample, self.started) {
            sample.record(started.elapsed());
        }
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
