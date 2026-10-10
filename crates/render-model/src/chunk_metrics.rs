//! Chunk render telemetry snapshots published by the render world.

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransparentSortMetricsSnapshot {
    pub request_generation: u64,
    pub result_generation: u64,
    pub committed_generation: u64,
    /// Generation whose draw command was encoded into a render pass.
    pub encoded_generation: u64,
    /// Generation proven by the frame's GPU-completion callback.
    pub presented_generation: u64,
    pub ref_count: usize,
    pub cpu_duration: std::time::Duration,
    pub request_to_commit_latency: std::time::Duration,
    pub staged_bytes: u64,
    /// Cumulative transparent ref bytes successfully written to the GPU.
    pub upload_bytes: u64,
    pub stale_reject_count: u64,
    pub ceiling_reject_count: u64,
    pub active_slot_age_frames: u64,
    pub transparent_water_distinct_tint_count: usize,
}

/// Exact model workload for one allocation cohort.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelWorkloadCount {
    pub model_ref_count: usize,
    pub model_draw_ref_count: usize,
    /// Quad vertex-shader invocations avoided relative to the former fixed
    /// 32-quad slot issued for every model ref.
    pub legacy_fixed_slot_quad_invocations_avoided: usize,
}

/// Current resident and frustum-visible model workload published by the
/// render world for acceptance telemetry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelWorkloadMetricsSnapshot {
    pub resident: ModelWorkloadCount,
    pub visible: ModelWorkloadCount,
}
