use crate::chunk::*;

/// Cross-world metrics bridge shared by the main and render worlds.
#[derive(Resource, Debug, Clone, Default)]
pub struct TransparentSortMetrics(pub(in crate::chunk) Arc<Mutex<TransparentSortMetricsSnapshot>>);

impl TransparentSortMetrics {
    #[must_use]
    pub fn snapshot(&self) -> TransparentSortMetricsSnapshot {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    pub(in crate::chunk) fn update(
        &self,
        update: impl FnOnce(&mut TransparentSortMetricsSnapshot),
    ) {
        update(&mut self.0.lock().unwrap_or_else(|poison| poison.into_inner()));
    }
}

/// Cross-world bridge for exact model workload telemetry.
#[derive(Resource, Debug, Clone, Default)]
pub struct ModelWorkloadMetrics(pub(in crate::chunk) Arc<Mutex<ModelWorkloadMetricsSnapshot>>);

impl ModelWorkloadMetrics {
    #[must_use]
    pub fn snapshot(&self) -> ModelWorkloadMetricsSnapshot {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner())
    }

    pub(in crate::chunk) fn begin_frame(&self, resident: ModelWorkloadCount) {
        *self.0.lock().unwrap_or_else(|poison| poison.into_inner()) =
            ModelWorkloadMetricsSnapshot {
                resident,
                visible: ModelWorkloadCount::default(),
            };
    }

    pub(in crate::chunk) fn record_visible(&self, visible: ModelWorkloadCount) {
        let mut snapshot = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        snapshot.visible.model_ref_count = snapshot
            .visible
            .model_ref_count
            .max(visible.model_ref_count);
        snapshot.visible.model_draw_ref_count = snapshot
            .visible
            .model_draw_ref_count
            .max(visible.model_draw_ref_count);
        snapshot.visible.legacy_fixed_slot_quad_invocations_avoided = snapshot
            .visible
            .legacy_fixed_slot_quad_invocations_avoided
            .max(visible.legacy_fixed_slot_quad_invocations_avoided);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_metrics_clones_share_the_latest_snapshot() {
        let metrics = TransparentSortMetrics::default();
        let render_world = metrics.clone();
        let snapshot = TransparentSortMetricsSnapshot {
            request_generation: 7,
            result_generation: 6,
            committed_generation: 5,
            encoded_generation: 5,
            presented_generation: 5,
            ref_count: 4,
            cpu_duration: std::time::Duration::from_micros(30),
            request_to_commit_latency: std::time::Duration::from_micros(50),
            staged_bytes: 24,
            upload_bytes: 16,
            stale_reject_count: 3,
            ceiling_reject_count: 2,
            active_slot_age_frames: 9,
            transparent_water_distinct_tint_count: 2,
        };
        render_world.update(|current| *current = snapshot);
        assert_eq!(metrics.snapshot(), snapshot);
    }
}
