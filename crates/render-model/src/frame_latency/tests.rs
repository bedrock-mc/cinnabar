use super::*;

/// Supplies one frame on a deterministic caller-owned timeline.
fn frame(
    policy: &mut FrameLatencyPolicy,
    id: u64,
    at: Duration,
    cadence: Option<Duration>,
    work: Option<(u64, u64)>,
) -> u32 {
    policy
        .update(
            at,
            FrameRate::from_hz(60),
            cadence,
            work.map(|(cpu, gpu)| FrameWorkSample {
                frame: id,
                at,
                render: Duration::from_millis(cpu),
                gpu: Duration::from_millis(gpu),
            }),
        )
        .get()
}

/// Supplies sustained fitting work and returns the time of the first depth-one probe.
fn qualify(policy: &mut FrameLatencyPolicy) -> Duration {
    for id in 1..=121 {
        let at = Duration::from_micros(id * 16_667);
        assert_eq!(
            frame(policy, id, at, None, Some((4, 5))),
            if id == 121 { 1 } else { 2 }
        );
    }
    Duration::from_micros(121 * 16_667)
}

#[test]
fn latency_cadence_overload_escalates_even_when_estimated_work_is_light_or_missing() {
    for work in [None, Some((1, 1))] {
        let mut policy = FrameLatencyPolicy::default();
        let start = qualify(&mut policy);
        for id in 1..=8 {
            let interval = Duration::from_millis(33);
            let at = start + interval * id;
            assert_eq!(
                frame(&mut policy, 121 + u64::from(id), at, Some(interval), work),
                if id == 8 { 2 } else { 1 }
            );
        }
    }
}

#[test]
fn latency_cadence_work_pressure_alone_never_escalates() {
    let mut policy = FrameLatencyPolicy::default();
    let start = qualify(&mut policy);
    for id in 1..=200 {
        assert_eq!(
            frame(
                &mut policy,
                121 + u64::from(id),
                start + Duration::from_millis(17) * id,
                Some(Duration::from_millis(17)),
                Some((20, 20))
            ),
            1
        );
    }
}

#[test]
fn latency_cadence_failed_probes_double_the_cooldown_and_remain_bounded() {
    let mut policy = FrameLatencyPolicy::default();
    let mut now = qualify(&mut policy);
    let mut id = 122;
    let mut expected_cooldown = QUALIFICATION;
    for _ in 0..8 {
        let probe_started = now;
        for _ in 0..8 {
            now += Duration::from_millis(33);
            frame(
                &mut policy,
                id,
                now,
                Some(Duration::from_millis(33)),
                Some((1, 1)),
            );
            id += 1;
        }
        assert_eq!(policy.latency.get(), 2);
        assert!(now - probe_started < Duration::from_millis(300));
        expected_cooldown = expected_cooldown.saturating_mul(2).min(MAX_COOLDOWN);
        let next_allowed = now + expected_cooldown;
        // Even a continuously optimistic estimate may not trigger an early probe.
        while now < next_allowed {
            now = (now + Duration::from_millis(17)).min(next_allowed);
            let selected = frame(&mut policy, id, now, None, Some((1, 1)));
            assert_eq!(selected, if now == next_allowed { 1 } else { 2 });
            id += 1;
        }
    }
    assert_eq!(expected_cooldown, MAX_COOLDOWN);
}

#[test]
fn latency_cadence_threshold_jitter_and_single_hitches_do_not_flap() {
    let mut policy = FrameLatencyPolicy::default();
    let mut now = qualify(&mut policy);
    for id in 122..=800 {
        let interval = Duration::from_millis(if id == 122 {
            100
        } else if id % 2 == 0 {
            21
        } else {
            20
        });
        now += interval;
        assert_eq!(frame(&mut policy, id, now, Some(interval), Some((9, 9))), 1);
    }
    for id in 801..=808 {
        now += Duration::from_millis(33);
        frame(&mut policy, id, now, Some(Duration::from_millis(33)), None);
    }
    assert_eq!(policy.latency.get(), 2);
    for id in 809..=1500 {
        now += Duration::from_millis(17);
        assert_eq!(
            frame(
                &mut policy,
                id,
                now,
                None,
                Some((if id % 2 == 0 { 6 } else { 7 }, 7))
            ),
            2
        );
    }
}

