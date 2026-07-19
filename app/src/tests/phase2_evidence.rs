use client_world::{
    BuildProfileIdentity, CohortManifestIdentity, Phase2PresentationSnapshot,
    Phase2PublicationSnapshot, PresentModeIdentity, PublicationStageCounters, RequestClass,
    RequestClassDepth, RequestQueueEvidence, StageDurations, SubChunkOutcomeCounters,
};
use protocol::BlobCacheStats;
use sha2::{Digest, Sha256};
use world::ChunkKey;

use crate::runtime::phase2_evidence::{
    CombinedPhase2Snapshot, PlayerColumnPresentationEvidence, generation_manifest_identity,
    graphics_identity_sha256, key_manifest_identity, phase2_publication_line_if_changed,
    phase2_publication_timing_line, sha256_identity_from_hex_or_text,
};
use render::{VisibilityDiagnosticsInput, VisibilityKeyDigest};

use crate::runtime::telemetry::local_subject_column;

fn combined_snapshot() -> CombinedPhase2Snapshot {
    let cohort = CohortManifestIdentity {
        session_generation: 7,
        publisher_epoch: 9,
        required_cohort_count: 1_089,
        required_cohort_hash: 11,
        generation_manifest_hash: 13,
        entry_count: 17,
    };
    CombinedPhase2Snapshot {
        publication: Phase2PublicationSnapshot {
            session_generation: 7,
            publisher_epoch: 9,
            publisher_center: Some([64, 70, -32]),
            player_column: ChunkKey::new(0, 4, -2),
            publisher_radius_blocks: Some(256),
            publisher_radius_chunks: Some(16),
            required_cohort_hash: 11,
            required_columns: 1_089,
            loaded_required_columns: 1_089,
            player_column_required: true,
            player_column_loaded: true,
            required_cohort_stable: true,
            inactive_level_chunks: 0,
            local_reset_armed: false,
            local_resets_armed: 1,
            local_resets_consumed: 1,
            local_reset_dispatch_count: 2,
            local_reset_dispatch_total: 2,
            local_reset_dispatch_trace_overflowed: false,
            local_reset_dispatch_classes: [
                Some(RequestClass::PlayerRetry),
                Some(RequestClass::VisibleInitial),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ],
            request_queue: RequestQueueEvidence {
                class_depths: [
                    RequestClassDepth {
                        class: RequestClass::PlayerRetry,
                        ready: 1,
                        eligible: 1,
                    },
                    RequestClassDepth {
                        class: RequestClass::PlayerInitial,
                        ready: 0,
                        eligible: 0,
                    },
                    RequestClassDepth {
                        class: RequestClass::VisibleRetry,
                        ready: 2,
                        eligible: 2,
                    },
                    RequestClassDepth {
                        class: RequestClass::VisibleInitial,
                        ready: 0,
                        eligible: 0,
                    },
                    RequestClassDepth {
                        class: RequestClass::PrefetchRetry,
                        ready: 0,
                        eligible: 0,
                    },
                    RequestClassDepth {
                        class: RequestClass::PrefetchInitial,
                        ready: 3,
                        eligible: 0,
                    },
                ],
                reservations: 1,
                ready_blocked_by_reservation: 3,
                next_class: Some(RequestClass::PlayerRetry),
                next_is_transport_retry: false,
                next_is_starved: false,
            },
            stages: PublicationStageCounters::default(),
            outcomes: SubChunkOutcomeCounters::default(),
            max_queue_wait: StageDurations::default(),
            max_worker_time: StageDurations::default(),
        },
        presentation: Phase2PresentationSnapshot {
            build_profile: BuildProfileIdentity::Release,
            graphics_identity_sha256: [3; 32],
            requested_present_mode: PresentModeIdentity::Fifo,
            effective_present_mode: PresentModeIdentity::Fifo,
            assets_manifest_sha256: [5; 32],
            visible_subset_of_resident: true,
            publisher_disk: cohort,
            resident: cohort,
            allocation: cohort,
            visible: cohort,
            submitted: cohort,
            gpu_presented: cohort,
        },
        player_column_presentation: PlayerColumnPresentationEvidence {
            column: ChunkKey::new(0, 4, -2),
            resident_subchunks: Some(16),
            allocated_subchunks: 16,
            visible_subchunks: Some(8),
            submitted_subchunks: Some(8),
            gpu_presented_subchunks: Some(8),
        },
        present_mode_proven: true,
        client_blob_cache_enabled: true,
        client_blob_cache: BlobCacheStats {
            hashes_classified: 7,
            hits: 3,
            misses: 4,
            admitted_blobs: 4,
            reconstructed_level_chunks: 2,
            reconstructed_sub_chunks: 1,
            ..Default::default()
        },
    }
}

