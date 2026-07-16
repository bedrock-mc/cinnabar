use assets::RuntimeAssets;
use bevy::prelude::{
    App, AppExit, IntoScheduleConfigs, MinimalPlugins, Quat, Transform, Update, Vec3,
};
use bevy::window::{PresentMode, WindowCloseRequested};
use meshing::{
    ChunkMesh, FaceConnectivity, PackedModelDrawRef, PackedModelRef, PackedQuadLighting,
};
use protocol::{
    ActorKind, BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockUpdateEvent, LevelChunkEvent,
    LevelChunkMode, PlayerMovementCorrectionEvent, PlayerSkin, StandardSkin, SubChunkBatchEvent,
    SubChunkEntryEvent, SubChunkResult, WorldBootstrap, WorldEvent,
};
use render::{
    ChunkBiomeTints, ChunkRenderApplySet, ChunkRenderPlugin, ChunkRenderQueue, ChunkUploadPriority,
    GraphicsAdapterMetadata, OpaqueDrawMode, PresentedFrameAck, RenderViewCohort,
    TargetRenderExpectation, VisibilityDiagnosticSnapshot, VisibilityDiagnosticsInput,
    VisibilityKeyDigest,
};
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use world::{ChunkKey, LightSolveError, SubChunkKey};

use crate::acceptance::markers::{
    ACCEPTANCE_RUNTIME_METADATA, CAMERA_COMMITTED, GALLERY_ANCHOR_READY, MOVE_PLAYER_INGRESS,
    MUTATION_COORDINATE, TARGET_MUTATION_ARMED, TELEPORT_COHORT, TELEPORT_GLOBAL_STAGE_DIAGNOSTIC,
    TRANSPARENT_SORT_COMMITTED, WORLD_PUBLICATION_SNAPSHOT, WORLD_READY,
};
use crate::metrics::TransparentSortMetricsSnapshot;
use crate::runtime::network::{NetworkControlEvent, session::SequencedWorldEvent};
use crate::{
    AcceptanceExitDecision, AcceptanceRun, AcceptanceRuntimeConfig, CaveVisibilityCache,
    FullViewRemeshTracker, FullViewTeleportCompletion, FullViewTeleportTracker,
    GalleryAnchorEmitter, MutationTracker, NETWORK_INGRESS_BUDGET_PER_FRAME,
    OUTBOUND_SEND_BUDGET_PER_FRAME, RollingFps, ShutdownWatchdog, SubChunkTimeoutProgress,
    TRANSPARENT_PRESENTATION_EXIT_GRACE, TeleportReadySnapshot, WORLD_READY_QUIET_INTERVAL,
    WorldReadySettler, WorldReadySnapshot, WorldReadyWork, acceptance_runtime_metadata_marker,
    accepted_move_player_ingress_marker, actor_render_source, apply_added_chunk_visibility,
    apply_committed_control, arm_shutdown_watchdog, bedrock_camera_rotation,
    bridge_endpoint_exists, bridge_endpoint_path, camera_sub_chunk_key, cumulative_counter_delta,
    deterministic_mutation_coordinate, drain_network_controls, drain_network_ingress,
    exact_full_view_proof_marker_fields, exit_on_window_close_requested, fatal_runtime_exit,
    flush_sub_chunk_requests, leaf_forest_target_mutation_coordinate,
    model_gallery_camera_committed_marker, preflight_bridge_endpoint, record_fatal_error,
    remove_chunk_visibility, requested_present_mode, resolve_socket_dir_from, startup_biome_tints,
    status_title, synchronize_biome_tints, target_mutation_armed_marker,
    teleport_global_stage_diagnostic_marker, teleport_proof, transparent_sort_committed_marker,
    update_visibility_diagnostics, visibility_digest_marker_fields, window_close_exit,
    world_publication_snapshot_marker, world_ready_markers, world_stream_fatal_message,
    write_move_player_ingress_before_source_capture, write_stdout_marker,
};
use client_world::{
    CommittedControlEvent, ForcedRemeshManifest, ForcedRemeshManifestState, ViewCohort,
    ViewCohortStatus, WorldMeshChange, WorldStream, WorldStreamFatalError, WorldStreamStats,
};

const DESTINATION_COHORT: ViewCohort = ViewCohort {
    dimension: 0,
    center: [65, 65],
    radius: 16,
};

const SOURCE_COHORT: ViewCohort = ViewCohort {
    dimension: 0,
    center: [0, 0],
    radius: 16,
};

fn exact_destination_status() -> ViewCohortStatus {
    ViewCohortStatus {
        target: DESTINATION_COHORT,
        committed: Some(DESTINATION_COHORT),
        expected: 797,
        loaded_target: 797,
        missing_target: 0,
        foreign_loaded: 0,
        foreign_requested: 0,
        foreign_resident: 0,
        source_leftover: 0,
        resident_count: 9_000,
        resident_hash: 0x1234,
        known_air_count: 1_000,
        known_air_hash: 0x5678,
    }
}

