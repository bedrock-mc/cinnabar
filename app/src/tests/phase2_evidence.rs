use client_world::{
    BuildProfileIdentity, CohortManifestIdentity, Phase2PresentationSnapshot,
    Phase2PublicationSnapshot, PresentModeIdentity, PublicationStageCounters, StageDurations,
    SubChunkOutcomeCounters,
};
use protocol::BlobCacheStats;
use world::ChunkKey;

use crate::runtime::phase2_evidence::{
    CombinedPhase2Snapshot, cohort_identity, generation_manifest_identity,
    graphics_identity_sha256, phase2_publication_line_if_changed, sha256_identity_from_hex_or_text,
};
use render::VisibilityKeyDigest;

fn combined_snapshot() -> CombinedPhase2Snapshot {
    let cohort = CohortManifestIdentity {
        session_generation: 7,
        required_cohort_hash: 11,
        generation_manifest_hash: 13,
        entry_count: 17,
    };
    CombinedPhase2Snapshot {
        publication: Phase2PublicationSnapshot {
            session_generation: 7,
            player_column: ChunkKey::new(0, 4, -2),
            publisher_radius_blocks: Some(256),
            publisher_radius_chunks: Some(16),
            required_cohort_hash: 11,
            required_columns: 1_089,
            loaded_required_columns: 1_089,
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
            publisher_disk: cohort,
            resident: cohort,
            allocation: cohort,
            visible: cohort,
            submitted: cohort,
            gpu_presented: cohort,
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
fn phase2_publication_emits_once_per_changed_combined_identity() {
    let snapshot = combined_snapshot();
    let mut previous = None;
    let first = phase2_publication_line_if_changed(&mut previous, snapshot).unwrap();
    assert!(first.starts_with("PHASE2_PUBLICATION={"));
    assert!(first.contains("\"publisher_radius_blocks\":256"));
    assert!(first.contains("\"publisher_radius_chunks\":16"));
    assert!(first.contains("\"graphics_identity_sha256\":\"030303"));
    assert!(first.contains("\"client_blob_cache_enabled\":true"));
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
        cohort_identity(7, 11, 19, Some(VisibilityKeyDigest { count: 3, hash: 5 })),
        CohortManifestIdentity {
            session_generation: 7,
            required_cohort_hash: 11,
            generation_manifest_hash: 19 ^ 5,
            entry_count: 3,
        }
    );
    assert_eq!(cohort_identity(7, 11, 19, None).generation_manifest_hash, 0);
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
        generation_manifest_identity(11, 13, &[first, second]),
        generation_manifest_identity(11, 13, &[second, first])
    );
}
