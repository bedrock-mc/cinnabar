use std::time::Duration;

use bevy::{
    prelude::{Res, ResMut, Resource, Time},
    time::Real,
};
use render::ChunkUploadBudget;

const KIBIBYTE: u64 = 1024;
const MEBIBYTE: u64 = 1024 * KIBIBYTE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PublicationControllerConfig {
    pub(crate) target_frame_time: Duration,
    pub(crate) recovery_frame_time: Duration,
    pub(crate) recovery_streak_frames: u32,
    pub(crate) minimum: ChunkUploadBudget,
    pub(crate) initial: ChunkUploadBudget,
    pub(crate) maximum: ChunkUploadBudget,
    pub(crate) additive_items: usize,
    pub(crate) additive_bytes: u64,
    pub(crate) decrease_numerator: usize,
    pub(crate) decrease_denominator: usize,
}

impl Default for PublicationControllerConfig {
    fn default() -> Self {
        Self {
            // `Time<Real>` includes FIFO pacing. Leave a wide overrun band so
            // normal 60 Hz jitter is not mistaken for publication pressure.
            target_frame_time: Duration::from_millis(25),
            recovery_frame_time: Duration::from_millis(19),
            recovery_streak_frames: 120,
            // Keep initial-world convergence independent from a slow baseline
            // frame rate. Dropping below this floor made a 6-8 FPS client
            // publish only two zero-byte removals per frame, which starved
            // real spawn meshes for several minutes and prevented the
            // adaptive controller from ever observing a recoverable frame.
            minimum: ChunkUploadBudget::new(8, 4 * MEBIBYTE),
            initial: ChunkUploadBudget::new(16, 4 * MEBIBYTE),
            maximum: ChunkUploadBudget::new(128, 64 * MEBIBYTE),
            additive_items: 1,
            additive_bytes: 256 * KIBIBYTE,
            decrease_numerator: 3,
            decrease_denominator: 4,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PublicationFrameWork {
    pub(crate) mesh_jobs_dispatched: usize,
    pub(crate) mesh_changes_published: usize,
    pub(crate) mesh_bytes_published: u64,
    pub(crate) pending_mesh_jobs: usize,
    pub(crate) in_flight_mesh_jobs: usize,
    pub(crate) upload_queue_items: usize,
    pub(crate) upload_queue_bytes: u64,
    pub(crate) cohort_expected: usize,
    pub(crate) cohort_loaded: usize,
    pub(crate) resident_meshes: usize,
    pub(crate) cave_visible_meshes: usize,
    pub(crate) frustum_visible_meshes: usize,
    pub(crate) submitted_meshes: usize,
    pub(crate) gpu_completed_meshes: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PublicationDiagnostics {
    pub(crate) frame_sequence: u64,
    pub(crate) observed_frame_time: Duration,
    pub(crate) budget: ChunkUploadBudget,
    pub(crate) under_target_streak: u32,
    pub(crate) multiplicative_decreases: u64,
    pub(crate) additive_increases: u64,
    pub(crate) last_work: PublicationFrameWork,
}

#[derive(Resource, Debug, Clone, Copy)]
pub(crate) struct PublicationController {
    config: PublicationControllerConfig,
    diagnostics: PublicationDiagnostics,
}

impl Default for PublicationController {
    fn default() -> Self {
        Self::new(PublicationControllerConfig::default())
    }
}

impl PublicationController {
    #[must_use]
    pub(crate) fn new(config: PublicationControllerConfig) -> Self {
        assert!(config.decrease_denominator > 0);
        assert!(config.decrease_numerator < config.decrease_denominator);
        assert!(config.recovery_streak_frames > 0);
        let initial = clamp_budget(config.initial, config.minimum, config.maximum);
        Self {
            config,
            diagnostics: PublicationDiagnostics {
                budget: initial,
                ..Default::default()
            },
        }
    }

    pub(crate) fn begin_frame(&mut self, frame_time: Duration) {
        self.diagnostics.frame_sequence = self.diagnostics.frame_sequence.saturating_add(1);
        self.diagnostics.observed_frame_time = frame_time;
        if frame_time > self.config.target_frame_time {
            self.diagnostics.budget = ChunkUploadBudget::new(
                multiplicative_decrease(
                    self.diagnostics.budget.max_per_frame,
                    self.config.decrease_numerator,
                    self.config.decrease_denominator,
                    self.config.minimum.max_per_frame,
                ),
                multiplicative_decrease_u64(
                    self.diagnostics.budget.max_bytes_per_frame,
                    self.config.decrease_numerator,
                    self.config.decrease_denominator,
                    self.config.minimum.max_bytes_per_frame,
                ),
            );
            self.diagnostics.under_target_streak = 0;
            self.diagnostics.multiplicative_decreases =
                self.diagnostics.multiplicative_decreases.saturating_add(1);
            return;
        }
        if frame_time > self.config.recovery_frame_time {
            self.diagnostics.under_target_streak = 0;
            return;
        }
        self.diagnostics.under_target_streak =
            self.diagnostics.under_target_streak.saturating_add(1);
        if self.diagnostics.under_target_streak < self.config.recovery_streak_frames {
            return;
        }
        self.diagnostics.under_target_streak = 0;
        self.diagnostics.budget = ChunkUploadBudget::new(
            self.diagnostics
                .budget
                .max_per_frame
                .saturating_add(self.config.additive_items)
                .min(self.config.maximum.max_per_frame),
            self.diagnostics
                .budget
                .max_bytes_per_frame
                .saturating_add(self.config.additive_bytes)
                .min(self.config.maximum.max_bytes_per_frame),
        );
        self.diagnostics.additive_increases = self.diagnostics.additive_increases.saturating_add(1);
    }

    pub(crate) fn finish_frame(&mut self, work: PublicationFrameWork) {
        self.diagnostics.last_work = work;
    }

    #[must_use]
    pub(crate) const fn budget(&self) -> ChunkUploadBudget {
        self.diagnostics.budget
    }

    #[must_use]
    pub(crate) const fn diagnostics(&self) -> PublicationDiagnostics {
        self.diagnostics
    }
}

pub(crate) fn begin_publication_frame(
    time: Res<Time<Real>>,
    mut controller: ResMut<PublicationController>,
    mut budget: ResMut<ChunkUploadBudget>,
) {
    controller.begin_frame(time.delta());
    *budget = controller.budget();
}

#[must_use]
pub(crate) fn adaptive_publication_diagnostic_line(diagnostics: PublicationDiagnostics) -> String {
    let work = diagnostics.last_work;
    format!(
        "ADAPTIVE_PUBLICATION frame={} frame_us={} cap_items={} cap_bytes={} under_target_streak={} decreases={} increases={} dispatched={} published={} published_bytes={} pending={} in_flight={} upload_items={} upload_bytes={} cohort_loaded={} cohort_expected={} resident={} cave={} frustum={} submitted={} gpu_completed={}",
        diagnostics.frame_sequence,
        diagnostics.observed_frame_time.as_micros(),
        diagnostics.budget.max_per_frame,
        diagnostics.budget.max_bytes_per_frame,
        diagnostics.under_target_streak,
        diagnostics.multiplicative_decreases,
        diagnostics.additive_increases,
        work.mesh_jobs_dispatched,
        work.mesh_changes_published,
        work.mesh_bytes_published,
        work.pending_mesh_jobs,
        work.in_flight_mesh_jobs,
        work.upload_queue_items,
        work.upload_queue_bytes,
        work.cohort_loaded,
        work.cohort_expected,
        work.resident_meshes,
        work.cave_visible_meshes,
        work.frustum_visible_meshes,
        work.submitted_meshes,
        work.gpu_completed_meshes,
    )
}

fn clamp_budget(
    budget: ChunkUploadBudget,
    minimum: ChunkUploadBudget,
    maximum: ChunkUploadBudget,
) -> ChunkUploadBudget {
    ChunkUploadBudget::new(
        budget
            .max_per_frame
            .clamp(minimum.max_per_frame, maximum.max_per_frame),
        budget
            .max_bytes_per_frame
            .clamp(minimum.max_bytes_per_frame, maximum.max_bytes_per_frame),
    )
}

fn multiplicative_decrease(
    value: usize,
    numerator: usize,
    denominator: usize,
    minimum: usize,
) -> usize {
    value
        .saturating_mul(numerator)
        .checked_div(denominator)
        .unwrap_or(minimum)
        .max(minimum)
}

fn multiplicative_decrease_u64(
    value: u64,
    numerator: usize,
    denominator: usize,
    minimum: u64,
) -> u64 {
    value
        .saturating_mul(u64::try_from(numerator).unwrap_or(u64::MAX))
        .checked_div(u64::try_from(denominator).unwrap_or(u64::MAX))
        .unwrap_or(minimum)
        .max(minimum)
}
