use super::*;

/// Missing frame boundaries cannot grow pending windows or hide dropped requests.
#[test]
fn pending_hitches_stay_bounded_without_frame_markers() {
    let trace = FrameTrace::build(None, Instant::now(), 16);
    let _ = std::thread::current().id();
    let (_, allocations) = test_alloc::count(|| {
        for frame_id in 1..=1000 {
            trace.slow_frame(SlowFrameEvent {
                frame_id,
                hard_hitch: true,
                reasons: 1,
                frame: Duration::from_millis(40),
            });
        }
    });
    assert_eq!(allocations, 0);
    {
        let window = trace.events.lock().unwrap();
        assert_eq!(window.pending.len(), MAX_PENDING_HITCHES);
        assert_eq!(window.dropped_hitches, 1000 - MAX_PENDING_HITCHES as u64);
    }
    let reports = trace.take_hitches(None);
    assert_eq!(reports.len(), MAX_PENDING_HITCHES);
    for report in reports {
        assert_eq!(
            report.snapshot.json()["dropped_hitch_windows"],
            1000 - MAX_PENDING_HITCHES
        );
        assert!(!report.post_complete);
    }
}

/// Wraparound changes retained history without allocating or copying the ring's storage.
#[test]
fn rolling_records_allocate_nothing_after_startup() {
    let trace = FrameTrace::build(None, Instant::now(), 16);
    let started = Instant::now();
    let (_, allocations) = test_alloc::count(|| {
        for frame in 1..=10_000 {
            trace.record(
                frame,
                RuntimeStage::WorldServiceReclaim,
                started,
                Duration::from_millis(40),
            );
        }
    });
    assert_eq!(allocations, 0);
    let window = trace.events.lock().unwrap();
    assert_eq!(window.events.capacity(), 16);
    assert_eq!(window.events.len(), 16);
    assert_eq!(window.overwritten(), 9984);
    assert_eq!(window.events.front().unwrap().frame_id, Some(9985));
    assert_eq!(window.events.back().unwrap().frame_id, Some(10_000));
}

/// Consecutive hard hitches each retain leading context and two following completed frames.
#[test]
fn every_late_hitch_gets_a_window_even_when_they_overlap() {
    let trace = FrameTrace::build(None, Instant::now(), 32);
    let mut reports = Vec::new();
    for frame_id in 1..=1010 {
        if (1001..=1004).contains(&frame_id) {
            trace.slow_frame(SlowFrameEvent {
                frame_id: frame_id - 1,
                hard_hitch: true,
                reasons: 1,
                frame: Duration::from_millis(40),
            });
        }
        reports.extend(trace.frame(frame_id, true, false, 0.0));
        trace.record(
            frame_id,
            RuntimeStage::RenderSubmission,
            Instant::now(),
            Duration::from_millis(2),
        );
    }
    assert_eq!(reports.len(), 4);
    for (index, report) in reports.into_iter().enumerate() {
        assert!(trace.events.try_lock().is_ok(), "report owns its samples");
        let text = report.to_string();
        let saved: serde_json::Value =
            serde_json::from_str(text.strip_prefix("CINNABAR_HITCH ").unwrap()).unwrap();
        assert_eq!(saved["hitch"]["frame_id"], 1000 + index);
        assert_eq!(saved["hitch"]["complete"], true);
        assert!(saved["overwritten_events"].as_u64().unwrap() > 0);
        let frames = saved["traceEvents"].as_array().unwrap().iter();
        let earliest = frames.filter_map(|event| event["frame_id"].as_u64()).min();
        assert_eq!(earliest, Some(1000 + index as u64 - HITCH_PRE_FRAMES));
        assert!(
            saved["traceEvents"]
                .as_array()
                .unwrap()
                .iter()
                .any(|event| {
                    event["name"] == "render_submission"
                        && event["args"]["frame_id"] == 1000 + index
                })
        );
    }
    assert!(trace.take_hitches(None).is_empty());
}

/// Startup, eviction and shutdown never claim a full pre/post window.
#[test]
fn incomplete_hitch_windows_explain_missing_context() {
    let trace = FrameTrace::build(None, Instant::now(), 2);
    trace.frame(1, true, false, 0.0);
    trace.slow_frame(SlowFrameEvent {
        frame_id: 1,
        hard_hitch: true,
        reasons: 1,
        frame: Duration::from_millis(40),
    });
    trace.record(2, RuntimeStage::WorldStream, Instant::now(), Duration::ZERO);
    trace.record(
        2,
        RuntimeStage::RenderSubmission,
        Instant::now(),
        Duration::ZERO,
    );
    let reports = trace.take_hitches(None);
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert!(!report.pre_complete);
    assert!(!report.post_complete);
    assert!(!report.hitch_retained);
}

