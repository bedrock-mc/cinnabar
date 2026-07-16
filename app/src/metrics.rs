use std::{
    collections::BTreeMap,
    fs, io,
    path::Path,
    time::{Duration, Instant},
};

use serde::Serialize;
use world::SubChunkKey;

const FRAME_HISTOGRAM_RESOLUTION_MS: f64 = 0.1;
const FRAME_HISTOGRAM_BUCKETS: usize = 20_001;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ExactFullViewProof {
    pub target: String,
    pub committed: String,
    pub ms: f64,
    pub view_generation: u64,
    pub transparent_sort_generation: u64,
    pub render_ready_ms: f64,
    pub first_frame_sequence: u64,
    pub stable_frame_sequence: u64,
    pub first_present_ms: f64,
    pub first_gpu_ms: f64,
    pub stable_present_ms: f64,
    pub stable_gpu_ms: f64,
    pub frame_count: u64,
    pub expected_manifest_count: usize,
    pub expected_manifest_hash: String,
    pub first_presented_manifest_count: usize,
    pub first_presented_manifest_hash: String,
    pub stable_presented_manifest_count: usize,
    pub stable_presented_manifest_hash: String,
    pub expected: usize,
    pub loaded_target: usize,
    pub missing_target: usize,
    pub foreign_loaded: usize,
    pub foreign_requested: usize,
    pub foreign_resident: usize,
    pub source_leftover: usize,
    pub resident_count: usize,
    pub resident_hash: String,
    pub known_air_count: usize,
    pub known_air_hash: String,
    pub missing_target_instances: usize,
    pub unexpected_target_instances: usize,
    pub source_instances: usize,
    pub foreign_instances: usize,
    pub stale_generation_instances: usize,
    pub orphan_allocations: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TeleportProof {
    #[serde(flatten)]
    pub exact: ExactFullViewProof,
    pub publisher_ms: Option<f64>,
    pub first_level_ms: Option<f64>,
    pub last_level_ms: Option<f64>,
    pub level_events: u64,
    pub first_sub_ms: Option<f64>,
    pub last_sub_ms: Option<f64>,
    pub sub_events: u64,
}

#[must_use]
pub fn deterministic_manifest_hash(manifest: &[(SubChunkKey, u64)]) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    let mut entries = manifest.to_vec();
    entries.sort_unstable();
    let mut hash = FNV_OFFSET_BASIS;
    for (key, generation) in entries {
        for byte in [key.dimension, key.x, key.y, key.z]
            .into_iter()
            .flat_map(i32::to_le_bytes)
            .chain(generation.to_le_bytes())
        {
            hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
        }
    }
    hash
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetMetrics {
    pub source_tag: String,
    pub source_sha256: String,
    pub blob_sha256: String,
    pub texture_layers: u32,
    pub texture_pages: u32,
    pub texture_bytes_including_mips: u64,
    pub material_count: u32,
    pub model_template_count: u32,
    pub model_quad_count: u32,
    pub animation_count: u32,
    pub animation_frame_count: u32,
    pub missing_mapping_count: u64,
    pub diagnostic_quad_count: u64,
}

impl Default for AssetMetrics {
    fn default() -> Self {
        Self {
            source_tag: "diagnostic".to_owned(),
            source_sha256: "diagnostic".to_owned(),
            blob_sha256: "diagnostic".to_owned(),
            texture_layers: 1,
            texture_pages: 1,
            texture_bytes_including_mips: 1_364,
            material_count: 1,
            model_template_count: 0,
            model_quad_count: 0,
            animation_count: 0,
            animation_frame_count: 0,
            missing_mapping_count: 0,
            diagnostic_quad_count: 0,
        }
    }
}

impl AssetMetrics {
    #[must_use]
    pub fn world_ready_marker(
        &self,
        resident_sub_chunks: usize,
        visible_sub_chunks: usize,
    ) -> String {
        format!(
            "WORLD_READY source_tag={} source_sha256={} blob_sha256={} texture_layers={} texture_pages={} \
             texture_bytes_including_mips={} material_count={} model_template_count={} model_quad_count={} \
             animation_count={} animation_frame_count={} missing_mapping_count={} \
             diagnostic_quad_count={} resident_sub_chunks={resident_sub_chunks} \
             visible_sub_chunks={visible_sub_chunks}",
            self.source_tag,
            self.source_sha256,
            self.blob_sha256,
            self.texture_layers,
            self.texture_pages,
            self.texture_bytes_including_mips,
            self.material_count,
            self.model_template_count,
            self.model_quad_count,
            self.animation_count,
            self.animation_frame_count,
            self.missing_mapping_count,
            self.diagnostic_quad_count,
        )
    }
}

#[derive(Debug, Default)]
pub struct DiagnosticQuadTracker {
    by_sub_chunk: BTreeMap<SubChunkKey, u64>,
    total: u64,
}

impl DiagnosticQuadTracker {
    pub fn upsert(&mut self, key: SubChunkKey, count: u64) {
        if count == 0 {
            self.remove(key);
            return;
        }
        let previous = self.by_sub_chunk.insert(key, count).unwrap_or(0);
        self.total = self.total.saturating_sub(previous).saturating_add(count);
    }

    pub fn remove(&mut self, key: SubChunkKey) {
        if let Some(previous) = self.by_sub_chunk.remove(&key) {
            self.total = self.total.saturating_sub(previous);
        }
    }

    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }
}

#[derive(Debug)]
struct FrameHistogram {
    counts: Box<[u64]>,
    sample_count: u64,
    max_milliseconds: f64,
}

impl Default for FrameHistogram {
    fn default() -> Self {
        Self {
            counts: vec![0; FRAME_HISTOGRAM_BUCKETS].into_boxed_slice(),
            sample_count: 0,
            max_milliseconds: 0.0,
        }
    }
}

impl FrameHistogram {
    fn record(&mut self, milliseconds: f64) {
        let milliseconds = if milliseconds.is_finite() {
            milliseconds.max(0.0)
        } else {
            (FRAME_HISTOGRAM_BUCKETS - 1) as f64 * FRAME_HISTOGRAM_RESOLUTION_MS
        };
        let bucket = (milliseconds / FRAME_HISTOGRAM_RESOLUTION_MS)
            .ceil()
            .clamp(0.0, (FRAME_HISTOGRAM_BUCKETS - 1) as f64) as usize;
        self.counts[bucket] = self.counts[bucket].saturating_add(1);
        self.sample_count = self.sample_count.saturating_add(1);
        self.max_milliseconds = self.max_milliseconds.max(milliseconds);
    }

