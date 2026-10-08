use super::{AssetMetrics, MetricsCollector};
use std::time::{Duration, Instant};

#[test]
fn fps_uses_the_recorded_window_and_the_slowest_mean_instead_of_reciprocal_p99() {
    let mut metrics = MetricsCollector::new();
    for _ in 0..198 {
        metrics.record_frame(Duration::from_millis(10));
    }
    metrics.record_frame(Duration::from_millis(40));
    metrics.record_frame(Duration::from_millis(80));
    let report = metrics.report();
    assert_eq!(report.frame_sample_seconds, 2.1);
    assert_eq!(report.average_fps, 200.0 / 2.1);
    let low = report.one_percent_low_fps.unwrap();
    assert_eq!(low.sample_count, 2);
    let expected = 1_000.0 / 60.0;
    assert!(low.lower <= expected && expected <= low.upper);
    assert!(low.upper - low.lower < 0.02);
    assert_ne!(low.lower, 1_000.0 / report.p99_frame_ms);
}

#[test]
fn fps_bounds_cover_partial_buckets_and_multi_second_hitches() {
    for slow in [[10_010, 10_090, 10_030], [3_000_000, 8_000_000, 5_000_000]] {
        let mut metrics = MetricsCollector::new();
        for _ in 0..197 {
            metrics.record_frame(Duration::from_micros(10));
        }
        for micros in slow {
            metrics.record_frame(Duration::from_micros(micros));
        }
        let low = metrics.report().one_percent_low_fps.unwrap();
        assert_eq!(low.sample_count, 2);
        let tail_sum = slow.iter().sum::<u64>() - slow.iter().min().unwrap();
        let expected = 2_000_000.0 / tail_sum as f64;
        assert!(low.lower <= expected && expected <= low.upper, "{low:?}");
    }
    let mut metrics = MetricsCollector::new();
    for micros in [10_010, 10_090, 10_030] {
        metrics.record_frame(Duration::from_micros(micros));
    }
    let low = metrics.report().one_percent_low_fps.unwrap();
    assert_eq!(low.sample_count, 1);
    assert!((low.lower - 1_000_000.0 / 10_090.0).abs() < 1e-12);
    assert_eq!(low.upper, low.lower);
}

#[test]
fn fps_bounds_contain_an_independently_sorted_sample() {
    for count in [1_usize, 99, 100, 101, 199, 200, 201, 1000] {
        let mut metrics = MetricsCollector::new();
        let mut micros = (0..count)
            .map(|i| ((i * 104729) % 100_003) as u64)
            .collect::<Vec<_>>();
        for &duration in &micros {
            metrics.record_frame(Duration::from_micros(duration));
        }
        micros.sort_unstable_by(|left, right| right.cmp(left));
        let tail_count = count.div_ceil(100);
        let tail_sum = micros[..tail_count].iter().sum::<u64>();
        if tail_sum == 0 {
            assert_eq!(metrics.report().one_percent_low_fps, None);
            continue;
        }
        let expected = tail_count as f64 * 1_000_000.0 / tail_sum as f64;
        let low = metrics.report().one_percent_low_fps.unwrap();
        assert_eq!(low.sample_count as usize, tail_count);
        assert!(low.lower <= expected + 1e-12 && expected <= low.upper + 1e-12);
    }
}

#[test]
fn fps_respects_warmup_freezing_and_timed_session_reset() {
    let mut metrics = MetricsCollector::with_asset_metrics_window(
        AssetMetrics::default(),
        Duration::from_secs(1),
        Duration::from_secs(1),
    );
    metrics.record_frame(Duration::from_secs(1));
    assert_eq!(metrics.report().average_fps, 0.0);
    assert_eq!(metrics.report().one_percent_low_fps, None);
    for _ in 0..100 {
        metrics.record_frame(Duration::from_millis(10));
    }
    let before = metrics.report();
    metrics.record_frame(Duration::from_secs(10));
    assert_eq!(metrics.report().average_fps, before.average_fps);
    assert_eq!(
        metrics.report().one_percent_low_fps,
        before.one_percent_low_fps
    );
    metrics.begin_timed_session(Instant::now());
    assert_eq!(metrics.report().average_fps, 0.0);
    assert_eq!(metrics.report().one_percent_low_fps, None);
    metrics.record_frame(Duration::ZERO);
    assert!(serde_json::to_string(&metrics.report()).is_ok());
}
