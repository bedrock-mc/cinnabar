use std::{
    fs, io,
    path::Path,
    time::{Duration, Instant},
};

use serde::Serialize;

#[derive(Debug)]
pub struct MetricsCollector {
    started: Instant,
    frame_milliseconds: Vec<f64>,
    max_remesh_milliseconds: f64,
    max_decode_milliseconds: f64,
    max_mesh_milliseconds: f64,
    decode_errors: u64,
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
    pub max_decode: Duration,
    pub max_mesh: Duration,
    pub max_remesh: Duration,
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
            frame_milliseconds: Vec::new(),
            max_remesh_milliseconds: 0.0,
            max_decode_milliseconds: 0.0,
            max_mesh_milliseconds: 0.0,
            decode_errors: 0,
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

    pub fn record_frame(&mut self, duration: Duration) {
        self.frame_milliseconds
            .push(duration.as_secs_f64() * 1_000.0);
    }

    pub fn record_remesh_latency(&mut self, duration: Duration) {
        self.max_remesh_milliseconds = self
            .max_remesh_milliseconds
            .max(duration.as_secs_f64() * 1_000.0);
    }

    pub fn add_decode_errors(&mut self, count: u64) {
        self.decode_errors = self.decode_errors.saturating_add(count);
    }

    pub fn record_pipeline_snapshot(&mut self, snapshot: PipelineMetricsSnapshot) {
        self.max_decode_milliseconds = self
            .max_decode_milliseconds
            .max(snapshot.max_decode.as_secs_f64() * 1_000.0);
        self.max_mesh_milliseconds = self
            .max_mesh_milliseconds
            .max(snapshot.max_mesh.as_secs_f64() * 1_000.0);
        self.record_remesh_latency(snapshot.max_remesh);
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
        let mut frames = self.frame_milliseconds.clone();
        frames.sort_by(f64::total_cmp);
        MetricsReport {
            session_seconds: self.started.elapsed().as_secs_f64(),
            frame_count: frames.len(),
            p50_frame_ms: percentile(&frames, 0.50),
            p95_frame_ms: percentile(&frames, 0.95),
            p99_frame_ms: percentile(&frames, 0.99),
            max_frame_ms: frames.last().copied().unwrap_or(0.0),
            max_decode_ms: self.max_decode_milliseconds,
            max_mesh_ms: self.max_mesh_milliseconds,
            max_remesh_ms: self.max_remesh_milliseconds,
            decode_error_count: self.decode_errors,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricsReport {
    pub session_seconds: f64,
    pub frame_count: usize,
    pub p50_frame_ms: f64,
    pub p95_frame_ms: f64,
    pub p99_frame_ms: f64,
    pub max_frame_ms: f64,
    pub max_decode_ms: f64,
    pub max_mesh_ms: f64,
    pub max_remesh_ms: f64,
    pub decode_error_count: u64,
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

fn percentile(sorted: &[f64], percentile: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let index = ((sorted.len() - 1) as f64 * percentile).ceil() as usize;
    sorted[index]
}

#[cfg(test)]
mod tests {
    use super::{MetricsCollector, MetricsReport, PipelineMetricsSnapshot, percentile};
    use std::{fs, time::Duration};

    #[test]
    fn empty_percentiles_are_zero() {
        assert_eq!(percentile(&[], 0.99), 0.0);
    }

    #[test]
    fn report_uses_sorted_nearest_rank_metrics() {
        let mut metrics = MetricsCollector::new();
        for milliseconds in 1..=100 {
            metrics.record_frame(Duration::from_secs_f64(milliseconds as f64 / 1_000.0));
        }
        metrics.record_remesh_latency(Duration::from_millis(42));
        metrics.add_decode_errors(3);
        metrics.record_pipeline_snapshot(PipelineMetricsSnapshot {
            max_decode: Duration::from_millis(11),
            max_mesh: Duration::from_millis(22),
            max_remesh: Duration::from_millis(42),
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
        assert_eq!(report.p50_frame_ms, 51.0);
        assert_eq!(report.p95_frame_ms, 96.0);
        assert_eq!(report.p99_frame_ms, 100.0);
        assert_eq!(report.max_frame_ms, 100.0);
        assert_eq!(report.max_decode_ms, 11.0);
        assert_eq!(report.max_mesh_ms, 22.0);
        assert_eq!(report.max_remesh_ms, 42.0);
        assert_eq!(report.decode_error_count, 3);
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
    fn json_output_is_pretty_deterministic_and_newline_terminated() {
        let report = MetricsReport {
            session_seconds: 15.0,
            frame_count: 2,
            p50_frame_ms: 4.0,
            p95_frame_ms: 5.0,
            p99_frame_ms: 5.0,
            max_frame_ms: 5.0,
            max_decode_ms: 6.0,
            max_mesh_ms: 7.0,
            max_remesh_ms: 20.0,
            decode_error_count: 0,
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
                "  \"frame_count\": 2,\n",
                "  \"p50_frame_ms\": 4.0,\n",
                "  \"p95_frame_ms\": 5.0,\n",
                "  \"p99_frame_ms\": 5.0,\n",
                "  \"max_frame_ms\": 5.0,\n",
                "  \"max_decode_ms\": 6.0,\n",
                "  \"max_mesh_ms\": 7.0,\n",
                "  \"max_remesh_ms\": 20.0,\n",
                "  \"decode_error_count\": 0,\n",
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
                "  \"gpu_upload_bytes\": 4096\n",
                "}\n",
            )
        );
        let _ = fs::remove_dir_all(directory);
    }
}
