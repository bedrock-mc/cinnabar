use std::{fs, io, path::Path};

use serde::Serialize;

use super::{AssetMetrics, ExactFullViewProof, TeleportProof};

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricsReport {
    pub session_seconds: f64,
    pub world_ready: bool,
    pub requested_radius_chunks: i32,
    pub received_radius_chunks: Option<i32>,
    pub publisher_radius_chunks: Option<i32>,
    pub mutation_coordinate: Option<[i32; 3]>,
    pub visible_mutation_count: u64,
    pub frame_count: usize,
    pub p50_frame_ms: f64,
    pub p95_frame_ms: f64,
    pub p99_frame_ms: f64,
    pub max_frame_ms: f64,
    pub gpu_pass_sample_count: u64,
    pub opaque_3d_gpu_p50_ms: f64,
    pub opaque_3d_gpu_p95_ms: f64,
    pub opaque_3d_gpu_p99_ms: f64,
    pub opaque_3d_gpu_max_ms: f64,
    pub transparent_3d_gpu_p50_ms: f64,
    pub transparent_3d_gpu_p95_ms: f64,
    pub transparent_3d_gpu_p99_ms: f64,
    pub transparent_3d_gpu_max_ms: f64,
    pub chunk_containing_pass_gpu_p50_ms: f64,
    pub chunk_containing_pass_gpu_p95_ms: f64,
    pub chunk_containing_pass_gpu_p99_ms: f64,
    pub chunk_containing_pass_gpu_max_ms: f64,
    pub max_decode_ms: f64,
    pub max_mesh_ms: f64,
    pub max_remesh_ms: f64,
    pub teleport_settle_ms: Option<f64>,
    pub forced_full_view_remesh_ms: Option<f64>,
    pub teleport_proof: Option<TeleportProof>,
    pub forced_full_view_remesh_proof: Option<ExactFullViewProof>,
    pub max_mutation_to_visible_ms: f64,
    pub decode_error_count: u64,
    pub rendered_sub_chunks: usize,
    pub resident_sub_chunks: usize,
    pub visible_sub_chunks: usize,
    pub peak_admitted_world_events: usize,
    pub peak_admitted_heavy_events: usize,
    pub peak_queued_decode_jobs: usize,
    pub peak_in_flight_decode_jobs: usize,
    pub peak_completed_decode_results: usize,
    pub peak_pending_retry_requests: usize,
    pub peak_outbound_requests: usize,
    pub peak_pending_mesh_jobs: usize,
    pub peak_in_flight_mesh_jobs: usize,
    pub nontransparent_gpu_upload_bytes: u64,
    pub gpu_upload_bytes: u64,
    pub transparent_sort_request_generation: u64,
    pub transparent_sort_result_generation: u64,
    pub transparent_sort_committed_generation: u64,
    pub transparent_sort_encoded_generation: u64,
    pub transparent_sort_presented_generation: u64,
    pub transparent_sort_ref_count: usize,
    pub transparent_sort_cpu_ms: f64,
    pub transparent_sort_request_to_commit_ms: f64,
    pub transparent_sort_staged_bytes: u64,
    pub transparent_sort_upload_bytes: u64,
    pub transparent_sort_stale_reject_count: u64,
    pub transparent_sort_ceiling_reject_count: u64,
    pub transparent_sort_active_slot_age_frames: u64,
    pub transparent_water_distinct_tint_count: usize,
    pub resident_model_ref_count: usize,
    pub resident_model_draw_ref_count: usize,
    pub resident_legacy_fixed_slot_quad_invocations_avoided: usize,
    pub visible_model_ref_count: usize,
    pub visible_model_draw_ref_count: usize,
    pub visible_legacy_fixed_slot_quad_invocations_avoided: usize,
    pub assets: AssetMetrics,
}

impl MetricsReport {
    pub fn write_json(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let mut encoded = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        encoded.push(b'\n');
        fs::write(path, encoded)
    }
}
