use std::time::Duration;

use bevy::{
    prelude::{App, IntoScheduleConfigs, ResMut, Time, Update},
    time::Real,
};
use render_api::PublicationServiceConfig;
use render_scene::{ChunkRenderApplySet, ChunkRenderQueue, ChunkUploadBudget};

use crate::publication::{
    PublicationController, PublicationFrameWork, begin_publication_frame,
    configure_publication_frame,
};

/// Streaming air and retirements leave the full elapsed-time geometry service available.
#[test]
fn sustained_zero_byte_work_does_not_starve_geometry_service() {
    let config = PublicationServiceConfig::DEFAULT;
    let mut controller = PublicationController::default();
    let mut payloads = 0;
    let mut removals = 0;
    let mut elapsed = Duration::ZERO;
    for frame in 0..120 {
        // Alternating frame lengths cover exactly one second without sleeping.
        let duration = Duration::from_nanos(if frame % 3 == 0 { 8_333_334 } else { 8_333_333 });
        elapsed += duration;
        controller.begin_frame(duration);
        let allowance = controller.allowance();
        let mut frame_removals = 0;
        let mut frame_payloads = 0;
        for _ in 0..config.maximum_zero_byte_operations_per_frame / 2 {
            let Some(permit) = allowance.try_admit_zero_byte() else {
                break;
            };
            frame_removals += 1;
            assert!(permit.retire());
        }
        while let Some(permit) = allowance.try_admit_payload(1) {
            frame_payloads += 1;
            assert!(permit.retire());
        }
        assert!(frame_payloads + frame_removals <= config.maximum_frame_items);
        assert!(frame_removals <= config.maximum_zero_byte_operations_per_frame);
        payloads += frame_payloads;
        removals += frame_removals;
        controller.finish_frame(PublicationFrameWork {
            mesh_changes_published: frame_payloads + frame_removals,
            mesh_payloads_published: frame_payloads,
            mesh_bytes_published: frame_payloads as u64,
            ..PublicationFrameWork::healthy()
        });
    }
    assert_eq!(elapsed, Duration::from_secs(1));
    assert_eq!(payloads, config.target_items_per_second as usize);
    assert_eq!(
        removals,
        120 * config.maximum_zero_byte_operations_per_frame / 2
    );
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
    assert_eq!(controller.allowance().live_permits(), 0);
}

/// A full handoff the renderer applies in the same frame must not read as a stuck backlog.
#[test]
fn a_batch_applied_in_its_own_frame_does_not_collapse_the_caps() {
    let config = PublicationServiceConfig::DEFAULT;
    let mut app = App::new();
    app.init_resource::<Time<Real>>()
        .init_resource::<ChunkUploadBudget>()
        .init_resource::<ChunkRenderQueue>()
        .init_resource::<PublicationController>();
    configure_publication_frame(&mut app);
    // World publication samples the queue before the renderer applies this frame's handoff.
    app.add_systems(
        Update,
        (move |mut controller: ResMut<PublicationController>| {
            controller.finish_frame(PublicationFrameWork {
                upload_queue_items: config.maximum_frame_items,
                upload_queue_bytes: config.maximum_frame_bytes,
                ..PublicationFrameWork::default()
            });
        })
        .after(begin_publication_frame)
        .before(ChunkRenderApplySet),
    );
    // The renderer applies the whole handoff, leaving the queue empty.
    app.add_systems(Update, (|| {}).in_set(ChunkRenderApplySet));
    for _ in 0..3 {
        app.update();
    }
    let controller = app.world().resource::<PublicationController>();
    assert_eq!(controller.diagnostics().multiplicative_decreases, 0);
    assert_eq!(
        controller.budget().max_zero_byte_operations_per_frame,
        config.maximum_zero_byte_operations_per_frame
    );
}

#[test]
fn draining_backlog_stops_collapsing_caps_before_the_one_operation_trap() {
    let config = PublicationServiceConfig::DEFAULT;
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
            upload_queue_items: chunk_pipeline::MAX_PENDING_MESH_CHANGES + frame,
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
    let config = PublicationServiceConfig::DEFAULT;
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
    let config = PublicationServiceConfig::DEFAULT;
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
    let config = PublicationServiceConfig::DEFAULT;
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
        upload_queue_items: chunk_pipeline::MAX_PENDING_MESH_CHANGES,
        ..PublicationFrameWork::default()
    });

    controller.begin_frame(Duration::from_millis(125));

    assert_eq!(controller.budget().max_per_frame, 256);
    assert_eq!(controller.diagnostics().multiplicative_decreases, 1);
    assert_eq!(controller.budget().max_zero_byte_operations_per_frame, 128);
}

#[test]
fn pressure_recovers_only_after_healthy_frames_without_self_funded_bursts() {
    let config = PublicationServiceConfig::DEFAULT;
    let mut controller = PublicationController::default();
    controller.finish_frame(PublicationFrameWork {
        upload_queue_items: chunk_pipeline::MAX_PENDING_MESH_CHANGES,
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
    let config = PublicationServiceConfig::DEFAULT;
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
    let config = PublicationServiceConfig::DEFAULT;
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
    let config = PublicationServiceConfig::DEFAULT;
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
    let config = PublicationServiceConfig::DEFAULT;
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

/// A disconnected frame reports no old work after pressure consumes the previous sample.
#[test]
fn publication_frame_does_not_repeat_retired_world_backlog() {
    let mut controller = PublicationController::default();
    controller.finish_frame(PublicationFrameWork {
        pending_mesh_jobs: 8_931,
        in_flight_mesh_jobs: 6,
        mesh_jobs_dispatched: 1,
        mesh_changes_published: 1,
        ..PublicationFrameWork::default()
    });
    for _ in 0..38 {
        controller.begin_frame(Duration::from_millis(16));
        assert_eq!(
            controller.diagnostics().last_work,
            PublicationFrameWork::default()
        );
    }
}

#[test]
fn review_pressure_floor_never_exceeds_configured_operation_caps() {
    for maximum in [1, 4] {
        let config = PublicationServiceConfig {
            maximum_frame_items: maximum,
            maximum_zero_byte_operations_per_frame: maximum,
            ..PublicationServiceConfig::DEFAULT
        };
        let mut controller = PublicationController::new(config);
        controller.finish_frame(PublicationFrameWork {
            upload_queue_items: 520,
            ..PublicationFrameWork::default()
        });
        controller.begin_frame(Duration::from_millis(125));
        assert!(controller.budget().max_per_frame <= maximum);
        assert!(controller.budget().max_zero_byte_operations_per_frame <= maximum);
    }
}