/// A large explicit trace cannot enlarge each queued hitch report or hide omitted history.
#[test]
fn hitch_reports_bound_snapshots_separately_from_export_capacity() {
    let trace = FrameTrace::build(None, Instant::now(), GAMEPLAY_CAPACITY * 2);
    for id in 1..=5000 {
        trace.frame(id, true, false, 0.0);
    }
    trace.slow_frame(SlowFrameEvent {
        frame_id: 5000,
        hard_hitch: true,
        reasons: 1,
        frame: Duration::from_millis(40),
    });
    trace.frame(5001, true, false, 0.0);
    trace.frame(5002, true, false, 0.0);
    let reports = trace.frame(5003, true, false, 0.0);
    assert_eq!(reports.len(), 1);
    let snapshot = reports[0].snapshot.json();
    // The window's frame starts, 4996 through the 5003 boundary, plus the slow-frame marker.
    assert_eq!(snapshot["retained_events"], 9);
    assert_eq!(snapshot["overwritten_events"], 0);
    assert!(snapshot["omitted_events"].as_u64().unwrap() > 0);
    assert_eq!(snapshot["truncated"], true);
    assert!(reports[0].pre_complete && reports[0].post_complete);
}

/// A machine that hitches every frame logs a bounded number of reports and counts the rest.
#[test]
fn constant_hitching_cannot_flood_the_log() {
    let trace = FrameTrace::build(None, Instant::now(), GAMEPLAY_CAPACITY);
    let frames = 10 * HITCH_REPORT_REFILL_FRAMES;
    let mut reports = Vec::new();
    for frame_id in 1..=frames {
        trace.slow_frame(SlowFrameEvent {
            frame_id,
            hard_hitch: true,
            reasons: 1,
            frame: Duration::from_millis(40),
        });
        reports.extend(trace.frame(frame_id + 1, true, false, 0.0));
    }
    let allowed = HITCH_REPORT_BURST as usize + (frames / HITCH_REPORT_REFILL_FRAMES) as usize;
    assert!(reports.len() <= allowed, "{} reports", reports.len());
    assert!(
        reports.len() > HITCH_REPORT_BURST as usize,
        "the budget refills"
    );
    let later = reports.last().unwrap().snapshot.json();
    assert!(later["suppressed_hitch_reports"].as_u64().unwrap() > 0);
    assert!(
        later["retained_events"].as_u64().unwrap() < 32,
        "a report holds only its window"
    );
}

/// A late-session stage must survive after the recording budget has been filled.
#[test]
fn late_stage_survives_a_full_trace() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("late.json");
    let trace = FrameTrace::with_capacity(path.clone(), Instant::now(), 2);
    for _ in 0..100 {
        trace.frame(1, false, false, 0.0);
    }
    trace.record(
        1,
        RuntimeStage::WorldServiceReclaim,
        Instant::now(),
        Duration::from_millis(40),
    );
    trace.flush();
    trace.wait_for_export();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(
        saved["traceEvents"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["name"] == "world_service_reclaim")
    );
}

/// Saved spans keep focus, thread identity and slow counters with no eviction.
#[test]
fn trace_preserves_thread_spans_and_focus_without_growing() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("trace.json");
    let trace = FrameTrace::with_capacity(path.clone(), Instant::now(), 3);
    trace.frame(1, false, false, 1.5);
    trace.slow_frame(SlowFrameEvent {
        frame_id: 1,
        hard_hitch: false,
        reasons: 0b1001,
        frame: Duration::from_millis(12),
    });
    trace.record(
        1,
        RuntimeStage::WorldStream,
        Instant::now(),
        Duration::from_millis(2),
    );
    trace.flush();
    trace.wait_for_export();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(saved["truncated"], false);
    assert_eq!(saved["traceEvents"][0]["args"]["game_seconds"], 1.5);
    assert_eq!(
        saved["traceEvents"][1]["args"]["violations"],
        "interval+gpu"
    );
    assert_eq!(saved["traceEvents"][2]["dur"], 2000.0);
    assert_eq!(
        saved["traceEvents"][0]["tid"],
        saved["traceEvents"][2]["tid"]
    );
}

#[test]
fn recording_capacity_is_bounded_and_never_grows() {
    assert_eq!(trace_capacity(Some("0")), TRACE_CAPACITY);
    assert_eq!(trace_capacity(Some("invalid")), TRACE_CAPACITY);
    assert_eq!(
        trace_capacity(Some(&(MAX_TRACE_CAPACITY + 1).to_string())),
        TRACE_CAPACITY
    );
    assert_eq!(
        trace_capacity(Some(&MAX_TRACE_CAPACITY.to_string())),
        MAX_TRACE_CAPACITY
    );
    let root = tempfile::tempdir().unwrap();
    let trace = FrameTrace::with_capacity(root.path().join("bounded.json"), Instant::now(), 2);
    for _ in 0..2 {
        trace.frame(1, false, false, 0.0);
    }
    assert_eq!(trace.events.lock().unwrap().overwritten(), 0);
    trace.frame(1, false, false, 0.0);
    let window = trace.events.lock().unwrap();
    let events = &window.events;
    assert_eq!(events.capacity(), 2);
    assert_eq!(events.len(), 2);
    assert_eq!(window.overwritten(), 1);
}

