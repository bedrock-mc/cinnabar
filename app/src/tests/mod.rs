#[cfg(not(feature = "acceptance"))]
use crate::acceptance::AcceptanceRun;
use crate::player_runtime::PlayerRuntime;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use assets::RuntimeAssets;
use bevy::prelude::{
    App, AppExit, IntoScheduleConfigs, MinimalPlugins, Quat, Transform, Update, Vec3,
};
use bevy::window::WindowCloseRequested;
use meshing::{
    ChunkBiomeTintIdentity, ChunkMesh, DiagnosticGeometryCount, DiagnosticGeometrySummary,
    FaceConnectivity, PackedBiomeRecord, PackedModelDrawRef, PackedModelRef, PackedQuadLighting,
};
use protocol::{
    ActorKind, BiomeDefinitionEvent, BiomeDefinitionsEvent, BlockUpdateEvent, LevelChunkEvent,
    LevelChunkMode, PlayerMovementCorrectionEvent, PlayerSkin, SubChunkBatchEvent,
    SubChunkEntryEvent, SubChunkResult, WorldBootstrap, WorldEvent,
};
use render::{
    ChunkBiomeTints, ChunkRenderApplySet, ChunkRenderPlugin, ChunkRenderQueue, ChunkUploadPriority,
    PresentedFrameAck, RenderViewCohort, TargetRenderExpectation, VisibilityDiagnosticsInput,
};
use render_api::StandardSkin;
use render_model::{
    GraphicsAdapterMetadata, OpaqueDrawMode, VisibilityDiagnosticSnapshot, VisibilityKeyDigest,
};
use std::{
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
use world::{ChunkKey, LightSolveError, SubChunkKey};

fn actor_snapshot(spawn: protocol::ActorSpawnEvent) -> client_world::ActorSnapshot {
    let runtime = spawn.runtime_id;
    let position = spawn.position;
    let mut stream = chunk_pipeline::WorldStream::new_with_assets(
        WorldBootstrap {
            dimension: spawn.dimension,
            local_player_runtime_id: 0,
            local_player_unique_id: 0,
            player_position: spawn.position,
            world_spawn_position: [0; 3],
            air_network_id: protocol::air_network_id(false),
            block_network_ids_are_hashes: false,
        },
        Arc::new(RuntimeAssets::diagnostic()),
        spawn.position,
        None,
    );
    stream
        .submit(1, WorldEvent::Actor(protocol::ActorEvent::Spawn(spawn)))
        .unwrap();
    stream.poll(position, 0);
    stream
        .authority()
        .actor(runtime)
        .expect("spawn committed")
        .clone()
}

use crate::runtime::network::{
    NetworkControlEvent,
    session::{SequencedWorldEvent, WorldIngress},
};
use crate::runtime::{
    endpoint::{preflight_bridge_endpoint, resolve_socket_dir_from},
    network::{
        NetworkHandle, OUTBOUND_SEND_BUDGET_PER_FRAME, WORLD_INGRESS_DRAIN_BUDGET,
        WorldIngressDrain, actor_render_source, drain_network_controls, drain_network_ingress,
        update_actor_render_scene,
    },
    shutdown::{
        exit_on_window_close_requested, fatal_runtime_exit, record_fatal_error, window_close_exit,
    },
    telemetry::{
        CommittedBiomeBlendSnapshot, bedrock_camera_rotation,
        biome_blend_diagnostic_marker_if_changed, biome_blend_diagnostics_enabled,
        camera_sub_chunk_key, refresh_diagnostic_attribution, update_visibility_diagnostics,
    },
    visibility::{CaveVisibilityCache, apply_added_chunk_visibility, remove_chunk_visibility},
    world::{
        ShutdownWatchdog, TeardownWatchdog, apply_committed_control, arm_shutdown_watchdog,
        flush_sub_chunk_requests, startup_biome_tints, synchronize_biome_tints,
        world_stream_fatal_message,
    },
};
use acceptance::committed_control::{
    model_gallery_camera_committed_marker, refresh_mutation_anchor_from_committed_control,
};
use chunk_pipeline::{
    ForcedRemeshManifest, ForcedRemeshManifestState, ViewCohortStatus, WorldMeshChange,
    WorldStream, WorldStreamFatalError, WorldStreamStats,
};
use client_world::{CommittedControlEvent, PublisherViewGeometry, ViewCohort};
use diagnostics::markers::{
    ACCEPTANCE_RUNTIME_METADATA, CAMERA_COMMITTED, GALLERY_ANCHOR_READY, MOVE_PLAYER_INGRESS,
    MUTATION_COORDINATE, TARGET_MUTATION_ARMED, TELEPORT_COHORT, TELEPORT_GLOBAL_STAGE_DIAGNOSTIC,
    TRANSPARENT_SORT_COMMITTED, WORLD_PUBLICATION_SNAPSHOT, WORLD_READY,
};
use diagnostics::metrics::{
    DiagnosticQuadTracker, MetricsCollector, TransparentSortMetricsSnapshot,
};
use {
    acceptance::mutation::{
        MutationTracker, accepted_move_player_ingress_marker, deterministic_mutation_coordinate,
        leaf_forest_target_mutation_coordinate, target_mutation_armed_marker, world_ready_markers,
        write_move_player_ingress_before_source_capture,
    },
    acceptance::proofs::{exact_full_view_proof_marker_fields, teleport_proof},
    acceptance::remesh::FullViewRemeshTracker,
    acceptance::teleport::{
        FullViewTeleportCompletion, FullViewTeleportTracker, TeleportReadySnapshot,
        teleport_global_stage_diagnostic_marker,
    },
    acceptance::world_ready::{
        GalleryAnchorEmitter, SubChunkTimeoutProgress, WORLD_READY_QUIET_INTERVAL,
        WorldReadySettler, WorldReadySnapshot, WorldReadyWork, mutation_look_target,
        orient_acceptance_camera, orient_mutation_camera,
    },
    acceptance::{
        AcceptanceExitDecision, Phase3TerminalDrainDecision, TRANSPARENT_PRESENTATION_EXIT_GRACE,
    },
};

const DESTINATION_COHORT: ViewCohort = ViewCohort {
    dimension: 0,
    center: [65, 65],
    radius: 16,
    publisher_geometry: Some(PublisherViewGeometry {
        center_blocks: [1_040, 1_040],
        radius_blocks: 256,
    }),
};

const SOURCE_COHORT: ViewCohort = ViewCohort {
    dimension: 0,
    center: [0, 0],
    radius: 16,
    publisher_geometry: Some(PublisherViewGeometry {
        center_blocks: [0, 0],
        radius_blocks: 256,
    }),
};

fn exact_destination_status() -> ViewCohortStatus {
    ViewCohortStatus {
        target: DESTINATION_COHORT,
        committed: Some(DESTINATION_COHORT),
        publisher_epoch: 1,
        expected: 797,
        required_hash: 0x9abc,
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
        readiness_produced: 0,
        readiness_consumed: 0,
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
            ..Default::default()
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
        target_columns: None,
        target_keys: None,
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

pub(crate) mod actor_frame_allocations;
mod actor_pack_publication;
pub(crate) mod actor_rest_presentation;
pub(crate) mod alloc_count;
mod audio;
mod audio_camera;
mod camera;
mod camera_controls;
mod cohort_epoch;
mod core;

mod crafting_authority_schedule;
mod finish;
#[cfg(feature = "reports")]
mod frame_cost_bench;
mod gameplay_click;
mod input_publication;
mod inventory;
mod inventory_close_order;
mod inventory_reopen;
mod inventory_schedule;
mod inventory_secondary_input;
mod menu_scene;
mod pack_entity_metadata;
mod phase2_evidence;
mod phase4_presentation;
mod player_animation;
mod publication;
mod publication_pressure;
mod runtime_metrics;

mod teleport;
mod viewmodel_presentation;

use core::{complete_world_stream_decodes, overworld_biome_payload, settled_world_snapshot};

/// Runs a fixture command against the same player and UI resources used by scheduled systems.
pub(crate) fn with_ui_player<T>(
    app: &mut App,
    command: impl FnOnce(&mut client_ui::ui_runtime::UiRuntime, &mut PlayerRuntime) -> T,
) -> T {
    app.world_mut()
        .resource_scope(|world, mut player: bevy::prelude::Mut<PlayerRuntime>| {
            command(
                &mut world.resource_mut::<client_ui::ui_runtime::UiRuntime>(),
                &mut player,
            )
        })
}

use bridge::{bridge_endpoint_exists, session_endpoint_path as bridge_endpoint_path};

use launcher_host::core_process::CoreProcessGuard;
