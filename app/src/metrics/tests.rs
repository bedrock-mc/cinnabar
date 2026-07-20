use super::{
    AssetMetrics, DiagnosticQuadTracker, ExactFullViewProof, GpuPassMeasurement, GpuPassSample,
    MetricsCollector, MetricsReport, ModelWorkloadCountSnapshot, ModelWorkloadMetricsSnapshot,
    PipelineMetricsSnapshot, TeleportProof, TransparentSortMetricsSnapshot,
    deterministic_manifest_hash, pair_gpu_pass_sample, percentile,
};
use meshing::{DiagnosticGeometryCount, DiagnosticGeometrySummary};
use std::{fs, time::Duration};
use world::SubChunkKey;

#[test]
fn empty_percentiles_are_zero() {
    assert_eq!(percentile(&[], 0.99), 0.0);
}

#[test]
fn globally_folded_identity_removal_cannot_subtract_a_later_tracked_contribution() {
    let tracked_first = SubChunkKey::new(0, 0, 0, 0);
    let folded_first = SubChunkKey::new(0, 1, 0, 0);
    let tracked_later = SubChunkKey::new(0, 2, 0, 0);
    let summary = |sequential_id, quad_count| {
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(sequential_id),
            sequential_id,
            quad_count,
        )])
    };
    let mut tracker = DiagnosticQuadTracker::with_identity_capacity(1);

    tracker.upsert(tracked_first, summary(1, 1));
    tracker.upsert(folded_first, summary(2, 5));
    tracker.remove(tracked_first);
    tracker.upsert(tracked_later, summary(2, 7));
    tracker.remove(folded_first);

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.total_quad_count, 7);
    assert_eq!(snapshot.top.len(), 1);
    assert_eq!(snapshot.top[0].sequential_id, Some(2));
    assert_eq!(snapshot.top[0].quad_count, 7);
    assert_eq!(snapshot.omitted_identity_count, 0);
    assert_eq!(snapshot.omitted_quad_count, 0);
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
        pair_gpu_pass_sample(Some(time), Some(GpuPassMeasurement::new(time, 1.0)), None,).is_none()
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
fn phase2_warmup_frames_are_excluded_from_the_report_histogram() {
    let mut metrics = MetricsCollector::with_asset_metrics_window(
        AssetMetrics::default(),
        Duration::from_secs(30),
        Duration::from_millis(1_200),
    );
    for _ in 0..30 {
        metrics.record_frame(Duration::from_secs(1));
    }
    assert_eq!(metrics.frame_count(), 0);
    for _ in 0..120 {
        metrics.record_frame(Duration::from_millis(10));
    }
    metrics.record_frame(Duration::from_secs(1));
    let report = metrics.report();
    assert_eq!(report.frame_count, 120);
    assert_eq!(report.p95_frame_ms, 10.0);
    assert_eq!(report.p99_frame_ms, 10.0);
    assert_eq!(report.max_frame_ms, 10.0);
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
            "    \"diagnostic_quad_count\": 0,\n",
            "    \"diagnostic_attribution\": {\n",
            "      \"total_quad_count\": 0,\n",
            "      \"top\": [],\n",
            "      \"omitted_identity_count\": 0,\n",
            "      \"omitted_quad_count\": 0\n",
            "    }\n",
            "  }\n",
            "}\n",
        )
    );
    let _ = fs::remove_dir_all(directory);
}
