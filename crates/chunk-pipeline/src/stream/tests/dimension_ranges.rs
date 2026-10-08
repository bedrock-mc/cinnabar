use super::*;

fn server_height_stream() -> WorldStream {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: RAW_IDS.air,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::DimensionHeights(vec![protocol::DimensionHeightDiagnostic {
                name: Arc::from("minecraft:overworld"),
                dimension: 3,
                minimum_y: 0,
                height_range: 256,
                generator: 1,
            }]),
        )
        .unwrap();
    stream
}

#[test]
fn advertised_height_places_inline_terrain_at_the_server_origin() {
    let mut stream = server_height_stream();
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: vec![8, 1, 1, 14],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let key = SubChunkKey::new(0, 0, 0, 0);
    assert_eq!(
        stream
            .authority
            .terrain()
            .sub_chunk(key)
            .and_then(|chunk| chunk.runtime_id(0, 0, 0, 0)),
        Some(7)
    );
    assert!(
        stream
            .authority
            .terrain()
            .sub_chunk(SubChunkKey::new(0, 0, -4, 0))
            .is_none()
    );
    assert_eq!(stream.top_non_air_block_y(0, 0), Some(15));
    assert_eq!(stream.light_column_top_sub_chunk_y(key), Some(15));
}

#[test]
fn advertised_height_controls_subchunk_requests() {
    let mut stream = server_height_stream();
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 2 },
                payload: Vec::new(),
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert!(stream.requests.is_expected(SubChunkKey::new(0, 0, 0, 0)));
    assert!(stream.requests.is_expected(SubChunkKey::new(0, 0, 1, 0)));
    assert!(!stream.requests.is_expected(SubChunkKey::new(0, 0, -4, 0)));
}

#[test]
fn definitions_waiting_behind_decode_do_not_shift_admitted_columns() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: RAW_IDS.air,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: vec![8, 1, 1, 14],
            }),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::DimensionHeights(vec![protocol::DimensionHeightDiagnostic {
                name: Arc::from("minecraft:overworld"),
                dimension: 3,
                minimum_y: 0,
                height_range: 256,
                generator: 1,
            }]),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 1,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: vec![8, 1, 1, 14],
            }),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    let base = vanilla_dimension_range(0).unwrap().base_sub_chunk_y;
    for x in [0, 1] {
        assert!(
            stream
                .authority
                .terrain()
                .sub_chunk(SubChunkKey::new(0, x, base, 0))
                .is_some()
        );
        assert!(
            stream
                .authority
                .terrain()
                .sub_chunk(SubChunkKey::new(0, x, 0, 0))
                .is_none()
        );
    }
}

#[test]
fn new_dimension_definition_is_available_to_later_decode_while_commits_wait() {
    let mut stream = server_height_stream();
    stream
        .submit(
            2,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 0 },
                payload: Vec::new(),
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::DimensionHeights(vec![protocol::DimensionHeightDiagnostic {
                name: Arc::from("example:dimension"),
                dimension: 1000,
                minimum_y: 0,
                height_range: 256,
                generator: 1,
            }]),
        )
        .unwrap();
    stream
        .submit(
            4,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 1000,
                x: 0,
                z: 0,
                mode: LevelChunkMode::Inline { count: 1 },
                payload: vec![8, 1, 1, 14],
            }),
        )
        .unwrap();
    let Some(QueuedDecodeJob {
        job: DecodeJob::InlineLevelChunk { slots, .. },
        ..
    }) = stream.pending_decode.back()
    else {
        panic!("declared dimension must retain its decode job");
    };
    assert_eq!(slots.base_sub_chunk_y, 0);
    assert_eq!(slots.count, 16);
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .stats
            .normalization_reasons
            .unsupported_level_chunk_dimensions,
        0
    );
}

#[test]
fn synced_block_admission_freezes_height_before_later_definitions() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: RAW_IDS.air,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::SyncedBlockUpdates(vec![protocol::SyncedBlockUpdateEvent {
                update: BlockUpdateEvent {
                    dimension: 0,
                    position: [0, -64, 0],
                    layer: 0,
                    network_id: RAW_IDS.air,
                },
                flags: 0,
                sync: protocol::ActorBlockSyncMessage {
                    actor_unique_id: -1,
                    message: 0,
                },
            }]),
        )
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::DimensionHeights(vec![protocol::DimensionHeightDiagnostic {
                name: Arc::from("minecraft:overworld"),
                dimension: 3,
                minimum_y: 0,
                height_range: 256,
                generator: 1,
            }]),
        )
        .unwrap();
    assert_eq!(stream.dimension_range(0), vanilla_dimension_range(0));
}