#[test]
fn phase2_publication_timing_binds_exact_line_hash_and_unix_millis() {
    let publication = "PHASE2_PUBLICATION={\"publication\":{}}";
    let marker = phase2_publication_timing_line(publication, 1_000_123);
    let expected_hash = format!("{:x}", Sha256::digest(publication.as_bytes()));
    assert_eq!(
        marker,
        format!(
            "RUST_MCBE_PHASE2_TIMING={{\"observed_unix_ms\":1000123,\"publication_sha256\":\"{expected_hash}\",\"schema\":\"rust-mcbe-phase2-timing-v1\"}}"
        )
    );
}

#[test]
fn player_column_witness_uses_subject_not_third_person_camera_boom() {
    let subject_eye = bevy::prelude::Vec3::new(15.9, 70.0, 0.0);
    let third_person_camera = bevy::prelude::Vec3::new(16.1, 70.0, 0.0);

    assert_eq!(
        local_subject_column(0, subject_eye),
        Some(ChunkKey::new(0, 0, 0))
    );
    assert_eq!(
        local_subject_column(0, third_person_camera),
        Some(ChunkKey::new(0, 1, 0))
    );
}

#[test]
fn serialized_phase2_path_reads_the_frozen_local_subject_column() {
    let source = include_str!("../runtime/telemetry.rs");
    let record = source
        .split("pub(crate) fn record_metrics_and_title")
        .nth(1)
        .expect("record_metrics_and_title source")
        .split("\n    if let Some(stream) = client_world.stream.as_ref() {")
        .next()
        .expect("record_metrics_and_title body");

    assert!(record.contains("render_metrics.local_player.snapshot()"));
    assert!(record.contains("local_subject_column(stream.current_dimension(), local_frame.eye())"));
    assert!(
        !record.contains("camera_sub_chunk_key(stream.current_dimension(), camera.translation)")
    );
}

#[test]
fn phase2_publication_emits_once_per_changed_combined_identity() {
    let snapshot = combined_snapshot();
    let mut previous = None;
    let first = phase2_publication_line_if_changed(&mut previous, snapshot).unwrap();
    assert!(first.starts_with("PHASE2_PUBLICATION={"));
    assert!(first.contains("\"publisher_radius_blocks\":256"));
    assert!(first.contains("\"publisher_radius_chunks\":16"));
    assert!(first.contains("\"publisher_epoch\":9"));
    assert!(first.contains("\"publisher_center\":[64,70,-32]"));
    assert!(first.contains("\"player_column_required\":true"));
    assert!(first.contains("\"player_column_loaded\":true"));
    assert!(first.contains("\"armed_count\":1"));
    assert!(first.contains("\"consumed_count\":1"));
    assert!(first.contains("\"dispatch_classes\":[\"player_retry\",\"visible_initial\"]"));
    assert!(first.contains("\"dispatch_total\":2"));
    assert!(first.contains("\"dispatch_trace_overflowed\":false"));
    assert!(first.contains("\"class\":\"player_retry\",\"eligible\":1,\"ready\":1"));
    assert!(first.contains("\"next_class\":\"player_retry\""));
    assert!(first.contains("\"required_cohort_stable\":true"));
    assert!(first.contains("\"graphics_identity_sha256\":\"030303"));
    assert!(first.contains("\"client_blob_cache_enabled\":true"));
    assert!(first.contains("\"resident_subchunks\":16"));
    assert!(first.contains("\"allocated_subchunks\":16"));
    assert!(first.contains("\"gpu_presented_subchunks\":8"));
    assert!(first.contains("\"hashes_classified\":7"));
    assert!(first.contains("\"hits\":3"));
    assert!(first.contains("\"misses\":4"));
    assert_eq!(
        phase2_publication_line_if_changed(&mut previous, snapshot),
        None
    );

    let mut changed = snapshot;
    changed.publication.stages.mesh_changes_dequeued = 1;
    assert!(phase2_publication_line_if_changed(&mut previous, changed).is_some());
}

#[test]
fn phase2_publication_line_contains_no_filesystem_or_auth_fields() {
    let mut previous = None;
    let line = phase2_publication_line_if_changed(&mut previous, combined_snapshot()).unwrap();
    for forbidden in ["path", "token", "auth", "payload", "credential"] {
        assert!(!line.to_ascii_lowercase().contains(forbidden));
    }
}

#[test]
fn phase2_publication_exposes_effective_present_mode_proof() {
    let mut previous = None;
    let line = phase2_publication_line_if_changed(&mut previous, combined_snapshot()).unwrap();
    assert!(line.contains("\"present_mode_proven\":true"));
}

