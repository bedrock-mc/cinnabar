use std::{
    collections::BTreeMap,
    fmt::Write as _,
    fs, io,
    path::Path,
    sync::OnceLock,
    time::{Duration, Instant},
};

use meshing::DiagnosticGeometrySummary;
use serde::Serialize;
use world::SubChunkKey;

const FRAME_HISTOGRAM_RESOLUTION_MS: f64 = 0.1;
const FRAME_HISTOGRAM_BUCKETS: usize = 20_001;
pub const DIAGNOSTIC_TOP_LIMIT: usize = 8;
const MAX_TRACKED_DIAGNOSTIC_IDENTITIES: usize = 16_913 + 256;

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
    pub diagnostic_attribution: DiagnosticAttributionSnapshot,
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
            diagnostic_attribution: DiagnosticAttributionSnapshot::default(),
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
             diagnostic_quad_count={} {} resident_sub_chunks={resident_sub_chunks} \
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
            self.diagnostic_attribution.marker_fields(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticAttributionEntry {
    pub sequential_id: Option<u32>,
    pub network_id: u32,
    pub name: String,
    pub quad_count: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DiagnosticAttributionSnapshot {
    pub total_quad_count: u64,
    pub top: Vec<DiagnosticAttributionEntry>,
    pub omitted_identity_count: u64,
    pub omitted_quad_count: u64,
}

impl DiagnosticAttributionSnapshot {
    #[must_use]
    pub fn marker_fields(&self) -> String {
        let mut top = String::new();
        for (index, entry) in self.top.iter().enumerate() {
            if index != 0 {
                top.push(',');
            }
            let sequential = entry
                .sequential_id
                .map_or_else(|| "unknown".to_owned(), |id| id.to_string());
            let _ = write!(
                top,
                "{sequential}|0x{:08x}|{}|{}",
                entry.network_id, entry.name, entry.quad_count
            );
        }
        if top.is_empty() {
            top.push_str("none");
        }
        format!(
            "diagnostic_attribution_total={} diagnostic_attribution_top={} diagnostic_attribution_omitted_identities={} diagnostic_attribution_omitted_quads={}",
            self.total_quad_count, top, self.omitted_identity_count, self.omitted_quad_count
        )
    }
}

#[derive(Debug)]
struct DiagnosticCatalogEntry {
    name: Box<str>,
}

fn protocol_1001_catalog() -> &'static [DiagnosticCatalogEntry] {
    static CATALOG: OnceLock<Box<[DiagnosticCatalogEntry]>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let records = assets::read_registry(include_bytes!(
            "../../crates/assets/data/block-registry-v1001.bin"
        ))
        .expect("checked-in protocol-1001 registry must remain valid");
        records
            .into_vec()
            .into_iter()
            .enumerate()
            .map(|(index, record)| {
                assert_eq!(record.sequential_id as usize, index);
                DiagnosticCatalogEntry { name: record.name }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice()
    })
}

#[derive(Debug)]
struct ResidentDiagnosticContribution {
    summary: DiagnosticGeometrySummary,
    tracked_entries: Box<[bool]>,
}

#[derive(Debug)]
pub struct DiagnosticQuadTracker {
    by_sub_chunk: BTreeMap<SubChunkKey, ResidentDiagnosticContribution>,
    totals: BTreeMap<(Option<u32>, u32), u64>,
    total: u64,
    explicit_omitted_identity_count: u64,
    explicit_omitted_quad_count: u64,
    revision: u64,
    identity_capacity: usize,
}

impl Default for DiagnosticQuadTracker {
    fn default() -> Self {
        Self {
            by_sub_chunk: BTreeMap::new(),
            totals: BTreeMap::new(),
            total: 0,
            explicit_omitted_identity_count: 0,
            explicit_omitted_quad_count: 0,
            revision: 0,
            identity_capacity: MAX_TRACKED_DIAGNOSTIC_IDENTITIES,
        }
    }
}

impl DiagnosticQuadTracker {
    #[cfg(test)]
    fn with_identity_capacity(identity_capacity: usize) -> Self {
        Self {
            identity_capacity,
            ..Self::default()
        }
    }

    pub fn upsert(&mut self, key: SubChunkKey, summary: DiagnosticGeometrySummary) {
        if self
            .by_sub_chunk
            .get(&key)
            .is_some_and(|resident| resident.summary == summary)
        {
            return;
        }
        if summary.total_quad_count() == 0 {
            self.remove(key);
            return;
        }
        if let Some(previous) = self.by_sub_chunk.remove(&key) {
            self.subtract_contribution(&previous);
        }
        let contribution = self.add_summary(summary);
        self.by_sub_chunk.insert(key, contribution);
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn remove(&mut self, key: SubChunkKey) {
        if let Some(previous) = self.by_sub_chunk.remove(&key) {
            self.subtract_contribution(&previous);
            self.revision = self.revision.wrapping_add(1);
        }
    }

    #[must_use]
    pub const fn total(&self) -> u64 {
        self.total
    }

    #[must_use]
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn snapshot(&self) -> DiagnosticAttributionSnapshot {
        let catalog = protocol_1001_catalog();
        let mut counts = self
            .totals
            .iter()
            .map(|(&(sequential_id, network_id), &quad_count)| {
                (sequential_id, network_id, quad_count)
            })
            .collect::<Vec<_>>();
        counts.sort_unstable_by(|left, right| {
            right
                .2
                .cmp(&left.2)
                .then_with(|| left.0.unwrap_or(u32::MAX).cmp(&right.0.unwrap_or(u32::MAX)))
                .then_with(|| left.1.cmp(&right.1))
        });
        let omitted = counts.split_off(counts.len().min(DIAGNOSTIC_TOP_LIMIT));
        let top = counts
            .into_iter()
            .map(|(sequential_id, network_id, quad_count)| {
                let name = sequential_id
                    .and_then(|id| catalog.get(id as usize))
                    .map_or_else(|| "unknown".to_owned(), |entry| entry.name.to_string());
                DiagnosticAttributionEntry {
                    sequential_id,
                    network_id,
                    name,
                    quad_count,
                }
            })
            .collect();
        DiagnosticAttributionSnapshot {
            total_quad_count: self.total,
            top,
            omitted_identity_count: self
                .explicit_omitted_identity_count
                .saturating_add(omitted.len() as u64),
            omitted_quad_count: self.explicit_omitted_quad_count.saturating_add(
                omitted
                    .into_iter()
                    .map(|(_, _, quad_count)| quad_count)
                    .sum::<u64>(),
            ),
        }
    }

    fn add_summary(
        &mut self,
        summary: DiagnosticGeometrySummary,
    ) -> ResidentDiagnosticContribution {
        self.total = self.total.saturating_add(summary.total_quad_count());
        self.explicit_omitted_identity_count = self
            .explicit_omitted_identity_count
            .saturating_add(u64::from(summary.omitted_identity_count()));
        self.explicit_omitted_quad_count = self
            .explicit_omitted_quad_count
            .saturating_add(summary.omitted_quad_count());
        let mut tracked_entries = Vec::with_capacity(summary.entries().len());
        for count in summary.entries() {
            let key = (count.sequential_id(), count.network_id());
            if let Some(total) = self.totals.get_mut(&key) {
                *total = total.saturating_add(u64::from(count.quad_count()));
                tracked_entries.push(true);
            } else if self.totals.len() < self.identity_capacity {
                self.totals.insert(key, u64::from(count.quad_count()));
                tracked_entries.push(true);
            } else {
                self.explicit_omitted_identity_count =
                    self.explicit_omitted_identity_count.saturating_add(1);
                self.explicit_omitted_quad_count = self
                    .explicit_omitted_quad_count
                    .saturating_add(u64::from(count.quad_count()));
                tracked_entries.push(false);
            }
        }
        ResidentDiagnosticContribution {
            summary,
            tracked_entries: tracked_entries.into_boxed_slice(),
        }
    }

    fn subtract_contribution(&mut self, contribution: &ResidentDiagnosticContribution) {
        let summary = &contribution.summary;
        self.total = self.total.saturating_sub(summary.total_quad_count());
        self.explicit_omitted_identity_count = self
            .explicit_omitted_identity_count
            .saturating_sub(u64::from(summary.omitted_identity_count()));
        self.explicit_omitted_quad_count = self
            .explicit_omitted_quad_count
            .saturating_sub(summary.omitted_quad_count());
        debug_assert_eq!(summary.entries().len(), contribution.tracked_entries.len());
        for (count, tracked) in summary
            .entries()
            .iter()
            .zip(contribution.tracked_entries.iter().copied())
        {
            let key = (count.sequential_id(), count.network_id());
            if tracked {
                let remove = self.totals.get_mut(&key).is_some_and(|total| {
                    *total = total.saturating_sub(u64::from(count.quad_count()));
                    *total == 0
                });
                if remove {
                    self.totals.remove(&key);
                }
            } else {
                self.explicit_omitted_identity_count =
                    self.explicit_omitted_identity_count.saturating_sub(1);
                self.explicit_omitted_quad_count = self
                    .explicit_omitted_quad_count
                    .saturating_sub(u64::from(count.quad_count()));
            }
        }
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

    pub fn record_diagnostic_attribution(&mut self, snapshot: DiagnosticAttributionSnapshot) {
        self.assets.diagnostic_attribution = snapshot;
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
mod tests;