#[test]
fn latency_cadence_incomplete_readbacks_do_not_reset_depth_or_headroom() {
    let mut policy = FrameLatencyPolicy::default();
    for id in 1..=242 {
        let at = Duration::from_micros(id * 16_667);
        frame(&mut policy, id, at, None, (id % 2 == 0).then_some((4, 5)));
    }
    assert_eq!(policy.latency.get(), 1);
    assert_eq!(
        policy
            .update(Duration::from_secs(8), FrameRate::from_hz(60), None, None)
            .get(),
        1
    );
}

#[test]
fn latency_cadence_duplicates_stale_and_old_depth_samples_cannot_start_probes() {
    let mut policy = FrameLatencyPolicy::default();
    for _ in 0..200 {
        assert_eq!(
            frame(
                &mut policy,
                1,
                Duration::from_millis(10),
                None,
                Some((1, 1))
            ),
            2
        );
    }
    let stale = FrameWorkSample {
        frame: 2,
        at: Duration::from_millis(10),
        ..Default::default()
    };
    for _ in 0..200 {
        assert_eq!(
            policy
                .update(
                    Duration::from_secs(5),
                    FrameRate::from_hz(60),
                    None,
                    Some(stale)
                )
                .get(),
            2
        );
    }
    // A telemetry gap expires headroom, rather than counting time without measurements.
    let mut policy = FrameLatencyPolicy::default();
    for id in 1..=120 {
        frame(
            &mut policy,
            id,
            Duration::from_micros(id * 16_667),
            None,
            Some((1, 1)),
        );
    }
    assert_eq!(
        frame(&mut policy, 121, Duration::from_secs(6), None, Some((1, 1))),
        2
    );
    let mut policy = FrameLatencyPolicy::default();
    qualify(&mut policy);
    assert_eq!(
        policy
            .update(Duration::from_secs(3), None, None, None)
            .get(),
        2
    );
}

#[test]
fn latency_cadence_fast_displays_require_elapsed_headroom_and_use_actual_refresh() {
    for (refresh, work_ms, expect_probe) in [(144, 1, true), (144, 5, false)] {
        let mut policy = FrameLatencyPolicy::default();
        for id in 1..=350 {
            let now = Duration::from_micros(id * 6_944);
            let selected = policy.update(
                now,
                FrameRate::from_hz(refresh),
                None,
                Some(FrameWorkSample {
                    frame: id,
                    at: now,
                    render: Duration::from_millis(work_ms),
                    gpu: Duration::from_millis(work_ms),
                }),
            );
            assert_eq!(
                selected.get(),
                if expect_probe && id >= 290 { 1 } else { 2 }
            );
        }
    }
}

#[test]
fn latency_cadence_sustained_recovery_can_probe_again_after_failed_estimate() {
    let mut policy = FrameLatencyPolicy::default();
    let mut now = qualify(&mut policy);
    for id in 122..130 {
        now += Duration::from_millis(33);
        frame(&mut policy, id, now, Some(Duration::from_millis(33)), None);
    }
    assert_eq!(policy.latency.get(), 2);
    for id in 130..=400 {
        now += Duration::from_millis(17);
        frame(
            &mut policy,
            id,
            now,
            Some(Duration::from_millis(17)),
            Some((4, 5)),
        );
    }
    assert_eq!(policy.latency.get(), 1);
    for id in 401..=800 {
        now += Duration::from_millis(17);
        assert_eq!(
            frame(&mut policy, id, now, Some(Duration::from_millis(17)), None),
            1
        );
    }
}
