use super::*;
use crate::runtime::network::resource_packs::PackApplication;
use client_runtime::network::NetworkHandle;
use protocol::{
    LevelChunkEvent, LevelChunkMode, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult,
    WorldBootstrap, WorldEvent,
};
use render_model::VisibilityKeyDigest;

#[test]
fn sparse_startup_waits_for_mesh_acknowledgement_and_a_fresh_gpu_frame() {
    let mut stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 42,
        local_player_unique_id: 42,
        player_position: [0.0, 64.0, 0.0],
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let key = world::SubChunkKey::new(0, 0, 4, 0);
    let identity = stream.stage_publication_fixture_completions(vec![(
        key,
        meshing::ChunkMesh::default(),
        meshing::PackedBiomeRecord::fallback(),
    )])[0];
    stream.load_publication_fixture_column(key.chunk());
    stream.service_publication_fixture_completions();
    let changes = stream.take_mesh_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(stream.unacknowledged_mesh_count(), 1);
    assert!(!stream.local_terrain_ready());
    assert!(stream.committed_view_cohort().is_none());

    let mut client_world = ClientWorld {
        stream: Some(stream),
        ..ClientWorld::default()
    };
    let runtime = UiRuntime::new(7);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let (network, _receiver) = NetworkHandle::<PackApplication>::with_command_capacity(1);
    let mut diagnostics = VisibilityDiagnosticsInput::default();
    let mut fatal_error = None;
    let mut observe = |client_world: &mut ClientWorld, frame_generation, render_work_drained| {
        prepare_loading(
            client_world,
            &mut fatal_error,
            &mut presentation,
            &runtime,
            &mut diagnostics,
            &network,
            LoadingObservation {
                restart: false,
                menu_visible: false,
                snapshot: VisibilityDiagnosticSnapshot {
                    frame_generation,
                    gpu_completed_opaque: Some(VisibilityKeyDigest { count: 0, hash: 0 }),
                    ..Default::default()
                },
                visible_rendered: 0,
                cohort: None,
                render_work_drained,
                pipelines_ready: true,
                now: Duration::ZERO,
            },
        );
        (
            presentation.loading_stage(),
            presentation.startup_mut().completion_queued,
        )
    };
    let loading = (Some(LoadingStage::BuildingTerrain), false);
    assert_eq!(observe(&mut client_world, 1, true), loading);

    client_world
        .stream
        .as_mut()
        .unwrap()
        .acknowledge_mesh_upload(
            key,
            identity.generation,
            identity.dirty_since,
            identity.dirty_since,
        );
    assert_eq!(observe(&mut client_world, 2, false), loading);
    assert_eq!(observe(&mut client_world, 3, true), loading);
    assert_eq!(observe(&mut client_world, 4, true), (None, true));
    assert_eq!(observe(&mut client_world, 5, true), (None, true));
    assert!(fatal_error.is_none());
}

/// BDS may leave distant ticking columns unannounced. A decoded, presented
/// destination still completes the matching loading-screen handshake promptly.
#[test]
fn dimension_loading_releases_local_terrain_without_distant_columns_or_a_timeout() {
    release_local_terrain(false, 8, true);
}

#[test]
fn dimension_loading_holds_presented_local_terrain_until_actor_pipelines_are_ready() {
    release_local_terrain(true, 8, true);
}

#[test]
fn dimension_loading_keeps_initial_completion_pending_when_the_queue_is_full() {
    // Start, switch acknowledgment and transfer End fill the queue. Session
    // initialization still needs its own accepted readiness command.
    release_local_terrain(false, 3, false);
}

/// Presents decoded destination terrain with a bounded, undrained command queue.
fn release_local_terrain(
    wait_for_actor_pipelines: bool,
    command_capacity: usize,
    completion_queued: bool,
) {
    let dimension = protocol::NETHER_DIMENSION_ID;
    let position = [0.0, 64.0, 0.0];
    let mut stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        dimension,
        local_player_runtime_id: 42,
        local_player_unique_id: 42,
        player_position: position,
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let slots = protocol::vanilla_dimension_range(dimension)
        .unwrap()
        .sub_chunk_count;
    let mut payload = vec![1, 0];
    payload.extend(std::iter::repeat_n(0xff, slots - 1));
    payload.push(0);
    let mut sequence = 1;
    for x in -1..=1 {
        for z in -1..=1 {
            stream
                .submit(
                    sequence,
                    WorldEvent::LevelChunk(LevelChunkEvent {
                        dimension,
                        x,
                        z,
                        mode: LevelChunkMode::LimitlessRequests,
                        payload: payload.clone(),
                    }),
                )
                .unwrap();
            sequence += 1;
        }
    }
    let mut entries = Vec::new();
    for x in -1..=1 {
        for y in 3..=5 {
            for z in -1..=1 {
                entries.push(SubChunkEntryEvent {
                    diagnostics: None,
                    position: [x, y, z],
                    result: SubChunkResult::AllAir,
                });
            }
        }
    }
    stream
        .submit(
            sequence,
            WorldEvent::SubChunks(SubChunkBatchEvent { dimension, entries }),
        )
        .unwrap();
    test_time::eventually("destination cells to decode", || {
        stream.poll(position, 128);
        stream.dimension_transfer_presentable(position)
    });
    assert!(stream.dimension_transfer_ready(position));
    assert!(!stream.dimension_loading_columns_ready());
    let mut client_world = ClientWorld {
        stream: Some(stream),
        ..ClientWorld::default()
    };
    let runtime = UiRuntime::new(7);
    let mut presentation =
        UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let (network, _receiver) =
        NetworkHandle::<PackApplication>::with_command_capacity(command_capacity);
    client_world.dimension_transfer.begin(
        runtime.session_id(),
        sequence + 1,
        protocol::ChangeDimensionEvent {
            dimension,
            position,
            loading_screen_id: Some(91),
            ..Default::default()
        },
        42,
        Duration::ZERO,
    );
    client_world.dimension_transfer.acknowledge(sequence + 1);
    client_world
        .dimension_transfer
        .queue_switch(Duration::ZERO, true, &network)
        .unwrap();
    client_world
        .dimension_transfer
        .queue_switch(Duration::ZERO, true, &network)
        .unwrap();
    let mut diagnostics = VisibilityDiagnosticsInput::default();
    let mut fatal_error = None;
    for frame_generation in 10..=if wait_for_actor_pipelines { 13 } else { 11 } {
        prepare_loading(
            &mut client_world,
            &mut fatal_error,
            &mut presentation,
            &runtime,
            &mut diagnostics,
            &network,
            LoadingObservation {
                restart: frame_generation == 10,
                menu_visible: false,
                snapshot: VisibilityDiagnosticSnapshot {
                    frame_generation,
                    gpu_completed_opaque: Some(VisibilityKeyDigest { count: 0, hash: 0 }),
                    ..Default::default()
                },
                visible_rendered: 0,
                cohort: None,
                render_work_drained: false,
                pipelines_ready: !wait_for_actor_pipelines || frame_generation >= 12,
                now: Duration::from_millis(200),
            },
        );
        assert_eq!(
            client_world.dimension_transfer.active(),
            frame_generation < if wait_for_actor_pipelines { 12 } else { 11 }
        );
        assert_eq!(
            presentation.loading_stage(),
            client_world
                .dimension_transfer
                .active()
                .then_some(LoadingStage::ChangingDimension)
        );
    }
    assert_eq!(
        presentation.startup_mut().completion_queued,
        completion_queued
    );
    assert!(fatal_error.is_none());
}