    fn quantile(&self, percentile: f64) -> f64 {
        if self.sample_count == 0 {
            return 0.0;
        }
        let target = ((self.sample_count - 1) as f64 * percentile).ceil() as u64;
        let mut cumulative = 0_u64;
        for (index, count) in self.counts.iter().copied().enumerate() {
            cumulative = cumulative.saturating_add(count);
            if cumulative > target {
                return index as f64 * FRAME_HISTOGRAM_RESOLUTION_MS;
            }
        }
        (FRAME_HISTOGRAM_BUCKETS - 1) as f64 * FRAME_HISTOGRAM_RESOLUTION_MS
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GpuPassMeasurement {
    pub time: Instant,
    pub value_ms: f64,
}

impl GpuPassMeasurement {
    #[must_use]
    pub const fn new(time: Instant, value_ms: f64) -> Self {
        Self { time, value_ms }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct GpuPassSample {
    pub opaque_ms: f64,
    pub transparent_ms: f64,
    pub chunk_containing_pass_ms: f64,
}

#[must_use]
pub fn pair_gpu_pass_sample(
    last_recorded_time: Option<Instant>,
    opaque: Option<GpuPassMeasurement>,
    transparent: Option<GpuPassMeasurement>,
) -> Option<(Instant, GpuPassSample)> {
    let opaque = opaque
        .filter(|measurement| measurement.value_ms.is_finite() && measurement.value_ms >= 0.0)?;
    if last_recorded_time == Some(opaque.time) {
        return None;
    }
    let transparent_ms = transparent
        .filter(|measurement| {
            measurement.time == opaque.time
                && measurement.value_ms.is_finite()
                && measurement.value_ms >= 0.0
        })
        .map_or(0.0, |measurement| measurement.value_ms);
    Some((
        opaque.time,
        GpuPassSample {
            opaque_ms: opaque.value_ms,
            transparent_ms,
            chunk_containing_pass_ms: opaque.value_ms + transparent_ms,
        },
    ))
}

#[derive(Debug)]
pub struct MetricsCollector {
    started: Instant,
    finished: Option<Instant>,
    assets: AssetMetrics,
    frame_histogram: FrameHistogram,
    opaque_gpu_histogram: FrameHistogram,
    transparent_gpu_histogram: FrameHistogram,
    chunk_containing_gpu_histogram: FrameHistogram,
    world_ready: bool,
    requested_radius_chunks: i32,
    received_radius_chunks: Option<i32>,
    publisher_radius_chunks: Option<i32>,
    mutation_coordinate: Option<[i32; 3]>,
    visible_mutation_count: u64,
    max_remesh_milliseconds: f64,
    teleport_settle_milliseconds: Option<f64>,
    forced_full_view_remesh_milliseconds: Option<f64>,
    teleport_proof: Option<TeleportProof>,
    forced_full_view_remesh_proof: Option<ExactFullViewProof>,
    max_mutation_to_visible_milliseconds: f64,
    max_decode_milliseconds: f64,
    max_mesh_milliseconds: f64,
    decode_errors: u64,
    rendered_sub_chunks: usize,
    resident_sub_chunks: usize,
    visible_sub_chunks: usize,
    peak_admitted_world_events: usize,
    peak_admitted_heavy_events: usize,
    peak_queued_decode_jobs: usize,
    peak_in_flight_decode_jobs: usize,
    peak_completed_decode_results: usize,
    peak_pending_retry_requests: usize,
    peak_outbound_requests: usize,
    peak_pending_mesh_jobs: usize,
    peak_in_flight_mesh_jobs: usize,
    gpu_upload_bytes: u64,
    transparent_sort: TransparentSortMetricsSnapshot,
    model_workload: ModelWorkloadMetricsSnapshot,
}

/// App-owned, copyable seam for render-world transparent-sort evidence.
///
/// The render crate may publish these counters through a Bevy resource; keeping
/// this snapshot local prevents the stable metrics schema from depending on the
/// renderer's internal resource layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TransparentSortMetricsSnapshot {
    pub request_generation: u64,
    pub result_generation: u64,
    pub committed_generation: u64,
    pub encoded_generation: u64,
    pub presented_generation: u64,
    pub ref_count: usize,
    pub cpu_duration: Duration,
    pub request_to_commit_latency: Duration,
    pub staged_bytes: u64,
    pub upload_bytes: u64,
    pub stale_reject_count: u64,
    pub ceiling_reject_count: u64,
    pub active_slot_age_frames: u64,
    pub transparent_water_distinct_tint_count: usize,
}

impl From<render::TransparentSortMetricsSnapshot> for TransparentSortMetricsSnapshot {
    fn from(snapshot: render::TransparentSortMetricsSnapshot) -> Self {
        Self {
            request_generation: snapshot.request_generation,
            result_generation: snapshot.result_generation,
            committed_generation: snapshot.committed_generation,
            encoded_generation: snapshot.encoded_generation,
            presented_generation: snapshot.presented_generation,
            ref_count: snapshot.ref_count,
            cpu_duration: snapshot.cpu_duration,
            request_to_commit_latency: snapshot.request_to_commit_latency,
            staged_bytes: snapshot.staged_bytes,
            upload_bytes: snapshot.upload_bytes,
            stale_reject_count: snapshot.stale_reject_count,
            ceiling_reject_count: snapshot.ceiling_reject_count,
            active_slot_age_frames: snapshot.active_slot_age_frames,
            transparent_water_distinct_tint_count: snapshot.transparent_water_distinct_tint_count,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelWorkloadCountSnapshot {
    pub model_ref_count: usize,
    pub model_draw_ref_count: usize,
    pub legacy_fixed_slot_quad_invocations_avoided: usize,
}

impl From<render::ModelWorkloadCount> for ModelWorkloadCountSnapshot {
    fn from(snapshot: render::ModelWorkloadCount) -> Self {
        Self {
            model_ref_count: snapshot.model_ref_count,
            model_draw_ref_count: snapshot.model_draw_ref_count,
            legacy_fixed_slot_quad_invocations_avoided: snapshot
                .legacy_fixed_slot_quad_invocations_avoided,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModelWorkloadMetricsSnapshot {
    pub resident: ModelWorkloadCountSnapshot,
    pub visible: ModelWorkloadCountSnapshot,
}

impl From<render::ModelWorkloadMetricsSnapshot> for ModelWorkloadMetricsSnapshot {
    fn from(snapshot: render::ModelWorkloadMetricsSnapshot) -> Self {
        Self {
            resident: snapshot.resident.into(),
            visible: snapshot.visible.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PipelineMetricsSnapshot {
    pub world_ready: bool,
    pub requested_radius_chunks: i32,
    pub received_radius_chunks: Option<i32>,
    pub publisher_radius_chunks: Option<i32>,
    pub mutation_coordinate: Option<[i32; 3]>,
    pub visible_mutation_count: u64,
    pub max_decode: Duration,
    pub max_mesh: Duration,
    pub max_remesh: Duration,
    pub rendered_sub_chunks: usize,
    pub resident_sub_chunks: usize,
    pub visible_sub_chunks: usize,
    pub admitted_world_events: usize,
    pub admitted_heavy_events: usize,
    pub queued_decode_jobs: usize,
    pub in_flight_decode_jobs: usize,
    pub completed_decode_results: usize,
    pub pending_retry_requests: usize,
    pub outbound_requests: usize,
    pub pending_mesh_jobs: usize,
    pub in_flight_mesh_jobs: usize,
    pub gpu_upload_bytes: u64,
    pub transparent_sort: TransparentSortMetricsSnapshot,
    pub model_workload: ModelWorkloadMetricsSnapshot,
}

impl Default for MetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsCollector {
    #[must_use]
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            finished: None,
            assets: AssetMetrics::default(),
            frame_histogram: FrameHistogram::default(),
            opaque_gpu_histogram: FrameHistogram::default(),
            transparent_gpu_histogram: FrameHistogram::default(),
            chunk_containing_gpu_histogram: FrameHistogram::default(),
            world_ready: false,
            requested_radius_chunks: 0,
            received_radius_chunks: None,
            publisher_radius_chunks: None,
            mutation_coordinate: None,
            visible_mutation_count: 0,
            max_remesh_milliseconds: 0.0,
            teleport_settle_milliseconds: None,
            forced_full_view_remesh_milliseconds: None,
            teleport_proof: None,
            forced_full_view_remesh_proof: None,
            max_mutation_to_visible_milliseconds: 0.0,
            max_decode_milliseconds: 0.0,
            max_mesh_milliseconds: 0.0,
            decode_errors: 0,
            rendered_sub_chunks: 0,
            resident_sub_chunks: 0,
            visible_sub_chunks: 0,
            peak_admitted_world_events: 0,
            peak_admitted_heavy_events: 0,
            peak_queued_decode_jobs: 0,
            peak_in_flight_decode_jobs: 0,
            peak_completed_decode_results: 0,
            peak_pending_retry_requests: 0,
            peak_outbound_requests: 0,
            peak_pending_mesh_jobs: 0,
            peak_in_flight_mesh_jobs: 0,
            gpu_upload_bytes: 0,
            transparent_sort: TransparentSortMetricsSnapshot::default(),
            model_workload: ModelWorkloadMetricsSnapshot::default(),
        }
    }

    #[must_use]
    pub fn with_asset_metrics(assets: AssetMetrics) -> Self {
        Self {
            assets,
            ..Self::new()
        }
    }

    pub fn record_asset_counters(
        &mut self,
        missing_mapping_count: u64,
        diagnostic_quad_count: u64,
    ) {
        self.assets.missing_mapping_count = missing_mapping_count;
        self.assets.diagnostic_quad_count = diagnostic_quad_count;
    }

    #[must_use]
    pub const fn asset_metrics(&self) -> &AssetMetrics {
        &self.assets
    }

    pub fn record_frame(&mut self, duration: Duration) {
        if self.finished.is_some() {
            return;
        }
        self.frame_histogram
            .record(duration.as_secs_f64() * 1_000.0);
    }

    pub fn record_gpu_pass_sample(&mut self, time: Instant, sample: GpuPassSample) {
        if time < self.started || self.finished.is_some() {
            return;
        }
        self.opaque_gpu_histogram.record(sample.opaque_ms);
        self.transparent_gpu_histogram.record(sample.transparent_ms);
        self.chunk_containing_gpu_histogram
            .record(sample.chunk_containing_pass_ms);
    }

    #[must_use]
    pub const fn frame_count(&self) -> u64 {
        self.frame_histogram.sample_count
    }

    pub fn begin_timed_session(&mut self, started: Instant) {
        self.started = started;
        self.finished = None;
        self.frame_histogram = FrameHistogram::default();
        self.opaque_gpu_histogram = FrameHistogram::default();
        self.transparent_gpu_histogram = FrameHistogram::default();
        self.chunk_containing_gpu_histogram = FrameHistogram::default();
        self.max_remesh_milliseconds = 0.0;
        self.max_mutation_to_visible_milliseconds = 0.0;
        self.max_decode_milliseconds = 0.0;
        self.max_mesh_milliseconds = 0.0;
        self.teleport_settle_milliseconds = None;
        self.forced_full_view_remesh_milliseconds = None;
        self.teleport_proof = None;
        self.forced_full_view_remesh_proof = None;
    }

    /// Freezes duration-bound performance evidence at the requested session
    /// deadline. Presentation/upload counters may still advance during the
    /// bounded post-session GPU-settle grace.
    pub fn finish_timed_session(&mut self, finished: Instant) {
        if self.finished.is_none() {
            self.finished = Some(finished.max(self.started));
        }
    }

    pub fn record_remesh_latency(&mut self, duration: Duration) {
        if self.finished.is_some() {
            return;
        }
        self.max_remesh_milliseconds = self
            .max_remesh_milliseconds
            .max(duration.as_secs_f64() * 1_000.0);
    }

    pub fn record_mutation_to_visible(&mut self, duration: Duration) {
        if self.finished.is_some() {
            return;
        }
        self.max_mutation_to_visible_milliseconds = self
            .max_mutation_to_visible_milliseconds
            .max(duration.as_secs_f64() * 1_000.0);
    }

    #[cfg(test)]
    pub fn record_teleport_settle(&mut self, duration: Duration) {
        self.teleport_settle_milliseconds = Some(duration.as_secs_f64() * 1_000.0);
    }

    #[cfg(test)]
    pub fn record_forced_full_view_remesh(&mut self, duration: Duration) {
        self.forced_full_view_remesh_milliseconds = Some(duration.as_secs_f64() * 1_000.0);
    }

    pub fn record_teleport_proof(&mut self, proof: TeleportProof) {
        self.teleport_settle_milliseconds = Some(proof.exact.ms);
        self.teleport_proof = Some(proof);
    }

    pub fn record_forced_full_view_remesh_proof(&mut self, proof: ExactFullViewProof) {
        self.forced_full_view_remesh_milliseconds = Some(proof.ms);
        self.forced_full_view_remesh_proof = Some(proof);
    }

    pub fn add_decode_errors(&mut self, count: u64) {
        self.decode_errors = self.decode_errors.saturating_add(count);
    }

    pub fn record_pipeline_snapshot(&mut self, snapshot: PipelineMetricsSnapshot) {
        if self.finished.is_some() {
            self.gpu_upload_bytes = self.gpu_upload_bytes.max(snapshot.gpu_upload_bytes);
            self.record_transparent_sort_snapshot(snapshot.transparent_sort);
            self.model_workload = snapshot.model_workload;
            return;
        }
        self.world_ready |= snapshot.world_ready;
        self.requested_radius_chunks = snapshot.requested_radius_chunks;
        self.received_radius_chunks = snapshot.received_radius_chunks;
        self.publisher_radius_chunks = snapshot.publisher_radius_chunks;
        self.mutation_coordinate = snapshot.mutation_coordinate;
        self.visible_mutation_count = snapshot.visible_mutation_count;
        self.max_decode_milliseconds = self
            .max_decode_milliseconds
            .max(snapshot.max_decode.as_secs_f64() * 1_000.0);
        self.max_mesh_milliseconds = self
            .max_mesh_milliseconds
            .max(snapshot.max_mesh.as_secs_f64() * 1_000.0);
        self.record_remesh_latency(snapshot.max_remesh);
        self.rendered_sub_chunks = snapshot.rendered_sub_chunks;
        self.resident_sub_chunks = snapshot.resident_sub_chunks;
        self.visible_sub_chunks = snapshot.visible_sub_chunks;
        self.peak_admitted_world_events = self
            .peak_admitted_world_events
            .max(snapshot.admitted_world_events);
        self.peak_admitted_heavy_events = self
            .peak_admitted_heavy_events
            .max(snapshot.admitted_heavy_events);
        self.peak_queued_decode_jobs = self
            .peak_queued_decode_jobs
            .max(snapshot.queued_decode_jobs);
        self.peak_in_flight_decode_jobs = self
            .peak_in_flight_decode_jobs
            .max(snapshot.in_flight_decode_jobs);
        self.peak_completed_decode_results = self
            .peak_completed_decode_results
            .max(snapshot.completed_decode_results);
        self.peak_pending_retry_requests = self
            .peak_pending_retry_requests
            .max(snapshot.pending_retry_requests);
        self.peak_outbound_requests = self.peak_outbound_requests.max(snapshot.outbound_requests);
        self.peak_pending_mesh_jobs = self.peak_pending_mesh_jobs.max(snapshot.pending_mesh_jobs);
        self.peak_in_flight_mesh_jobs = self
            .peak_in_flight_mesh_jobs
            .max(snapshot.in_flight_mesh_jobs);
        self.gpu_upload_bytes = self.gpu_upload_bytes.max(snapshot.gpu_upload_bytes);
        self.record_transparent_sort_snapshot(snapshot.transparent_sort);
        self.model_workload = snapshot.model_workload;
    }

    pub fn record_transparent_sort_snapshot(&mut self, snapshot: TransparentSortMetricsSnapshot) {
        self.transparent_sort = snapshot;
    }

    #[must_use]
    pub fn report(&self) -> MetricsReport {
        let finished = self.finished.unwrap_or_else(Instant::now);
        MetricsReport {
            session_seconds: finished
                .saturating_duration_since(self.started)
                .as_secs_f64(),
            world_ready: self.world_ready,
            requested_radius_chunks: self.requested_radius_chunks,
            received_radius_chunks: self.received_radius_chunks,
            publisher_radius_chunks: self.publisher_radius_chunks,
            mutation_coordinate: self.mutation_coordinate,
            visible_mutation_count: self.visible_mutation_count,
            frame_count: usize::try_from(self.frame_histogram.sample_count).unwrap_or(usize::MAX),
            p50_frame_ms: self.frame_histogram.quantile(0.50),
            p95_frame_ms: self.frame_histogram.quantile(0.95),
            p99_frame_ms: self.frame_histogram.quantile(0.99),
            max_frame_ms: self.frame_histogram.max_milliseconds,
            gpu_pass_sample_count: self.opaque_gpu_histogram.sample_count,
            opaque_3d_gpu_p50_ms: self.opaque_gpu_histogram.quantile(0.50),
            opaque_3d_gpu_p95_ms: self.opaque_gpu_histogram.quantile(0.95),
            opaque_3d_gpu_p99_ms: self.opaque_gpu_histogram.quantile(0.99),
            opaque_3d_gpu_max_ms: self.opaque_gpu_histogram.max_milliseconds,
            transparent_3d_gpu_p50_ms: self.transparent_gpu_histogram.quantile(0.50),
            transparent_3d_gpu_p95_ms: self.transparent_gpu_histogram.quantile(0.95),
            transparent_3d_gpu_p99_ms: self.transparent_gpu_histogram.quantile(0.99),
            transparent_3d_gpu_max_ms: self.transparent_gpu_histogram.max_milliseconds,
            chunk_containing_pass_gpu_p50_ms: self.chunk_containing_gpu_histogram.quantile(0.50),
            chunk_containing_pass_gpu_p95_ms: self.chunk_containing_gpu_histogram.quantile(0.95),
            chunk_containing_pass_gpu_p99_ms: self.chunk_containing_gpu_histogram.quantile(0.99),
            chunk_containing_pass_gpu_max_ms: self.chunk_containing_gpu_histogram.max_milliseconds,
            max_decode_ms: self.max_decode_milliseconds,
            max_mesh_ms: self.max_mesh_milliseconds,
            max_remesh_ms: self.max_remesh_milliseconds,
            teleport_settle_ms: self.teleport_settle_milliseconds,
            forced_full_view_remesh_ms: self.forced_full_view_remesh_milliseconds,
            teleport_proof: self.teleport_proof.clone(),
            forced_full_view_remesh_proof: self.forced_full_view_remesh_proof.clone(),
            max_mutation_to_visible_ms: self.max_mutation_to_visible_milliseconds,
            decode_error_count: self.decode_errors,
            rendered_sub_chunks: self.rendered_sub_chunks,
            resident_sub_chunks: self.resident_sub_chunks,
            visible_sub_chunks: self.visible_sub_chunks,
            peak_admitted_world_events: self.peak_admitted_world_events,
            peak_admitted_heavy_events: self.peak_admitted_heavy_events,
            peak_queued_decode_jobs: self.peak_queued_decode_jobs,
            peak_in_flight_decode_jobs: self.peak_in_flight_decode_jobs,
            peak_completed_decode_results: self.peak_completed_decode_results,
            peak_pending_retry_requests: self.peak_pending_retry_requests,
            peak_outbound_requests: self.peak_outbound_requests,
            peak_pending_mesh_jobs: self.peak_pending_mesh_jobs,
            peak_in_flight_mesh_jobs: self.peak_in_flight_mesh_jobs,
            nontransparent_gpu_upload_bytes: self.gpu_upload_bytes,
            gpu_upload_bytes: self
                .gpu_upload_bytes
                .checked_add(self.transparent_sort.upload_bytes)
                .expect("combined GPU upload byte counter overflowed"),
            transparent_sort_request_generation: self.transparent_sort.request_generation,
            transparent_sort_result_generation: self.transparent_sort.result_generation,
            transparent_sort_committed_generation: self.transparent_sort.committed_generation,
            transparent_sort_encoded_generation: self.transparent_sort.encoded_generation,
            transparent_sort_presented_generation: self.transparent_sort.presented_generation,
            transparent_sort_ref_count: self.transparent_sort.ref_count,
            transparent_sort_cpu_ms: self.transparent_sort.cpu_duration.as_secs_f64() * 1_000.0,
            transparent_sort_request_to_commit_ms: self
                .transparent_sort
                .request_to_commit_latency
                .as_secs_f64()
                * 1_000.0,
            transparent_sort_staged_bytes: self.transparent_sort.staged_bytes,
            transparent_sort_upload_bytes: self.transparent_sort.upload_bytes,
            transparent_sort_stale_reject_count: self.transparent_sort.stale_reject_count,
            transparent_sort_ceiling_reject_count: self.transparent_sort.ceiling_reject_count,
            transparent_sort_active_slot_age_frames: self.transparent_sort.active_slot_age_frames,
            transparent_water_distinct_tint_count: self
                .transparent_sort
                .transparent_water_distinct_tint_count,
            resident_model_ref_count: self.model_workload.resident.model_ref_count,
            resident_model_draw_ref_count: self.model_workload.resident.model_draw_ref_count,
            resident_legacy_fixed_slot_quad_invocations_avoided: self
                .model_workload
                .resident
                .legacy_fixed_slot_quad_invocations_avoided,
            visible_model_ref_count: self.model_workload.visible.model_ref_count,
            visible_model_draw_ref_count: self.model_workload.visible.model_draw_ref_count,
            visible_legacy_fixed_slot_quad_invocations_avoided: self
                .model_workload
                .visible
                .legacy_fixed_slot_quad_invocations_avoided,
            assets: self.assets.clone(),
        }
    }

    #[cfg(test)]
    fn frame_sample_capacity(&self) -> usize {
        self.frame_histogram.counts.len()
    }
}

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

#[cfg(test)]
fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::{
        AssetMetrics, ExactFullViewProof, GpuPassMeasurement, GpuPassSample, MetricsCollector,
        MetricsReport, ModelWorkloadCountSnapshot, ModelWorkloadMetricsSnapshot,
        PipelineMetricsSnapshot, TeleportProof, TransparentSortMetricsSnapshot,
        deterministic_manifest_hash, pair_gpu_pass_sample, percentile,
    };
    use std::{fs, time::Duration};
    use world::SubChunkKey;

    #[test]
    fn empty_percentiles_are_zero() {
        assert_eq!(percentile(&[], 0.99), 0.0);
    }

    #[test]
    fn gpu_pass_pair_sums_only_measurements_from_the_same_render_frame() {
        let time = std::time::Instant::now();
        let (_, sample) = pair_gpu_pass_sample(
            None,
            Some(GpuPassMeasurement::new(time, 3.25)),
            Some(GpuPassMeasurement::new(time, 0.75)),
        )
        .unwrap();
        assert_eq!(sample.opaque_ms, 3.25);
        assert_eq!(sample.transparent_ms, 0.75);
        assert_eq!(sample.chunk_containing_pass_ms, 4.0);
    }

    #[test]
    fn gpu_pass_pair_does_not_reuse_a_stale_transparent_measurement() {
        let transparent_time = std::time::Instant::now();
        let opaque_time = transparent_time + Duration::from_millis(1);
        let (_, sample) = pair_gpu_pass_sample(
            None,
            Some(GpuPassMeasurement::new(opaque_time, 2.5)),
            Some(GpuPassMeasurement::new(transparent_time, 9.0)),
        )
        .unwrap();
        assert_eq!(sample.transparent_ms, 0.0);
        assert_eq!(sample.chunk_containing_pass_ms, 2.5);
    }

    #[test]
    fn gpu_pass_pair_records_each_async_measurement_timestamp_once() {
        let time = std::time::Instant::now();
        assert!(
            pair_gpu_pass_sample(Some(time), Some(GpuPassMeasurement::new(time, 1.0)), None,)
                .is_none()
        );
    }

    #[test]
    fn gpu_pass_histograms_reset_with_the_timed_session_and_report_quantiles() {
        let mut metrics = MetricsCollector::new();
        let started = std::time::Instant::now();
        metrics.record_gpu_pass_sample(
            started,
            GpuPassSample {
                opaque_ms: 99.0,
                transparent_ms: 99.0,
                chunk_containing_pass_ms: 198.0,
            },
        );
        metrics.begin_timed_session(started);
        metrics.record_gpu_pass_sample(
            started + Duration::from_millis(1),
            GpuPassSample {
                opaque_ms: 1.25,
                transparent_ms: 0.75,
                chunk_containing_pass_ms: 2.0,
            },
        );

        let report = metrics.report();
        assert_eq!(report.gpu_pass_sample_count, 1);
        assert_eq!(report.opaque_3d_gpu_p50_ms, 1.3);
        assert_eq!(report.opaque_3d_gpu_max_ms, 1.25);
        assert_eq!(report.transparent_3d_gpu_p50_ms, 0.8);
        assert_eq!(report.transparent_3d_gpu_max_ms, 0.75);
        assert_eq!(report.chunk_containing_pass_gpu_p50_ms, 2.0);
        assert_eq!(report.chunk_containing_pass_gpu_max_ms, 2.0);
    }

    #[test]
    fn timed_session_discards_pre_ready_frame_samples() {
        let mut metrics = MetricsCollector::new();
        metrics.record_frame(Duration::from_millis(250));
        metrics.record_remesh_latency(Duration::from_secs(12));
        metrics.record_mutation_to_visible(Duration::from_secs(1));

        metrics.begin_timed_session(std::time::Instant::now());

        let report = metrics.report();
        assert_eq!(report.frame_count, 0);
        assert_eq!(report.max_remesh_ms, 0.0);
        assert_eq!(report.max_mutation_to_visible_ms, 0.0);
    }

    #[test]
    fn timed_session_freeze_excludes_grace_frames_but_accepts_final_presentation_metrics() {
        let started = std::time::Instant::now();
        let mut metrics = MetricsCollector::new();
        metrics.begin_timed_session(started);
        metrics.record_frame(Duration::from_millis(16));

        metrics.finish_timed_session(started + Duration::from_secs(60));
        metrics.record_frame(Duration::from_millis(500));
        metrics.record_pipeline_snapshot(PipelineMetricsSnapshot {
            gpu_upload_bytes: 1_000,
            transparent_sort: TransparentSortMetricsSnapshot {
                committed_generation: 9,
                encoded_generation: 9,
                presented_generation: 8,
                ref_count: 42,
                upload_bytes: 80,
                ..Default::default()
            },
            ..Default::default()
        });
        metrics.record_transparent_sort_snapshot(TransparentSortMetricsSnapshot {
            committed_generation: 9,
            encoded_generation: 9,
            presented_generation: 9,
            ref_count: 42,
            upload_bytes: 80,
            ..Default::default()
        });

        let report = metrics.report();
        assert_eq!(report.session_seconds, 60.0);
        assert_eq!(report.frame_count, 1);
        assert_eq!(report.p99_frame_ms, 16.0);
        assert_eq!(report.max_frame_ms, 16.0);
        assert_eq!(report.transparent_sort_presented_generation, 9);
        assert_eq!(report.transparent_sort_ref_count, 42);
        assert_eq!(report.nontransparent_gpu_upload_bytes, 1_000);
        assert_eq!(report.gpu_upload_bytes, 1_080);
    }

    #[test]
    fn report_publishes_exact_resident_and_visible_model_workload() {
        let mut metrics = MetricsCollector::new();
        metrics.record_pipeline_snapshot(PipelineMetricsSnapshot {
            model_workload: ModelWorkloadMetricsSnapshot {
                resident: ModelWorkloadCountSnapshot {
                    model_ref_count: 95,
                    model_draw_ref_count: 1_407,
                    legacy_fixed_slot_quad_invocations_avoided: 1_633,
                },
                visible: ModelWorkloadCountSnapshot {
                    model_ref_count: 77,
                    model_draw_ref_count: 1_123,
                    legacy_fixed_slot_quad_invocations_avoided: 1_341,
                },
            },
            ..Default::default()
        });

        let report = metrics.report();
        assert_eq!(report.resident_model_ref_count, 95);
        assert_eq!(report.resident_model_draw_ref_count, 1_407);
        assert_eq!(
            report.resident_legacy_fixed_slot_quad_invocations_avoided,
            1_633
        );
        assert_eq!(report.visible_model_ref_count, 77);
        assert_eq!(report.visible_model_draw_ref_count, 1_123);
        assert_eq!(
            report.visible_legacy_fixed_slot_quad_invocations_avoided,
            1_341
        );
    }

    #[test]
    fn report_uses_sorted_nearest_rank_metrics() {
        let mut metrics = MetricsCollector::new();
        for milliseconds in 1..=100 {
            metrics.record_frame(Duration::from_secs_f64(milliseconds as f64 / 1_000.0));
        }
        metrics.record_remesh_latency(Duration::from_millis(42));
        metrics.record_mutation_to_visible(Duration::from_millis(75));
        metrics.record_teleport_settle(Duration::from_millis(23_400));
        metrics.record_forced_full_view_remesh(Duration::from_millis(1_234));
        metrics.add_decode_errors(3);
        metrics.record_pipeline_snapshot(PipelineMetricsSnapshot {
            world_ready: true,
            requested_radius_chunks: 16,
            received_radius_chunks: Some(16),
            publisher_radius_chunks: Some(16),
            mutation_coordinate: Some([4, 65, -2]),
            visible_mutation_count: 3,
            max_decode: Duration::from_millis(11),
            max_mesh: Duration::from_millis(22),
            max_remesh: Duration::from_millis(42),
            rendered_sub_chunks: 13,
            resident_sub_chunks: 19,
            visible_sub_chunks: 17,
            admitted_world_events: 7,
            admitted_heavy_events: 5,
            queued_decode_jobs: 4,
            in_flight_decode_jobs: 3,
            completed_decode_results: 2,
            pending_retry_requests: 1,
            outbound_requests: 6,
            pending_mesh_jobs: 9,
            in_flight_mesh_jobs: 8,
            gpu_upload_bytes: 12_345,
            transparent_sort: TransparentSortMetricsSnapshot {
                request_generation: 21,
                result_generation: 20,
                committed_generation: 19,
                encoded_generation: 19,
                presented_generation: 18,
                ref_count: 4_096,
                cpu_duration: Duration::from_micros(1_250),
                request_to_commit_latency: Duration::from_micros(2_500),
                staged_bytes: 32_768,
                upload_bytes: 16_384,
                stale_reject_count: 3,
                ceiling_reject_count: 2,
                active_slot_age_frames: 7,
                transparent_water_distinct_tint_count: 5,
            },
            model_workload: ModelWorkloadMetricsSnapshot::default(),
        });

        let report = metrics.report();
        assert_eq!(report.frame_count, 100);
        assert!(report.world_ready);
        assert_eq!(report.requested_radius_chunks, 16);
        assert_eq!(report.received_radius_chunks, Some(16));
        assert_eq!(report.publisher_radius_chunks, Some(16));
        assert_eq!(report.mutation_coordinate, Some([4, 65, -2]));
        assert_eq!(report.visible_mutation_count, 3);
        assert_eq!(report.p50_frame_ms, 51.0);
        assert_eq!(report.p95_frame_ms, 96.0);
        assert_eq!(report.p99_frame_ms, 100.0);
        assert_eq!(report.max_frame_ms, 100.0);
        assert_eq!(report.max_decode_ms, 11.0);
        assert_eq!(report.max_mesh_ms, 22.0);
        assert_eq!(report.max_remesh_ms, 42.0);
        assert_eq!(report.max_mutation_to_visible_ms, 75.0);
        assert_eq!(report.teleport_settle_ms, Some(23_400.0));
        assert_eq!(report.forced_full_view_remesh_ms, Some(1_234.0));
        assert_eq!(report.decode_error_count, 3);
        assert_eq!(report.rendered_sub_chunks, 13);
        assert_eq!(report.resident_sub_chunks, 19);
        assert_eq!(report.visible_sub_chunks, 17);
        assert_eq!(report.peak_admitted_world_events, 7);
        assert_eq!(report.peak_admitted_heavy_events, 5);
        assert_eq!(report.peak_queued_decode_jobs, 4);
        assert_eq!(report.peak_in_flight_decode_jobs, 3);
        assert_eq!(report.peak_completed_decode_results, 2);
        assert_eq!(report.peak_pending_retry_requests, 1);
        assert_eq!(report.peak_outbound_requests, 6);
        assert_eq!(report.peak_pending_mesh_jobs, 9);
        assert_eq!(report.peak_in_flight_mesh_jobs, 8);
        assert_eq!(report.nontransparent_gpu_upload_bytes, 12_345);
        assert_eq!(
            report.gpu_upload_bytes,
            report.nontransparent_gpu_upload_bytes + report.transparent_sort_upload_bytes
        );
        assert_eq!(report.transparent_sort_request_generation, 21);
        assert_eq!(report.transparent_sort_result_generation, 20);
        assert_eq!(report.transparent_sort_committed_generation, 19);
        assert_eq!(report.transparent_sort_encoded_generation, 19);
        assert_eq!(report.transparent_sort_presented_generation, 18);
        assert_eq!(report.transparent_sort_ref_count, 4_096);
        assert_eq!(report.transparent_sort_cpu_ms, 1.25);
        assert_eq!(report.transparent_sort_request_to_commit_ms, 2.5);
        assert_eq!(report.transparent_sort_staged_bytes, 32_768);
        assert_eq!(report.transparent_sort_upload_bytes, 16_384);
        assert_eq!(report.transparent_sort_stale_reject_count, 3);
        assert_eq!(report.transparent_sort_ceiling_reject_count, 2);
        assert_eq!(report.transparent_sort_active_slot_age_frames, 7);
        assert_eq!(report.transparent_water_distinct_tint_count, 5);
    }

    #[test]
    fn render_transparent_sort_snapshot_conversion_is_exact() {
        let source = render::TransparentSortMetricsSnapshot {
            request_generation: 31,
            result_generation: 30,
            committed_generation: 29,
            encoded_generation: 29,
            presented_generation: 28,
            ref_count: 27,
            cpu_duration: Duration::from_micros(26),
            request_to_commit_latency: Duration::from_micros(25),
            staged_bytes: 24,
            upload_bytes: 23,
            stale_reject_count: 22,
            ceiling_reject_count: 21,
            active_slot_age_frames: 20,
            transparent_water_distinct_tint_count: 19,
        };

        assert_eq!(
            TransparentSortMetricsSnapshot::from(source),
            TransparentSortMetricsSnapshot {
                request_generation: 31,
                result_generation: 30,
                committed_generation: 29,
                encoded_generation: 29,
                presented_generation: 28,
                ref_count: 27,
                cpu_duration: Duration::from_micros(26),
                request_to_commit_latency: Duration::from_micros(25),
                staged_bytes: 24,
                upload_bytes: 23,
                stale_reject_count: 22,
                ceiling_reject_count: 21,
                active_slot_age_frames: 20,
                transparent_water_distinct_tint_count: 19,
            }
        );
    }

    #[test]
    fn frame_quantiles_use_constant_memory_and_are_deterministic_at_large_sample_counts() {
        let mut first = MetricsCollector::new();
        let mut second = MetricsCollector::new();
        let capacity = first.frame_sample_capacity();

        for sample in 0..100_000 {
            let duration = Duration::from_micros((sample % 2_000 + 1) as u64 * 100);
            first.record_frame(duration);
            second.record_frame(duration);
        }

        assert_eq!(first.frame_sample_capacity(), capacity);
        assert_eq!(second.frame_sample_capacity(), capacity);
        assert!(capacity < 100_000);
        let first = first.report();
        let second = second.report();
        assert_eq!(first.frame_count, 100_000);
        assert_eq!(first.p50_frame_ms, second.p50_frame_ms);
        assert_eq!(first.p95_frame_ms, second.p95_frame_ms);
        assert_eq!(first.p99_frame_ms, second.p99_frame_ms);
        assert_eq!(first.max_frame_ms, second.max_frame_ms);
    }

    fn exact_full_view_proof(
        milliseconds: f64,
        view_generation: u64,
        manifest_hash: &str,
    ) -> ExactFullViewProof {
        ExactFullViewProof {
            target: "0:65:65:16".to_owned(),
            committed: "0:65:65:16".to_owned(),
            ms: milliseconds,
            view_generation,
            transparent_sort_generation: 6,
            render_ready_ms: 100.0,
            first_frame_sequence: 41,
            stable_frame_sequence: 42,
            first_present_ms: 110.0,
            first_gpu_ms: 120.0,
            stable_present_ms: 130.0,
            stable_gpu_ms: milliseconds,
            frame_count: 12,
            expected_manifest_count: 4,
            expected_manifest_hash: manifest_hash.to_owned(),
            first_presented_manifest_count: 4,
            first_presented_manifest_hash: manifest_hash.to_owned(),
            stable_presented_manifest_count: 4,
            stable_presented_manifest_hash: manifest_hash.to_owned(),
            expected: 797,
            loaded_target: 797,
            missing_target: 0,
            foreign_loaded: 0,
            foreign_requested: 0,
            foreign_resident: 0,
            source_leftover: 0,
            resident_count: 3,
            resident_hash: "aaaabbbbccccdddd".to_owned(),
            known_air_count: 1,
            known_air_hash: "eeeeffff00001111".to_owned(),
            missing_target_instances: 0,
            unexpected_target_instances: 0,
            source_instances: 0,
            foreign_instances: 0,
            stale_generation_instances: 0,
            orphan_allocations: 0,
        }
    }

    #[test]
    fn binding_and_secondary_metrics_remain_distinct() {
        let mut metrics = MetricsCollector::new();
        let teleport = TeleportProof {
            exact: exact_full_view_proof(2_400.0, 7, "1111222233334444"),
            publisher_ms: Some(10.0),
            first_level_ms: Some(20.0),
            last_level_ms: Some(30.0),
            level_events: 1_089,
            first_sub_ms: Some(40.0),
            last_sub_ms: Some(50.0),
            sub_events: 1_089,
        };
        let remesh = exact_full_view_proof(150.0, 8, "5555666677778888");

        metrics.record_teleport_proof(teleport.clone());
        metrics.record_forced_full_view_remesh_proof(remesh.clone());

        let report = metrics.report();
        assert_eq!(report.teleport_settle_ms, Some(2_400.0));
        assert_eq!(report.forced_full_view_remesh_ms, Some(150.0));
        assert_eq!(report.teleport_proof, Some(teleport));
        assert_eq!(report.forced_full_view_remesh_proof, Some(remesh));
    }

    #[test]
    fn cohort_stage_and_frame_evidence_serializes_deterministically_with_missing_stages_as_null() {
        let key_a = SubChunkKey::new(0, 1, -4, 2);
        let key_b = SubChunkKey::new(0, 0, -4, 0);
        assert_eq!(
            deterministic_manifest_hash(&[(key_a, 9), (key_b, 7)]),
            deterministic_manifest_hash(&[(key_b, 7), (key_a, 9)])
        );

        let mut metrics = MetricsCollector::new();
        metrics.record_teleport_proof(TeleportProof {
            exact: exact_full_view_proof(1_500.0, 7, "1111222233334444"),
            publisher_ms: None,
            first_level_ms: None,
            last_level_ms: None,
            level_events: 0,
            first_sub_ms: None,
            last_sub_ms: None,
            sub_events: 0,
        });
        metrics.record_forced_full_view_remesh_proof(exact_full_view_proof(
            1_500.0,
            8,
            "5555666677778888",
        ));
        let report = metrics.report();
        let first = serde_json::to_string_pretty(&report).unwrap();
        let second = serde_json::to_string_pretty(&report).unwrap();
        assert_eq!(first, second);

        let document = serde_json::to_value(report).unwrap();
        let teleport = &document["teleport_proof"];
        assert_eq!(teleport["target"], "0:65:65:16");
        assert_eq!(teleport["expected"], 797);
        assert_eq!(teleport["frame_count"], 12);
        assert_eq!(teleport["transparent_sort_generation"], 6);
        for stage in [
            "publisher_ms",
            "first_level_ms",
            "last_level_ms",
            "first_sub_ms",
            "last_sub_ms",
        ] {
            assert!(teleport[stage].is_null(), "{stage} was not JSON null");
        }
        assert!(!first.contains(": -1"));
    }

    #[test]
    fn json_output_is_pretty_deterministic_and_newline_terminated() {
        let report = MetricsReport {
            session_seconds: 15.0,
            world_ready: true,
            requested_radius_chunks: 16,
            received_radius_chunks: Some(16),
            publisher_radius_chunks: Some(16),
            mutation_coordinate: Some([4, 65, -2]),
            visible_mutation_count: 3,
            frame_count: 2,
            p50_frame_ms: 4.0,
            p95_frame_ms: 5.0,
            p99_frame_ms: 5.0,
            max_frame_ms: 5.0,
            gpu_pass_sample_count: 2,
            opaque_3d_gpu_p50_ms: 2.0,
            opaque_3d_gpu_p95_ms: 3.0,
            opaque_3d_gpu_p99_ms: 3.0,
            opaque_3d_gpu_max_ms: 3.0,
            transparent_3d_gpu_p50_ms: 0.5,
            transparent_3d_gpu_p95_ms: 1.0,
            transparent_3d_gpu_p99_ms: 1.0,
            transparent_3d_gpu_max_ms: 1.0,
            chunk_containing_pass_gpu_p50_ms: 2.5,
            chunk_containing_pass_gpu_p95_ms: 4.0,
            chunk_containing_pass_gpu_p99_ms: 4.0,
            chunk_containing_pass_gpu_max_ms: 4.0,
            max_decode_ms: 6.0,
            max_mesh_ms: 7.0,
            max_remesh_ms: 20.0,
            teleport_settle_ms: Some(23_400.0),
            forced_full_view_remesh_ms: Some(1_234.0),
            teleport_proof: None,
            forced_full_view_remesh_proof: None,
            max_mutation_to_visible_ms: 75.0,
            decode_error_count: 0,
            rendered_sub_chunks: 13,
            resident_sub_chunks: 14,
            visible_sub_chunks: 12,
            peak_admitted_world_events: 8,
            peak_admitted_heavy_events: 7,
            peak_queued_decode_jobs: 6,
            peak_in_flight_decode_jobs: 4,
            peak_completed_decode_results: 3,
            peak_pending_retry_requests: 2,
            peak_outbound_requests: 5,
            peak_pending_mesh_jobs: 9,
            peak_in_flight_mesh_jobs: 8,
            nontransparent_gpu_upload_bytes: 3_072,
            gpu_upload_bytes: 4_096,
            transparent_sort_request_generation: 11,
            transparent_sort_result_generation: 10,
            transparent_sort_committed_generation: 9,
            transparent_sort_encoded_generation: 9,
            transparent_sort_presented_generation: 8,
            transparent_sort_ref_count: 256,
            transparent_sort_cpu_ms: 1.25,
            transparent_sort_request_to_commit_ms: 3.5,
            transparent_sort_staged_bytes: 2_048,
            transparent_sort_upload_bytes: 1_024,
            transparent_sort_stale_reject_count: 2,
            transparent_sort_ceiling_reject_count: 1,
            transparent_sort_active_slot_age_frames: 4,
            transparent_water_distinct_tint_count: 3,
            resident_model_ref_count: 95,
            resident_model_draw_ref_count: 1_407,
            resident_legacy_fixed_slot_quad_invocations_avoided: 1_633,
            visible_model_ref_count: 77,
            visible_model_draw_ref_count: 1_123,
            visible_legacy_fixed_slot_quad_invocations_avoided: 1_341,
            assets: AssetMetrics::default(),
        };
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory =
            std::env::temp_dir().join(format!("rust-mcbe-metrics-{}-{unique}", std::process::id()));
        let path = directory.join("nested/metrics.json");
        report.write_json(&path).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            concat!(
                "{\n",
                "  \"session_seconds\": 15.0,\n",
                "  \"world_ready\": true,\n",
                "  \"requested_radius_chunks\": 16,\n",
                "  \"received_radius_chunks\": 16,\n",
                "  \"publisher_radius_chunks\": 16,\n",
                "  \"mutation_coordinate\": [\n",
                "    4,\n",
                "    65,\n",
                "    -2\n",
                "  ],\n",
                "  \"visible_mutation_count\": 3,\n",
                "  \"frame_count\": 2,\n",
                "  \"p50_frame_ms\": 4.0,\n",
                "  \"p95_frame_ms\": 5.0,\n",
                "  \"p99_frame_ms\": 5.0,\n",
                "  \"max_frame_ms\": 5.0,\n",
                "  \"gpu_pass_sample_count\": 2,\n",
                "  \"opaque_3d_gpu_p50_ms\": 2.0,\n",
                "  \"opaque_3d_gpu_p95_ms\": 3.0,\n",
                "  \"opaque_3d_gpu_p99_ms\": 3.0,\n",
                "  \"opaque_3d_gpu_max_ms\": 3.0,\n",
                "  \"transparent_3d_gpu_p50_ms\": 0.5,\n",
                "  \"transparent_3d_gpu_p95_ms\": 1.0,\n",
                "  \"transparent_3d_gpu_p99_ms\": 1.0,\n",
                "  \"transparent_3d_gpu_max_ms\": 1.0,\n",
                "  \"chunk_containing_pass_gpu_p50_ms\": 2.5,\n",
                "  \"chunk_containing_pass_gpu_p95_ms\": 4.0,\n",
                "  \"chunk_containing_pass_gpu_p99_ms\": 4.0,\n",
                "  \"chunk_containing_pass_gpu_max_ms\": 4.0,\n",
                "  \"max_decode_ms\": 6.0,\n",
                "  \"max_mesh_ms\": 7.0,\n",
                "  \"max_remesh_ms\": 20.0,\n",
                "  \"teleport_settle_ms\": 23400.0,\n",
                "  \"forced_full_view_remesh_ms\": 1234.0,\n",
                "  \"teleport_proof\": null,\n",
                "  \"forced_full_view_remesh_proof\": null,\n",
                "  \"max_mutation_to_visible_ms\": 75.0,\n",
                "  \"decode_error_count\": 0,\n",
                "  \"rendered_sub_chunks\": 13,\n",
                "  \"resident_sub_chunks\": 14,\n",
                "  \"visible_sub_chunks\": 12,\n",
                "  \"peak_admitted_world_events\": 8,\n",
                "  \"peak_admitted_heavy_events\": 7,\n",
                "  \"peak_queued_decode_jobs\": 6,\n",
                "  \"peak_in_flight_decode_jobs\": 4,\n",
                "  \"peak_completed_decode_results\": 3,\n",
                "  \"peak_pending_retry_requests\": 2,\n",
                "  \"peak_outbound_requests\": 5,\n",
                "  \"peak_pending_mesh_jobs\": 9,\n",
                "  \"peak_in_flight_mesh_jobs\": 8,\n",
                "  \"nontransparent_gpu_upload_bytes\": 3072,\n",
                "  \"gpu_upload_bytes\": 4096,\n",
                "  \"transparent_sort_request_generation\": 11,\n",
                "  \"transparent_sort_result_generation\": 10,\n",
                "  \"transparent_sort_committed_generation\": 9,\n",
                "  \"transparent_sort_encoded_generation\": 9,\n",
                "  \"transparent_sort_presented_generation\": 8,\n",
                "  \"transparent_sort_ref_count\": 256,\n",
                "  \"transparent_sort_cpu_ms\": 1.25,\n",
                "  \"transparent_sort_request_to_commit_ms\": 3.5,\n",
                "  \"transparent_sort_staged_bytes\": 2048,\n",
                "  \"transparent_sort_upload_bytes\": 1024,\n",
                "  \"transparent_sort_stale_reject_count\": 2,\n",
                "  \"transparent_sort_ceiling_reject_count\": 1,\n",
                "  \"transparent_sort_active_slot_age_frames\": 4,\n",
                "  \"transparent_water_distinct_tint_count\": 3,\n",
                "  \"resident_model_ref_count\": 95,\n",
                "  \"resident_model_draw_ref_count\": 1407,\n",
                "  \"resident_legacy_fixed_slot_quad_invocations_avoided\": 1633,\n",
                "  \"visible_model_ref_count\": 77,\n",
                "  \"visible_model_draw_ref_count\": 1123,\n",
                "  \"visible_legacy_fixed_slot_quad_invocations_avoided\": 1341,\n",
                "  \"assets\": {\n",
                "    \"source_tag\": \"diagnostic\",\n",
                "    \"source_sha256\": \"diagnostic\",\n",
                "    \"blob_sha256\": \"diagnostic\",\n",
                "    \"texture_layers\": 1,\n",
                "    \"texture_pages\": 1,\n",
                "    \"texture_bytes_including_mips\": 1364,\n",
                "    \"material_count\": 1,\n",
                "    \"model_template_count\": 0,\n",
                "    \"model_quad_count\": 0,\n",
                "    \"animation_count\": 0,\n",
                "    \"animation_frame_count\": 0,\n",
                "    \"missing_mapping_count\": 0,\n",
                "    \"diagnostic_quad_count\": 0\n",
                "  }\n",
                "}\n",
            )
        );
        let _ = fs::remove_dir_all(directory);
    }
}
