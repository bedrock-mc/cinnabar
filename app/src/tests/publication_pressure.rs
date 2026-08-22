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
