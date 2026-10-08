//! Contracts for active visibility witnesses and current publication counters.

use super::*;

#[test]
fn world_publication_snapshot_is_deterministic_and_keeps_stage_identities_separate() {
    let stats = WorldStreamStats {
        accepted_light_jobs: u64::MAX,
        noop_light_jobs: 2,
        value_changed_light_jobs: 3,
        provenance_only_light_jobs: 5,
        light_mesh_invalidations: 7,
        stale_light_jobs: 11,
        stale_mesh_jobs: 13,
        queued_decode_jobs: 17,
        in_flight_decode_jobs: 19,
        pending_light_jobs: 23,
        in_flight_light_jobs: 29,
        pending_mesh_jobs: 31,
        in_flight_mesh_jobs: 37,
        max_decode_queue_wait: Duration::from_millis(41),
        max_light_queue_wait: Duration::from_millis(43),
        max_mesh_queue_wait: Duration::from_millis(47),
        max_decode_duration: Duration::from_millis(53),
        max_light_duration: Duration::from_millis(59),
        max_mesh_duration: Duration::from_millis(61),
        ..Default::default()
    };
    let visibility = VisibilityDiagnosticSnapshot {
        frame_generation: 67,
        pose_generation: 71,
        view_generation: 73,
        draw_mode: OpaqueDrawMode::Direct,
        ..Default::default()
    };
    let graphics = GraphicsAdapterMetadata {
        backend: "Dx12".to_owned(),
        adapter: "Test Adapter".to_owned(),
        driver: "test-driver".to_owned(),
        driver_info: "1.2.3".to_owned(),
        requested_present_mode: "Fifo".to_owned(),
        effective_present_mode: "Fifo".to_owned(),
        present_mode_proven: true,
    };

    let marker = world_publication_snapshot_marker(
        stats,
        79,
        83,
        89,
        visibility,
        AcceptanceRuntimeConfig {
            build_profile: "debug",
        },
        &graphics,
    );
    assert_eq!(
        marker,
        world_publication_snapshot_marker(
            stats,
            79,
            83,
            89,
            visibility,
            AcceptanceRuntimeConfig {
                build_profile: "debug",
            },
            &graphics,
        )
    );
    let document: serde_json::Value = serde_json::from_str(
        marker
            .strip_prefix(&format!("{WORLD_PUBLICATION_SNAPSHOT}="))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(document["accepted_light_jobs"], u64::MAX);
    assert_eq!(document["max_decode_queue_wait_ms"], 41.0);
    assert_eq!(document["max_decode_worker_ms"], 53.0);
    assert_eq!(document["upload_queue_items"], 79);
    assert_eq!(document["upload_queue_bytes"], 83);
    assert_eq!(document["gpu_upload_bytes"], 89);
    assert_eq!(document["frame_generation"], 67);
    assert!(document.get("visibility_snapshot_valid").is_none());
    assert_eq!(document["draw_mode"], "Direct");
    assert_eq!(document["build_profile"], "debug");
    assert_eq!(document["requested_present_mode"], "Fifo");
    assert_eq!(document["effective_present_mode"], "Fifo");
    assert_eq!(document["present_mode_proven"], true);
    let inactive_marker = world_publication_snapshot_marker(
        stats,
        79,
        83,
        89,
        VisibilityDiagnosticSnapshot::default(),
        AcceptanceRuntimeConfig {
            build_profile: "debug",
        },
        &graphics,
    );
    let inactive: serde_json::Value =
        serde_json::from_str(inactive_marker.split_once('=').unwrap().1).unwrap();
    assert_eq!(inactive["accepted_light_jobs"], u64::MAX);
    assert_eq!(inactive["gpu_upload_bytes"], 89);
    assert_eq!(inactive["visibility_snapshot_valid"], false);
    for identity in [
        "frame_generation",
        "pose_generation",
        "view_generation",
        "draw_mode",
    ] {
        assert!(inactive[identity].is_null());
    }
}
