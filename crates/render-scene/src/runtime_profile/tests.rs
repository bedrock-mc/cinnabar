use super::*;

/// Extraction and open timers keep frame N even after the main thread admits N+1.
#[test]
fn overlapping_main_and_render_spans_keep_their_input_identity() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("identity.json");
    let profiler = RuntimeStageProfiler::for_gameplay(true, Some(path.clone()));
    profiler.begin_frame(true, false);
    profiler.trace_frame(true, false, 0.0);
    let render = profiler.for_frame();
    let timer = profiler.time(RuntimeStage::WorldStream);
    profiler.begin_frame(true, false);
    profiler.trace_frame(true, false, 0.0);
    assert_eq!(profiler.frame_id(), 2);
    assert_eq!(render.frame_id(), 1);
    drop(timer);
    render.record(
        RuntimeStage::RenderSubmission,
        Instant::now(),
        Duration::from_millis(3),
    );
    profiler.record(
        RuntimeStage::UiPublication,
        Instant::now(),
        Duration::from_millis(1),
    );
    profiler.flush_trace();
    profiler.wait_for_trace_export();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for (stage, frame_id) in [
        ("world_stream", 1),
        ("render_submission", 1),
        ("ui_publication", 2),
    ] {
        let event = saved["traceEvents"]
            .as_array()
            .unwrap()
            .iter()
            .find(|event| event["name"] == stage)
            .unwrap();
        assert_eq!(event["args"]["frame_id"], frame_id);
    }
}

#[test]
fn stage_table_indices_match_and_gpu_stages_form_the_tail() {
    for (index, stage) in RuntimeStage::ALL.into_iter().enumerate() {
        assert_eq!(stage as usize, index);
    }
    let tail = &RuntimeStage::ALL[RuntimeStage::ALL.len() - RuntimeStage::GPU.len()..];
    assert_eq!(tail, RuntimeStage::GPU);
    assert!(
        RuntimeStage::GPU
            .iter()
            .all(|stage| stage.name().starts_with("gpu_"))
    );
    assert_eq!(RuntimeStage::RenderFrame.gpu_index(), None);
}

/// Owned cull passes retain their names and durations through decoding and profiling.
#[test]
fn gpu_cull_passes_feed_stage_profiles() {
    let stages = [
        "gpu_cull_early",
        "gpu_cull_hiz",
        "gpu_cull_late",
        "gpu_terrain_late",
    ]
    .map(|name| {
        RuntimeStage::GPU
            .into_iter()
            .find(|stage| stage.name() == name)
            .unwrap_or_else(|| panic!("missing GPU stage: {name}"))
    });
    let frame = crate::gpu_times::decode_spans(
        stages.into_iter().enumerate().map(|(index, stage)| {
            let begin = 100 * (index as u64 + 1);
            (stage, begin, begin + 10 * (index as u64 + 1))
        }),
        2.0,
        true,
    );
    let profiler = RuntimeStageProfiler::for_gameplay(true, None);
    profiler.record_gpu_frame(&frame);
    assert_eq!(profiler.latest_pass_frame(), Some(frame));
    let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
    for (index, stage) in stages.into_iter().enumerate() {
        let duration = Duration::from_nanos(20 * (index as u64 + 1));
        assert_eq!(frame.get(stage), Some(duration));
        assert!(frame.iter().any(|sample| sample == (stage, duration)));
        let sample = snapshot.samples[stage as usize];
        assert_eq!(sample.count, 1);
        assert_eq!(sample.total, duration);
    }
    assert_eq!(
        frame.get(RuntimeStage::GpuFrame),
        Some(Duration::from_nanos(680))
    );
}

#[test]
fn gpu_frames_feed_aggregates_and_the_latest_snapshot() {
    let profiler = RuntimeStageProfiler::for_gameplay(true, None);
    assert_eq!(profiler.latest_pass_frame(), None);
    let frame = crate::gpu_times::decode_spans([(RuntimeStage::GpuOpaque, 10, 30)], 1.0, true);
    profiler.record_gpu_frame(&frame);
    assert_eq!(profiler.latest_pass_frame(), Some(frame));
    // A whole-frame span updates aggregates but keeps the last per-pass breakdown.
    let whole = crate::gpu_times::decode_spans([(RuntimeStage::GpuFrame, 10, 50)], 1.0, true);
    profiler.record_gpu_frame(&whole);
    assert_eq!(profiler.latest_pass_frame(), Some(frame));
    let latency = crate::GpuFrameTimes::completion_latency(Duration::from_millis(100));
    profiler.record_gpu_frame(&latency);
    assert_eq!(profiler.latest_pass_frame(), Some(frame));
    let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
    assert_eq!(snapshot.samples[RuntimeStage::GpuOpaque as usize].count, 1);
    assert_eq!(snapshot.samples[RuntimeStage::GpuFrame as usize].count, 2);
    assert_eq!(
        snapshot.samples[RuntimeStage::GpuFrameCompletionLatency as usize].count,
        1
    );
    assert_eq!(latency.get(RuntimeStage::GpuFrame), None);
    assert_eq!(profiler.slow_frame_counts().unwrap().gpu, 0);
    assert_eq!(profiler.slow_frame_counts().unwrap().slow, 0);
}