#[test]
fn gpu_samples_share_receipt_time_without_growing_or_faking_cpu_spans() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("gpu.json");
    let trace = FrameTrace::with_capacity(path.clone(), Instant::now(), 6);
    let frame = crate::gpu_times::decode_spans(
        [
            (RuntimeStage::GpuUi, 10, 30),
            (RuntimeStage::GpuHand, 40, 70),
        ],
        2.0,
        true,
    );
    trace.gpu_frame(&frame);
    trace.gpu_frame(&frame);
    trace.gpu_frame(&frame);
    {
        let window = trace.events.lock().unwrap();
        let events = &window.events;
        assert_eq!(events.len(), 6);
        assert_eq!(events.capacity(), 6);
        assert_eq!(window.overwritten(), 3);
    }
    trace.flush();
    trace.wait_for_export();
    let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let samples = saved["traceEvents"].as_array().unwrap();
    for frame in samples.as_chunks::<3>().0 {
        assert!(frame.iter().all(|sample| sample["ts"] == frame[0]["ts"]));
        assert!(frame.iter().all(|sample| sample["ph"] == "C"));
        assert!(frame.iter().all(|sample| {
            sample["args"]
                .as_object()
                .unwrap()
                .values()
                .all(serde_json::Value::is_number)
        }));
        assert!(frame.iter().all(|sample| sample["frame_id"].is_null()));
        assert!(
            frame
                .iter()
                .all(|sample| sample["event_sequence"].as_u64().is_some())
        );
        assert!(frame.iter().all(|sample| sample["dur"] == 0.0));
        assert!(
            frame
                .iter()
                .all(|sample| sample["timestamp_kind"] == "readback_received")
        );
    }
    assert_eq!(samples[0]["name"], "gpu_frame");
    assert_eq!(samples[0]["args"]["duration_ns"], 120);
    assert_eq!(samples[1]["args"]["duration_ns"], 40);
    assert_eq!(samples[2]["args"]["duration_ns"], 60);
    assert_eq!(samples[0]["args"]["sample_sequence"], 2);
    assert_eq!(samples[3]["args"]["sample_sequence"], 3);
}

/// Exporting a full ring cannot allocate a second event collection on the caller.
#[test]
fn full_export_does_not_allocate_per_event_on_the_caller() {
    let root = tempfile::tempdir().unwrap();
    let trace = FrameTrace::with_capacity(root.path().join("full.json"), Instant::now(), 4096);
    for frame_id in 1..=4096 {
        trace.frame(frame_id, true, false, 0.0);
    }
    let (_, allocations) = test_alloc::measure(|| trace.flush());
    assert!(
        allocations.bytes < 64 * 1024,
        "export caller allocated {} bytes",
        allocations.bytes
    );
    trace.record(
        5000,
        RuntimeStage::ChunkExtraction,
        Instant::now(),
        Duration::ZERO,
    );
    trace.flush();
    trace.wait_for_export();
    let saved: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("full.json")).unwrap()).unwrap();
    assert_eq!(saved["retained_events"], 4096);
    assert_eq!(saved["traceEvents"].as_array().unwrap().len(), 4096);
    assert!(trace.events.lock().unwrap().events.is_empty());
}

/// A writer failure near the start must not serialize or allocate the remaining events.
#[test]
fn export_serializes_events_only_as_the_writer_consumes_them() {
    /// Rejects output after a short prefix, like a full disk or closed pipe.
    struct PrefixWriter(usize);
    impl std::io::Write for PrefixWriter {
        /// Writes only the permitted prefix before reporting an I/O failure.
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let count = bytes.len().min(self.0);
            if count == 0 {
                return Err(std::io::ErrorKind::StorageFull.into());
            }
            self.0 -= count;
            Ok(count)
        }
        /// Has no buffered bytes to commit.
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let trace = FrameTrace::build(None, Instant::now(), 4096);
    for frame_id in 1..=4096 {
        trace.frame(frame_id, true, false, 0.0);
    }
    let snapshot = trace.snapshot(&trace.events.lock().unwrap(), usize::MAX, None);
    let (result, allocations) =
        test_alloc::measure(|| serde_json::to_writer(PrefixWriter(1024), &snapshot));
    assert!(result.is_err());
    assert!(
        allocations.bytes < 64 * 1024,
        "stream prefix allocated {} bytes",
        allocations.bytes
    );
}
