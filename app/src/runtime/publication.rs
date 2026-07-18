use std::time::Duration;

use bevy::{
    prelude::{Res, ResMut, Resource, Time},
    time::Real,
};
use client_world::{PublicationAllowance, PublicationServiceConfig};
use render::ChunkUploadBudget;

const NANOS_PER_SECOND: u128 = 1_000_000_000;
const PRESSURE_FRAME_TIME: Duration = Duration::from_millis(25);
const PACED_LOW_FREQUENCY_FRAME_TIME: Duration = Duration::from_millis(100);
const RECOVERY_STREAK_FRAMES: u32 = 120;
const RENDER_QUEUE_PRESSURE_ITEMS: usize = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PublicationFrameWork {
    pub(crate) mesh_jobs_dispatched: usize,
    pub(crate) mesh_changes_published: usize,
    pub(crate) mesh_payloads_published: usize,
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

impl PublicationFrameWork {
    #[must_use]
    pub(crate) const fn healthy() -> Self {
        Self {
            mesh_jobs_dispatched: 0,
            mesh_changes_published: 0,
            mesh_payloads_published: 0,
            mesh_bytes_published: 0,
            pending_mesh_jobs: 0,
            in_flight_mesh_jobs: 0,
            upload_queue_items: 0,
            upload_queue_bytes: 0,
            cohort_expected: 0,
            cohort_loaded: 0,
            resident_meshes: 0,
            cave_visible_meshes: 0,
            frustum_visible_meshes: 0,
            submitted_meshes: 0,
            gpu_completed_meshes: 0,
        }
    }
}

impl Default for PublicationFrameWork {
    fn default() -> Self {
        Self::healthy()
    }
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

#[derive(Resource, Debug, Clone)]
pub(crate) struct PublicationController {
    config: PublicationServiceConfig,
    item_rate_per_second: u32,
    byte_rate_per_second: u64,
    allowance: PublicationAllowance,
    item_numerator_remainder: u128,
    byte_numerator_remainder: u128,
    diagnostics: PublicationDiagnostics,
}

impl Default for PublicationController {
    fn default() -> Self {
        Self::new(PublicationServiceConfig::PHASE2_GATE)
    }
}

impl PublicationController {
    #[must_use]
    pub(crate) fn new(config: PublicationServiceConfig) -> Self {
        assert!(config.minimum_items_per_second > 0);
        assert!(config.minimum_bytes_per_second > 0);
        assert!(config.minimum_items_per_second <= config.target_items_per_second);
        assert!(config.minimum_bytes_per_second <= config.target_bytes_per_second);
        assert!(config.maximum_frame_items <= config.maximum_burst_items);
        assert!(config.maximum_frame_bytes <= config.maximum_burst_bytes);
        let budget = frame_budget(config, 0, 0);
        Self {
            config,
            item_rate_per_second: config.target_items_per_second,
            byte_rate_per_second: config.target_bytes_per_second,
            allowance: PublicationAllowance::new(config),
            item_numerator_remainder: 0,
            byte_numerator_remainder: 0,
            diagnostics: PublicationDiagnostics {
                budget,
                ..Default::default()
            },
        }
    }

    pub(crate) fn begin_frame(&mut self, elapsed: Duration) {
        self.diagnostics.frame_sequence = self.diagnostics.frame_sequence.saturating_add(1);
        self.diagnostics.observed_frame_time = elapsed;
        self.update_pressure_state(elapsed);

        let (items, item_remainder) = accrue_tokens(
            u128::from(self.item_rate_per_second),
            elapsed,
            self.item_numerator_remainder,
            0,
            self.config.maximum_burst_items as u128,
        );
        let (bytes, byte_remainder) = accrue_tokens(
            u128::from(self.byte_rate_per_second),
            elapsed,
            self.byte_numerator_remainder,
            0,
            u128::from(self.config.maximum_burst_bytes),
        );
        let issued_items = usize::try_from(items)
            .unwrap_or(self.config.maximum_burst_items)
            .min(self.config.maximum_burst_items);
        let issued_bytes = u64::try_from(bytes)
            .unwrap_or(self.config.maximum_burst_bytes)
            .min(self.config.maximum_burst_bytes);
        self.item_numerator_remainder = item_remainder;
        self.byte_numerator_remainder = byte_remainder;
        self.allowance.begin_frame(
            self.diagnostics.frame_sequence,
            issued_items,
            issued_bytes,
            self.config.maximum_zero_byte_operations_per_frame,
        );
        self.diagnostics.budget = frame_budget(
            self.config,
            self.allowance.frame_remaining_items(),
            self.allowance.frame_remaining_bytes(),
        );
    }

    pub(crate) fn finish_frame(&mut self, work: PublicationFrameWork) {
        self.diagnostics.last_work = work;
    }

    #[must_use]
    pub(crate) const fn budget(&self) -> ChunkUploadBudget {
        self.diagnostics.budget
    }

    #[must_use]
    pub(crate) fn allowance(&self) -> PublicationAllowance {
        self.allowance.clone()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn accrued_items(&self) -> usize {
        self.allowance.remaining_items()
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn accrued_bytes(&self) -> u64 {
        self.allowance.remaining_bytes()
    }

    #[must_use]
    pub(crate) const fn diagnostics(&self) -> PublicationDiagnostics {
        self.diagnostics
    }

    fn update_pressure_state(&mut self, elapsed: Duration) {
        let work = self.diagnostics.last_work;
        let gpu_backlog = work.upload_queue_items >= RENDER_QUEUE_PRESSURE_ITEMS
            || work.upload_queue_bytes >= self.config.maximum_frame_bytes;
        let spent_frame_allowance = (self.diagnostics.budget.max_per_frame > 0
            && work.mesh_payloads_published >= self.diagnostics.budget.max_per_frame)
            || (self.diagnostics.budget.max_bytes_per_frame > 0
                && work.mesh_bytes_published >= self.diagnostics.budget.max_bytes_per_frame);
        let saturated_slow_frame = elapsed > PRESSURE_FRAME_TIME
            && elapsed <= PACED_LOW_FREQUENCY_FRAME_TIME
            && spent_frame_allowance
            && (work.pending_mesh_jobs > 0 || work.in_flight_mesh_jobs > 0);
        if gpu_backlog || saturated_slow_frame {
            if self.item_rate_per_second != self.config.minimum_items_per_second
                || self.byte_rate_per_second != self.config.minimum_bytes_per_second
            {
                self.diagnostics.multiplicative_decreases =
                    self.diagnostics.multiplicative_decreases.saturating_add(1);
            }
            self.item_rate_per_second = self.config.minimum_items_per_second;
            self.byte_rate_per_second = self.config.minimum_bytes_per_second;
            self.diagnostics.under_target_streak = 0;
            return;
        }

        let recovering = self.item_rate_per_second != self.config.target_items_per_second
            || self.byte_rate_per_second != self.config.target_bytes_per_second;
        if !recovering {
            self.diagnostics.under_target_streak = 0;
            return;
        }
        self.diagnostics.under_target_streak = self
            .diagnostics
            .under_target_streak
            .checked_add(1)
            .unwrap_or(RECOVERY_STREAK_FRAMES)
            .min(RECOVERY_STREAK_FRAMES);
        if self.diagnostics.under_target_streak < RECOVERY_STREAK_FRAMES {
            return;
        }
        self.diagnostics.under_target_streak = 0;
        self.item_rate_per_second = self.config.target_items_per_second;
        self.byte_rate_per_second = self.config.target_bytes_per_second;
        self.diagnostics.additive_increases = self.diagnostics.additive_increases.saturating_add(1);
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
        "ADAPTIVE_PUBLICATION frame={} frame_us={} cap_items={} cap_bytes={} cap_zero={} under_target_streak={} decreases={} increases={} dispatched={} published={} published_bytes={} pending={} in_flight={} upload_items={} upload_bytes={} cohort_loaded={} cohort_expected={} resident={} cave={} frustum={} submitted={} gpu_completed={}",
        diagnostics.frame_sequence,
        diagnostics.observed_frame_time.as_micros(),
        diagnostics.budget.max_per_frame,
        diagnostics.budget.max_bytes_per_frame,
        diagnostics.budget.max_zero_byte_operations_per_frame,
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

fn frame_budget(
    config: PublicationServiceConfig,
    accrued_items: usize,
    accrued_bytes: u64,
) -> ChunkUploadBudget {
    ChunkUploadBudget::new(
        accrued_items.min(config.maximum_frame_items),
        accrued_bytes.min(config.maximum_frame_bytes),
    )
    .with_zero_byte_operations_per_frame(config.maximum_zero_byte_operations_per_frame)
}

fn accrue_tokens(
    rate_per_second: u128,
    elapsed: Duration,
    numerator_remainder: u128,
    accrued: u128,
    ceiling: u128,
) -> (u128, u128) {
    let Some(numerator) = rate_per_second
        .checked_mul(elapsed.as_nanos())
        .and_then(|value| value.checked_add(numerator_remainder))
    else {
        return (ceiling, 0);
    };
    let earned = numerator / NANOS_PER_SECOND;
    let remainder = numerator % NANOS_PER_SECOND;
    let total = accrued.checked_add(earned).unwrap_or(ceiling).min(ceiling);
    (total, if total == ceiling { 0 } else { remainder })
}
