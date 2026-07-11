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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetMetrics {
    pub source_tag: String,
    pub source_sha256: String,
    pub blob_sha256: String,
    pub texture_layers: u32,
    pub texture_bytes_including_mips: u64,
    pub material_count: u32,
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
            texture_bytes_including_mips: 1_364,
            material_count: 1,
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
            "WORLD_READY source_tag={} source_sha256={} blob_sha256={} texture_layers={} \
             texture_bytes_including_mips={} material_count={} missing_mapping_count={} \
             diagnostic_quad_count={} resident_sub_chunks={resident_sub_chunks} \
             visible_sub_chunks={visible_sub_chunks}",
            self.source_tag,
            self.source_sha256,
            self.blob_sha256,
            self.texture_layers,
            self.texture_bytes_including_mips,
            self.material_count,
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

#[derive(Debug)]
pub struct MetricsCollector {
    started: Instant,
    assets: AssetMetrics,
    frame_histogram: FrameHistogram,
    world_ready: bool,
    requested_radius_chunks: i32,
    received_radius_chunks: Option<i32>,
    publisher_radius_chunks: Option<i32>,
    mutation_coordinate: Option<[i32; 3]>,
    visible_mutation_count: u64,
    max_remesh_milliseconds: f64,
    teleport_settle_milliseconds: Option<f64>,
    forced_full_view_remesh_milliseconds: Option<f64>,
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
            assets: AssetMetrics::default(),
            frame_histogram: FrameHistogram::default(),
            world_ready: false,
            requested_radius_chunks: 0,
            received_radius_chunks: None,
            publisher_radius_chunks: None,
            mutation_coordinate: None,
            visible_mutation_count: 0,
            max_remesh_milliseconds: 0.0,
            teleport_settle_milliseconds: None,
            forced_full_view_remesh_milliseconds: None,
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
        self.frame_histogram
            .record(duration.as_secs_f64() * 1_000.0);
    }

    pub fn begin_timed_session(&mut self, started: Instant) {
        self.started = started;
        self.frame_histogram = FrameHistogram::default();
        self.max_remesh_milliseconds = 0.0;
        self.max_mutation_to_visible_milliseconds = 0.0;
        self.max_decode_milliseconds = 0.0;
        self.max_mesh_milliseconds = 0.0;
        self.teleport_settle_milliseconds = None;
        self.forced_full_view_remesh_milliseconds = None;
    }

    pub fn record_remesh_latency(&mut self, duration: Duration) {
        self.max_remesh_milliseconds = self
            .max_remesh_milliseconds
            .max(duration.as_secs_f64() * 1_000.0);
    }

    pub fn record_mutation_to_visible(&mut self, duration: Duration) {
        self.max_mutation_to_visible_milliseconds = self
            .max_mutation_to_visible_milliseconds
            .max(duration.as_secs_f64() * 1_000.0);
    }

    pub fn record_teleport_settle(&mut self, duration: Duration) {
        self.teleport_settle_milliseconds = Some(duration.as_secs_f64() * 1_000.0);
    }

    pub fn record_forced_full_view_remesh(&mut self, duration: Duration) {
        self.forced_full_view_remesh_milliseconds = Some(duration.as_secs_f64() * 1_000.0);
    }

    pub fn add_decode_errors(&mut self, count: u64) {
        self.decode_errors = self.decode_errors.saturating_add(count);
    }

    pub fn record_pipeline_snapshot(&mut self, snapshot: PipelineMetricsSnapshot) {
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
    }

    #[must_use]
    pub fn report(&self) -> MetricsReport {
        MetricsReport {
            session_seconds: self.started.elapsed().as_secs_f64(),
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
            max_decode_ms: self.max_decode_milliseconds,
            max_mesh_ms: self.max_mesh_milliseconds,
            max_remesh_ms: self.max_remesh_milliseconds,
            teleport_settle_ms: self.teleport_settle_milliseconds,
            forced_full_view_remesh_ms: self.forced_full_view_remesh_milliseconds,
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
            gpu_upload_bytes: self.gpu_upload_bytes,
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
    pub max_decode_ms: f64,
    pub max_mesh_ms: f64,
    pub max_remesh_ms: f64,
    pub teleport_settle_ms: Option<f64>,
    pub forced_full_view_remesh_ms: Option<f64>,
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
    pub gpu_upload_bytes: u64,
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
        AssetMetrics, MetricsCollector, MetricsReport, PipelineMetricsSnapshot, percentile,
    };
    use std::{fs, time::Duration};

    #[test]
    fn empty_percentiles_are_zero() {
        assert_eq!(percentile(&[], 0.99), 0.0);
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
        assert_eq!(report.gpu_upload_bytes, 12_345);
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
            max_decode_ms: 6.0,
            max_mesh_ms: 7.0,
            max_remesh_ms: 20.0,
            teleport_settle_ms: Some(23_400.0),
            forced_full_view_remesh_ms: Some(1_234.0),
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
            gpu_upload_bytes: 4_096,
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
                "  \"max_decode_ms\": 6.0,\n",
                "  \"max_mesh_ms\": 7.0,\n",
                "  \"max_remesh_ms\": 20.0,\n",
                "  \"teleport_settle_ms\": 23400.0,\n",
                "  \"forced_full_view_remesh_ms\": 1234.0,\n",
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
                "  \"gpu_upload_bytes\": 4096,\n",
                "  \"assets\": {\n",
                "    \"source_tag\": \"diagnostic\",\n",
                "    \"source_sha256\": \"diagnostic\",\n",
                "    \"blob_sha256\": \"diagnostic\",\n",
                "    \"texture_layers\": 1,\n",
                "    \"texture_bytes_including_mips\": 1364,\n",
                "    \"material_count\": 1,\n",
                "    \"missing_mapping_count\": 0,\n",
                "    \"diagnostic_quad_count\": 0\n",
                "  }\n",
                "}\n",
            )
        );
        let _ = fs::remove_dir_all(directory);
    }
}