fn settled_teleport_snapshot() -> TeleportReadySnapshot {
    TeleportReadySnapshot {
        received_radius_chunks: Some(16),
        publisher_radius_chunks: Some(16),
        rendered_sub_chunks: 8_000,
        resident_sub_chunks: 9_000,
        visible_sub_chunks: 7_000,
        loaded_columns: 797,
        cohort: Some(exact_destination_status()),
        last_chunk_commit_at: None,
        last_mesh_dispatch_at: None,
        last_mesh_completion_at: None,
        last_mesh_ack_at: None,
        work: WorldReadyWork::default(),
    }
}

fn destination_tracker(started: Instant) -> FullViewTeleportTracker {
    let mut tracker = FullViewTeleportTracker::new(true);
    tracker.set_source_mutation_coordinate([0, 58, 0]);
    tracker.begin_world_ready([0.5, 70.0, 0.5], 1);
    assert!(tracker.observe(
        &WorldEvent::MovePlayer(protocol::MovePlayerEvent {
            runtime_id: 1,
            position: [1_040.5, 70.0, 1_040.5],
            pitch: 0.0,
            yaw: 0.0,
        }),
        started,
        0,
    ));
    tracker.observe(
        &WorldEvent::PublisherUpdate(protocol::PublisherUpdateEvent {
            center: [1_040, 70, 1_040],
            radius_blocks: 256,
        }),
        started + Duration::from_millis(100),
        0,
    );
    tracker
}

fn proposed_render_expectation(
    render_ready_at: Instant,
    manifest: impl IntoIterator<Item = (SubChunkKey, u64)>,
) -> TargetRenderExpectation {
    TargetRenderExpectation {
        cohort: RenderViewCohort::new(
            DESTINATION_COHORT.dimension,
            DESTINATION_COHORT.center,
            DESTINATION_COHORT.radius,
        ),
        source_cohort: Some(RenderViewCohort::new(
            SOURCE_COHORT.dimension,
            SOURCE_COHORT.center,
            SOURCE_COHORT.radius,
        )),
        manifest: Arc::from(manifest.into_iter().collect::<Vec<_>>()),
        view_generation: 0,
        render_ready_at,
    }
}

fn presented_acknowledgement(
    expectation: &TargetRenderExpectation,
    frame_sequence: u64,
    present_after_ready: Duration,
    gpu_after_ready: Duration,
) -> PresentedFrameAck {
    PresentedFrameAck {
        cohort: expectation.cohort,
        frame_sequence,
        allocation_manifest: Arc::clone(&expectation.manifest),
        visible_allocation_manifest: Arc::clone(&expectation.manifest),
        drawn_manifest: Arc::clone(&expectation.manifest),
        view_generation: expectation.view_generation,
        render_ready_at: expectation.render_ready_at,
        present_returned_at: expectation.render_ready_at + present_after_ready,
        gpu_completed_at: expectation.render_ready_at + gpu_after_ready,
        missing_target_instances: 0,
        unexpected_target_instances: 0,
        source_instances: 0,
        foreign_instances: 0,
        stale_generation_instances: 0,
        orphan_allocations: 0,
        transparent_sort_generation: 17,
        model_witness: None,
    }
}

fn binding_teleport_completion(
    started: Instant,
    settle_latency: Duration,
) -> FullViewTeleportCompletion {
    let mut tracker = destination_tracker(started);
    let key = SubChunkKey::new(0, 64, 65, 65);
    let render_ready = Duration::from_millis(200);
    let expectation = tracker
        .reconcile_presented_expectation(
            settled_teleport_snapshot(),
            proposed_render_expectation(started + render_ready, [(key, 7)]),
            started + render_ready,
        )
        .unwrap();
    let stable_gpu_after_ready = settle_latency.saturating_sub(render_ready);
    let first_gpu_after_ready = stable_gpu_after_ready.saturating_sub(Duration::from_millis(20));
    assert_eq!(
        tracker.observe_presented_frame(presented_acknowledgement(
            &expectation,
            41,
            first_gpu_after_ready.saturating_sub(Duration::from_millis(10)),
            first_gpu_after_ready,
        )),
        None
    );
    tracker
        .observe_presented_frame(presented_acknowledgement(
            &expectation,
            42,
            stable_gpu_after_ready.saturating_sub(Duration::from_millis(10)),
            stable_gpu_after_ready,
        ))
        .unwrap()
}

mod core;
mod finish;
mod teleport;

use core::{complete_world_stream_decodes, overworld_biome_payload, settled_world_snapshot};
