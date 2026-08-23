use std::time::Duration;

use client_world::PublicationServiceConfig;

use crate::runtime::publication::{PublicationController, PublicationFrameWork};

#[test]
fn draining_backlog_stops_collapsing_caps_before_the_one_operation_trap() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    controller.finish_frame(PublicationFrameWork {
        upload_queue_items: 520,
        ..PublicationFrameWork::default()
    });
    controller.begin_frame(Duration::from_millis(125));
    assert_eq!(controller.budget().max_per_frame, 256);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);

    for items in (257..=519).rev() {
        controller.finish_frame(PublicationFrameWork {
            upload_queue_items: items,
            ..PublicationFrameWork::default()
        });
        controller.begin_frame(Duration::from_millis(125));
        assert!(
            controller.budget().max_per_frame >= 256,
            "a strictly draining backlog must never collapse the caps further"
        );
        assert_eq!(
            controller.diagnostics().multiplicative_decreases,
            1,
            "draining must not trigger additional multiplicative collapses"
        );
    }
    assert_eq!(config.maximum_frame_items, 512);
}

#[test]
fn stuck_backlog_keeps_collapsing_toward_the_pressure_floor_instead_of_one() {
    let mut controller = PublicationController::default();
    for frame in 0..24 {
        controller.finish_frame(PublicationFrameWork {
            upload_queue_items: client_world::MAX_PENDING_MESH_CHANGES + frame,
            ..PublicationFrameWork::default()
        });
        controller.begin_frame(Duration::from_millis(125));
    }

    assert_eq!(controller.budget().max_per_frame, 8);
    assert_eq!(controller.budget().max_zero_byte_operations_per_frame, 8);
    assert!(controller.diagnostics().multiplicative_decreases > 1);
}

#[test]
fn fifo_jitter_accrues_wall_clock_service_without_frame_count_bias() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    let jitter = [
        Duration::from_micros(15_800),
        Duration::from_micros(16_667),
        Duration::from_micros(17_900),
        Duration::from_micros(16_200),
    ];
    let mut serviced = 0_usize;
    for frame in 0..240 {
        controller.begin_frame(jitter[frame % jitter.len()]);
        let allowance = controller.allowance();
        while let Some(permit) = allowance.try_admit_payload(1) {
            serviced = serviced.saturating_add(1);
            assert!(permit.retire());
        }
        controller.finish_frame(PublicationFrameWork::default());
    }
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
    let elapsed_nanos = jitter
        .iter()
        .cycle()
        .take(240)
        .map(Duration::as_nanos)
        .sum::<u128>();
    let minimum = u128::from(config.minimum_items_per_second)
        .checked_mul(elapsed_nanos)
        .unwrap()
        / 1_000_000_000;
    assert!(u128::try_from(serviced).unwrap() >= minimum);
}
#[test]
fn slow_saturated_frame_without_gpu_backlog_preserves_service_caps() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    controller.begin_frame(Duration::from_millis(16));
    let allowance = controller.allowance();
    while let Some(permit) = allowance.try_admit_payload(1) {
        assert!(permit.retire());
    }
    controller.finish_frame(PublicationFrameWork {
        pending_mesh_jobs: 1,
        mesh_changes_published: controller.budget().max_per_frame,
        mesh_payloads_published: controller.budget().max_per_frame,
        ..PublicationFrameWork::default()
    });

    controller.begin_frame(Duration::from_millis(80));

    assert_eq!(
        controller.budget().max_per_frame,
        config.maximum_frame_items
    );
    assert_eq!(
        controller.budget().max_zero_byte_operations_per_frame,
        config.maximum_zero_byte_operations_per_frame
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
}

#[test]
fn zero_byte_saturation_without_gpu_backlog_preserves_service_caps() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    controller.begin_frame(Duration::from_millis(16));
    controller.finish_frame(PublicationFrameWork {
        mesh_changes_published: config.maximum_zero_byte_operations_per_frame,
        mesh_payloads_published: 0,
        mesh_bytes_published: 0,
        pending_mesh_jobs: 1,
        in_flight_mesh_jobs: 1,
        ..PublicationFrameWork::healthy()
    });

    controller.begin_frame(Duration::from_secs(3));

    assert_eq!(
        controller.budget().max_per_frame,
        config.maximum_frame_items
    );
    assert_eq!(
        controller.budget().max_zero_byte_operations_per_frame,
        config.maximum_zero_byte_operations_per_frame
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
}
#[test]
fn gpu_backlog_is_genuine_pressure_even_when_fifo_frame_time_is_healthy() {
    let mut controller = PublicationController::default();
    controller.finish_frame(PublicationFrameWork {
        upload_queue_items: client_world::MAX_PENDING_MESH_CHANGES,
        ..PublicationFrameWork::default()
    });

    controller.begin_frame(Duration::from_millis(125));

    assert_eq!(controller.budget().max_per_frame, 256);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
    assert_eq!(controller.budget().max_zero_byte_operations_per_frame, 128);
}