#[test]
fn every_gpu_overrun_is_marked_in_the_trace() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("trace.json");
    let profiler = RuntimeStageProfiler::for_gameplay(true, Some(path.clone()));
    profiler.set_frame_interval(Duration::from_secs_f64(1.0 / 120.0));
    let over = crate::gpu_times::decode_spans([(RuntimeStage::GpuOpaque, 1, 7_000_001)], 1.0, true);
    profiler.record_gpu_frame(&over);
    profiler.record_gpu_frame(&over);
    profiler.flush_trace();
    profiler.wait_for_trace_export();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let markers = saved["traceEvents"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["name"] == "slow_frame")
        .count();
    assert_eq!(markers, 2);
    assert_eq!(profiler.slow_frame_counts().unwrap().gpu, 2);
}

/// Summed worker polls remain available as diagnostics without becoming a fictitious
/// execution span on the frame thread that receives them.
#[test]
fn background_work_exports_a_counter_while_reclaim_stays_a_cpu_span() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("background.json");
    let profiler = RuntimeStageProfiler::for_gameplay(true, Some(path.clone()));
    profiler.record_background_sample(RuntimeStage::WorldService, Duration::from_millis(3));
    profiler.record(
        RuntimeStage::WorldServiceReclaim,
        Instant::now(),
        Duration::from_millis(1),
    );
    let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
    let background_sample = snapshot.samples[RuntimeStage::WorldService as usize];
    assert_eq!(background_sample.count, 1);
    assert_eq!(background_sample.total, Duration::from_millis(3));
    profiler.flush_trace();
    profiler.wait_for_trace_export();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let events = saved["traceEvents"].as_array().unwrap();
    assert_eq!(events.len(), 2);
    let background = &events[0];
    assert_eq!(background["name"], "world_service");
    assert_eq!(background["ph"], "C");
    assert_eq!(background["dur"], 0.0);
    assert_eq!(background["cat"], "background_work");
    assert_eq!(background["timestamp_kind"], "sample_received");
    assert_eq!(background["args"]["duration_ns"], 3_000_000);
    let reclaim = &events[1];
    assert_eq!(reclaim["name"], "world_service_reclaim");
    assert_eq!(reclaim["ph"], "X");
    assert_eq!(reclaim["dur"], 1000.0);
}

#[test]
fn render_frame_excludes_only_the_acquisition_wait() {
    let started = Instant::now();
    let now = started + Duration::from_millis(80);
    let elapsed = render_frame_elapsed(started, now, Duration::from_millis(50));
    assert_eq!(elapsed, Duration::from_millis(30));
    assert_eq!(
        render_frame_elapsed(started, now, Duration::from_millis(90)),
        Duration::ZERO
    );
}

#[test]
fn disabled_profiler_records_nothing() {
    let profiler = RuntimeStageProfiler::new(false);
    drop(profiler.time(RuntimeStage::WorldStream));
    assert_eq!(profiler.take_snapshot_if_due(Duration::ZERO), None);
}

#[test]
fn snapshot_drains_each_stage_interval() {
    let profiler = RuntimeStageProfiler::new(true);
    drop(profiler.time(RuntimeStage::WorldStream));
    let first = profiler
        .take_snapshot_if_due(Duration::ZERO)
        .expect("enabled profiler emits a due snapshot");
    assert_eq!(first.samples[RuntimeStage::WorldStream as usize].count, 1);

    let second = profiler
        .take_snapshot_if_due(Duration::ZERO)
        .expect("zero interval permits another snapshot");
    assert_eq!(second.samples[RuntimeStage::WorldStream as usize].count, 0);
}

#[test]
fn concurrent_records_are_drained_as_complete_samples() {
    let profiler = RuntimeStageProfiler::new(true);
    let workers = (0..4)
        .map(|_| {
            let profiler = profiler.clone();
            std::thread::spawn(move || {
                for _ in 0..1_000 {
                    drop(profiler.time(RuntimeStage::TransparentWorker));
                }
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().unwrap();
    }

    let snapshot = profiler
        .take_snapshot_if_due(Duration::ZERO)
        .expect("enabled profiler emits a due snapshot");
    assert_eq!(
        snapshot.samples[RuntimeStage::TransparentWorker as usize].count,
        4_000
    );
}

/// Metal frame spans count against the GPU budget; their callback latency never does.
#[test]
fn metal_frame_spans_feed_the_gpu_budget_without_latency() {
    const BEGIN: u64 = 90_000_000_000_000;
    let profiler = RuntimeStageProfiler::for_gameplay(true, None);
    profiler.set_frame_interval(Duration::from_secs_f64(1.0 / 120.0));
    let latency = crate::GpuFrameTimes::completion_latency(Duration::from_millis(100));
    let frame = |millis: u64| latency.with_frame_span(BEGIN, BEGIN + millis * 1_000_000);
    assert_eq!(
        frame(2).get(RuntimeStage::GpuFrame),
        Some(Duration::from_millis(2))
    );
    profiler.record_gpu_frame(&frame(2));
    assert_eq!(profiler.slow_frame_counts().unwrap().gpu, 0);
    profiler.record_gpu_frame(&frame(20));
    assert_eq!(profiler.slow_frame_counts().unwrap().gpu, 1);
    // Missing or reversed host times keep only the latency.
    for invalid in [
        latency.with_frame_span(0, BEGIN),
        latency.with_frame_span(BEGIN, BEGIN - 1),
    ] {
        assert_eq!(invalid, latency);
    }
    let snapshot = profiler.take_snapshot_if_due(Duration::ZERO).unwrap();
    assert_eq!(snapshot.samples[RuntimeStage::GpuFrame as usize].count, 2);
    assert_eq!(
        snapshot.samples[RuntimeStage::GpuFrameCompletionLatency as usize].count,
        2
    );
}