#[test]
fn phase2_identity_helpers_preserve_exact_hashes_and_mark_missing_digests() {
    assert_eq!(
        sha256_identity_from_hex_or_text(&"a5".repeat(32)),
        [0xa5; 32]
    );
    assert_ne!(sha256_identity_from_hex_or_text("diagnostic"), [0; 32]);

    assert_eq!(
        key_manifest_identity(
            7,
            9,
            1_089,
            11,
            Some(VisibilityKeyDigest { count: 3, hash: 5 }),
        ),
        CohortManifestIdentity {
            session_generation: 7,
            publisher_epoch: 9,
            required_cohort_count: 1_089,
            required_cohort_hash: 11,
            generation_manifest_hash: 5,
            entry_count: 3,
        }
    );
    assert_eq!(
        key_manifest_identity(7, 9, 1_089, 11, None).generation_manifest_hash,
        0
    );
}

#[test]
fn phase2_publication_labels_comparable_manifest_hash_domains() {
    let mut previous = None;
    let line = phase2_publication_line_if_changed(&mut previous, combined_snapshot()).unwrap();
    assert!(line.contains("\"manifest_domain\":\"key_generation\""));
    assert!(line.contains("\"manifest_domain\":\"key\""));
    assert!(line.contains("\"visible_subset_of_resident\":true"));
}

#[test]
fn phase2_key_manifest_identity_does_not_change_with_observation_frame() {
    let key = world::SubChunkKey::new(0, 1, 2, 3);
    let mut diagnostics = VisibilityDiagnosticsInput::new(true);
    diagnostics.advance([key], [key]);
    let first = key_manifest_identity(7, 9, 1_089, 11, diagnostics.resident_mesh());
    diagnostics.advance([key], [key]);
    let second = key_manifest_identity(7, 9, 1_089, 11, diagnostics.resident_mesh());
    assert_eq!(
        first, second,
        "an unchanged semantic key manifest must not churn solely because a render frame advanced",
    );

    let mut snapshot = combined_snapshot();
    snapshot.presentation.resident = first;
    snapshot.presentation.visible = first;
    snapshot.presentation.submitted = first;
    snapshot.presentation.gpu_presented = first;
    let mut previous = None;
    assert!(phase2_publication_line_if_changed(&mut previous, snapshot).is_some());

    snapshot.presentation.resident = second;
    snapshot.presentation.visible = second;
    snapshot.presentation.submitted = second;
    snapshot.presentation.gpu_presented = second;
    assert_eq!(
        phase2_publication_line_if_changed(&mut previous, snapshot),
        None,
        "an observation-only frame must not emit or flush a duplicate marker",
    );

    let changed_key = world::SubChunkKey::new(0, 2, 2, 3);
    diagnostics.advance([key, changed_key], [key]);
    snapshot.presentation.resident =
        key_manifest_identity(7, 9, 1_089, 11, diagnostics.resident_mesh());
    assert!(
        phase2_publication_line_if_changed(&mut previous, snapshot).is_some(),
        "a semantic resident-manifest change must emit one updated marker",
    );
}

#[test]
fn phase2_frame_metrics_observe_the_real_clock() {
    let source = include_str!("../runtime/telemetry.rs");
    let function = source
        .split_once("pub(crate) fn record_metrics_and_title(")
        .expect("record_metrics_and_title definition")
        .1
        .split_once(") {")
        .expect("record_metrics_and_title signature")
        .0;
    assert!(
        function.contains("time: Res<Time<Real>>"),
        "frame metrics must use unclamped wall-clock frame deltas, not Bevy's virtual clock",
    );
}

#[test]
fn phase2_graphics_identity_covers_effective_present_mode() {
    let mut graphics = render::GraphicsAdapterMetadata {
        backend: "Vulkan".to_owned(),
        adapter: "adapter".to_owned(),
        driver: "driver".to_owned(),
        driver_info: "info".to_owned(),
        requested_present_mode: "Fifo".to_owned(),
        effective_present_mode: "Fifo".to_owned(),
        present_mode_proven: true,
    };
    let fifo = graphics_identity_sha256(&graphics);
    graphics.effective_present_mode = "Immediate".to_owned();
    assert_ne!(fifo, graphics_identity_sha256(&graphics));
}

#[test]
fn phase2_generation_manifest_identity_is_order_independent() {
    let first = (world::SubChunkKey::new(0, 1, 2, 3), 7);
    let second = (world::SubChunkKey::new(0, -4, 5, 6), 9);
    assert_eq!(
        generation_manifest_identity(11, 12, 749, 13, &[first, second]),
        generation_manifest_identity(11, 12, 749, 13, &[second, first])
    );
}