#[test]
fn pressure_recovers_only_after_healthy_frames_without_self_funded_bursts() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    controller.finish_frame(PublicationFrameWork {
        upload_queue_items: client_world::MAX_PENDING_MESH_CHANGES,
        ..PublicationFrameWork::default()
    });
    controller.begin_frame(Duration::from_millis(125));
    let reduced = controller.budget().max_per_frame;
    let reduced_zero = controller.budget().max_zero_byte_operations_per_frame;
    let allowance = controller.allowance();
    while let Some(permit) = allowance.try_admit_payload(1) {
        assert!(permit.retire());
    }
    for _ in 0..119 {
        controller.finish_frame(PublicationFrameWork::default());
        controller.begin_frame(Duration::from_millis(125));
        assert_eq!(controller.budget().max_per_frame, reduced);
        assert!(controller.budget().max_zero_byte_operations_per_frame <= reduced_zero);
        while let Some(permit) = allowance.try_admit_payload(1) {
            assert!(permit.retire());
        }
    }
    controller.finish_frame(PublicationFrameWork::default());
    controller.begin_frame(Duration::from_millis(125));
    assert_eq!(
        controller.budget().max_per_frame,
        reduced.saturating_mul(2).min(config.maximum_frame_items)
    );
    assert_eq!(
        controller.budget().max_zero_byte_operations_per_frame,
        reduced_zero + 1
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
    assert_eq!(controller.diagnostics().additive_increases, 1);
}
#[test]
fn byte_tokens_follow_elapsed_time_and_never_cross_frame_or_burst_ceilings() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();

    controller.begin_frame(Duration::from_millis(125));
    assert_eq!(controller.budget().max_bytes_per_frame, 16 * 1024 * 1024);
    assert!(controller.budget().max_bytes_per_frame <= config.maximum_frame_bytes);
    controller.finish_frame(PublicationFrameWork::default());
    controller.begin_frame(Duration::MAX);
    assert!(controller.budget().max_per_frame <= config.maximum_frame_items);
    assert!(controller.budget().max_bytes_per_frame <= config.maximum_frame_bytes);
    assert!(controller.accrued_items() <= config.maximum_burst_items);
    assert!(controller.accrued_bytes() <= config.maximum_burst_bytes);
}
#[test]
fn idle_wall_time_never_accumulates_more_than_the_one_second_burst_ceiling() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    controller.begin_frame(Duration::from_secs(10));
    assert_eq!(controller.accrued_items(), config.maximum_burst_items);
    assert_eq!(controller.accrued_bytes(), config.maximum_burst_bytes);
    for _ in 0..=config.maximum_burst_items {
        controller.begin_frame(Duration::ZERO);
    }

    assert_eq!(controller.accrued_items(), config.maximum_burst_items);
    assert_eq!(controller.accrued_bytes(), config.maximum_burst_bytes);
}

#[test]
fn eight_hz_frames_receive_two_seconds_of_bounded_service_without_runaway_burst() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    let mut serviced = 0_usize;

    for _ in 0..16 {
        controller.begin_frame(Duration::from_millis(125));
        let allowance = controller.allowance();
        while let Some(permit) = allowance.try_admit_payload(1) {
            serviced = serviced.saturating_add(1);
            assert!(permit.retire());
        }
        controller.finish_frame(PublicationFrameWork::default());
    }

    assert!(
        serviced >= 6_951,
        "two wall-clock seconds at 8 Hz serviced only {serviced} items"
    );
    assert!(controller.budget().max_per_frame <= config.maximum_frame_items);
    assert!(controller.accrued_items() <= config.maximum_burst_items);
}
#[test]
fn paced_eight_hz_saturated_backlog_preserves_bounded_publication_service() {
    let config = PublicationServiceConfig::PHASE2_GATE;
    let mut controller = PublicationController::default();
    let mut serviced = 0_usize;

    for _ in 0..16 {
        controller.begin_frame(Duration::from_millis(125));
        let budget = controller.budget();
        let allowance = controller.allowance();
        let mut published = 0;
        while let Some(permit) = allowance.try_admit_payload(1) {
            serviced = serviced.saturating_add(1);
            published += 1;
            assert!(permit.retire());
        }
        controller.finish_frame(PublicationFrameWork {
            mesh_changes_published: published,
            mesh_payloads_published: published,
            mesh_bytes_published: published as u64,
            pending_mesh_jobs: 5_461,
            in_flight_mesh_jobs: 32,
            upload_queue_items: 128,
            upload_queue_bytes: 32 * 1024 * 1024,
            ..PublicationFrameWork::healthy()
        });
        assert_eq!(published, budget.max_per_frame);
    }

    assert_eq!(serviced, config.maximum_frame_items * 16);
    assert_eq!(
        controller.budget().max_per_frame,
        config.maximum_frame_items
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
}

#[test]
fn controller_credits_shared_allowance_and_only_admitted_work_spends_it() {
    let mut controller = PublicationController::default();
    let allowance = controller.allowance();

    controller.begin_frame(Duration::from_millis(125));
    let first_available = allowance.remaining_items();
    let permit = allowance.try_admit_payload(1).unwrap();
    assert!(permit.retire());
    controller.finish_frame(PublicationFrameWork::healthy());
    controller.begin_frame(Duration::from_millis(125));

    assert_eq!(first_available, 1_024);
    assert_eq!(allowance.remaining_items(), 2_047);
    assert_eq!(allowance.frame_remaining_items(), 512);
}
