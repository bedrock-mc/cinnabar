use super::*;

#[test]
fn rolling_fps_uses_only_the_most_recent_second() {
    let mut fps = RollingFps::default();
    for _ in 0..60 {
        fps.record(Duration::from_secs_f64(1.0 / 60.0));
    }
    assert!((fps.value() - 60.0).abs() < 0.01);

    for _ in 0..30 {
        fps.record(Duration::from_secs_f64(1.0 / 30.0));
    }
    assert!((fps.value() - 30.0).abs() < 0.01);
}

#[test]
fn diagnostic_telemetry_refreshes_only_after_resident_attribution_changes() {
    let key = SubChunkKey::new(0, 1, 2, 3);
    let mut tracker = DiagnosticQuadTracker::default();
    let mut metrics = MetricsCollector::new();
    let mut revision = tracker.revision();

    assert!(refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none());
    tracker.upsert(
        key,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );
    let marker = refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics)
        .expect("changed diagnostic residency emits one marker");
    assert!(marker.contains("diagnostic_attribution_top=54|0x200a28f1|minecraft:leaf_litter|6"));
    assert!(
        refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none(),
        "unchanged frames must not rebuild or re-emit diagnostic attribution"
    );
    let unchanged_revision = tracker.revision();
    tracker.upsert(
        key,
        DiagnosticGeometrySummary::from_counts([DiagnosticGeometryCount::new(
            Some(54),
            537_536_753,
            6,
        )]),
    );
    assert_eq!(tracker.revision(), unchanged_revision);
    assert!(
        refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics).is_none(),
        "an identical remesh must not increment revision or re-emit telemetry"
    );
    tracker.remove(key);
    let cleared = refresh_diagnostic_attribution(&mut revision, &tracker, &mut metrics)
        .expect("eviction publishes the cleared resident state");
    assert!(cleared.contains("diagnostic_attribution_total=0"));
}

#[test]
fn cumulative_counter_delta_tolerates_a_counter_reset() {
    assert_eq!(cumulative_counter_delta(9, 4), 5);
    assert_eq!(cumulative_counter_delta(2, 9), 2);
}
