use super::*;
use protocol::{
    LevelChunkEvent, LevelChunkMode, SubChunkBatchEvent, SubChunkEntryEvent, SubChunkResult,
    WorldBootstrap, WorldEvent,
};
use render_model::VisibilityKeyDigest;

/// BDS may leave distant ticking columns unannounced. A decoded, presented
/// destination still completes the matching loading-screen handshake promptly.
#[test]
fn dimension_loading_releases_local_terrain_without_distant_columns_or_a_timeout() {
    let dimension = protocol::NETHER_DIMENSION_ID;
    let position = [0.0, 64.0, 0.0];
    let mut stream = chunk_pipeline::WorldStream::new(WorldBootstrap {
        dimension,
        local_player_runtime_id: 42,
        local_player_unique_id: 42,
        player_position: position,
        world_spawn_position: [0, 64, 0],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
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
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !stream.dimension_transfer_presentable(position) {
        stream.poll(position, 128);
        assert!(
            std::time::Instant::now() < deadline,
            "destination cells did not decode"
        );
        std::thread::yield_now();
    }
    assert!(stream.dimension_transfer_ready(position));
    assert!(!stream.dimension_loading_columns_ready());
    let mut client_world = ClientWorld {
        stream: Some(stream),
        ..ClientWorld::default()
    };
    let runtime = UiRuntime::new(7);
    let mut presentation =
        UiPresentationRuntime::new(crate::ui_runtime::presentation::tests::fixture_font()).unwrap();
    let (network, _receiver) = NetworkHandle::with_command_capacity(8);
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
        .queue_switch(&network)
        .unwrap();
    let mut diagnostics = VisibilityDiagnosticsInput::default();
    for frame_generation in [10, 11] {
        prepare_loading(
            &mut client_world,
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
                now: Duration::from_millis(200),
            },
        );
        assert_eq!(
            client_world.dimension_transfer.active(),
            frame_generation == 10
        );
    }
    assert!(presentation.startup_mut().completion_queued);
    assert!(client_world.fatal_error.is_none());
}
