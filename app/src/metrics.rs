//! Metrics shared with the optional acceptance plugin.
pub use diagnostics::metrics::{
    AssetMetrics, DIAGNOSTIC_TOP_LIMIT, DiagnosticAttributionEntry, DiagnosticAttributionSnapshot,
    DiagnosticQuadTracker, ExactFullViewProof, GpuPassMeasurement, GpuPassSample, LowFpsBounds,
    MetricsCollector, MetricsReport, ModelWorkloadCountSnapshot, ModelWorkloadMetricsSnapshot,
    PipelineMetricsSnapshot, TeleportProof, TransparentSortMetricsSnapshot,
    deterministic_manifest_hash, pair_gpu_pass_sample,
};
