use super::*;

fn lenient_test_stream() -> WorldStream {
    WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    })
}

fn inline_event(x: i32, count: usize, payload: Vec<u8>) -> WorldEvent {
    WorldEvent::LevelChunk(LevelChunkEvent {
        dimension: 0,
        x,
        z: 0,
        mode: LevelChunkMode::Inline { count },
        payload,
    })
}

/// One v9 sub-chunk at `y` holding a single uniform block.
fn uniform_v9(y: i8, runtime_id: u32) -> Vec<u8> {
    let mut bytes = vec![9, 1, y as u8, 1];
    bytes.extend(zig_zag_i32(runtime_id as i32));
    bytes
}

#[test]
fn malformed_chunk_content_never_stalls_or_fatals_the_stream() {
    let mut stream = lenient_test_stream();
    stream.submit(1, inline_event(0, 1, vec![0xff])).unwrap();
    stream
        .submit_level_chunk_bytes(
            2,
            LevelChunkEvent {
                dimension: 0,
                x: 1,
                z: 0,
                mode: LevelChunkMode::Inline { count: 3 },
                payload: Vec::new(),
            },
            bytes::Bytes::from_static(&[9, 2, 0xfc, 0x0d]),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::LevelChunk(LevelChunkEvent {
                dimension: 0,
                x: 2,
                z: 0,
                mode: LevelChunkMode::LimitedRequests { highest: 1 },
                payload: vec![0x0b],
            }),
        )
        .unwrap();
    stream
        .submit(4, WorldEvent::SetTime(SetTimeEvent { time: 7 }))
        .unwrap();

    complete_pending_decode_jobs(&mut stream);

    assert!(stream.take_fatal_error().is_none());
    assert_eq!(stream.stats().decode_errors, 0);
    assert!(matches!(
        stream.take_committed_controls().as_slice(),
        [CommittedControlEvent::SetTime { sequence: 4, .. }]
    ));
    // A truncated storage still stores its slot; everything unsent lights as air.
    for (x, first_air) in [(0, -4), (1, -3)] {
        let chunk = ChunkKey::new(0, x, 0);
        assert!(stream.loaded_columns.contains(&chunk));
        assert_eq!(
            stream
                .authority
                .terrain()
                .sub_chunk(SubChunkKey::from_chunk(chunk, -4))
                .is_some(),
            first_air == -3
        );
        assert!((first_air..20).all(|y| {
            stream
                .known_air
                .contains(&SubChunkKey::from_chunk(chunk, y))
        }));
    }
    assert_eq!(stream.take_requests().len(), 1);
}

#[test]
fn malformed_sub_chunk_payload_completes_without_retry() {
    let (mut stream, keys, _) = stream_with_unsent_sub_chunks(2);
    stream
        .submit(
            2,
            WorldEvent::SubChunks(SubChunkBatchEvent {
                dimension: 0,
                entries: vec![
                    SubChunkEntryEvent {
                        position: [keys[0].x, keys[0].y, keys[0].z],
                        result: SubChunkResult::AllAir,
                    },
                    SubChunkEntryEvent {
                        position: [keys[1].x, keys[1].y, keys[1].z],
                        result: SubChunkResult::Success {
                            payload: vec![0xff],
                        },
                    },
                ],
            }),
        )
        .unwrap();

    complete_pending_decode_jobs(&mut stream);

    assert!(stream.take_fatal_error().is_none());
    assert!(stream.take_requests().is_empty());
    assert!(stream.loaded_columns.contains(&keys[0].chunk()));
    assert!(keys.iter().all(|key| stream.known_air.contains(key)));
}

#[test]
fn malformed_live_block_entity_nbt_is_counted_not_fatal() {
    for nbt in [
        vec![10, 1, 0xff],
        vec![10, 1],
        vec![10, 0, 3, 2, b'i', b'd'],
        vec![10, 0, 3, 2, b'i', b'd', 0, 0],
        vec![1, 0, 0],
    ] {
        let mut stream = lenient_test_stream();
        stream
            .submit(
                1,
                WorldEvent::BlockEntityUpdate(BlockEntityUpdateEvent {
                    dimension: 0,
                    position: [0, 0, 0],
                    nbt,
                }),
            )
            .unwrap();
        stream
            .submit(2, WorldEvent::SetTime(SetTimeEvent { time: 7 }))
            .unwrap();

        complete_pending_decode_jobs(&mut stream);

        assert!(stream.take_fatal_error().is_none());
        assert_eq!(stream.stats().decode_errors, 1);
        assert!(matches!(
            stream.take_committed_controls().as_slice(),
            [CommittedControlEvent::SetTime { sequence: 2, .. }]
        ));
    }
}

#[test]
fn inline_level_chunk_records_every_unsent_slot_as_known_air() {
    let mut stream = lenient_test_stream();
    let chunk = ChunkKey::new(0, 0, 0);
    let mut payload = uniform_v9(-4, 0);
    payload.extend(biome_payload(0, 1));
    stream.submit(1, inline_event(0, 1, payload)).unwrap();

    complete_pending_decode_jobs(&mut stream);

    let stored = SubChunkKey::from_chunk(chunk, -4);
    assert!(stream.authority.terrain().sub_chunk(stored).is_some());
    assert!(!stream.known_air.contains(&stored));
    for y in -3..20 {
        let key = SubChunkKey::from_chunk(chunk, y);
        assert!(
            stream.known_air.contains(&key),
            "slot {y} must light as air"
        );
        assert!(stream.resident.contains(&key));
    }
}

#[test]
fn inline_count_above_dimension_slots_still_loads_the_column() {
    let mut stream = lenient_test_stream();
    define_custom_biomes(&mut stream, [1]);
    let chunk = ChunkKey::new(0, 0, 0);
    let mut payload = uniform_v9(-4, 0);
    for y in -3_i8..20 {
        payload.extend([9, 0, y as u8]);
    }
    // Reads past the 24 overworld slots consume no bytes, so biomes follow.
    payload.extend(biome_payload(0, 1));
    stream.submit(1, inline_event(0, 30, payload)).unwrap();

    complete_pending_decode_jobs(&mut stream);

    assert!(stream.loaded_columns.contains(&chunk));
    assert_eq!(stream.stats().normalization_errors, 0);
    let stored = SubChunkKey::from_chunk(chunk, -4);
    assert!(stream.authority.terrain().sub_chunk(stored).is_some());
    assert_eq!(
        stream.authority.terrain().biome_id(stored, 0, 0, 0),
        Some(1)
    );
    assert!(stream.take_fatal_error().is_none());
}

#[test]
fn block_update_with_unknown_runtime_id_applies_air() {
    let mut stream = WorldStream::new_with_assets(
        WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 2,
            block_network_ids_are_hashes: false,
        },
        Arc::new(super::non_default_air_runtime_assets()),
        [0.0, crate::server_position::SAFE_SERVER_HEIGHT, 0.0],
        None,
    );
    let key = SubChunkKey::new(0, 0, -4, 0);
    let mut payload = uniform_v9(-4, 0);
    payload.extend(biome_payload(0, 1));
    stream.submit(1, inline_event(0, 1, payload)).unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .authority
            .terrain()
            .sub_chunk(key)
            .unwrap()
            .runtime_id(0, 0, 0, 0),
        Some(0)
    );

    stream
        .submit(
            2,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [0, -64, 0],
                layer: 0,
                network_id: 999,
            }]),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);

    let sub_chunk = stream.authority.terrain().sub_chunk(key).unwrap();
    assert_eq!(sub_chunk.runtime_id(0, 0, 0, 0), Some(2));
    assert_eq!(sub_chunk.runtime_id(0, 1, 0, 0), Some(0));

    // Server-defined block ids after the vanilla palette stay themselves.
    stream.set_custom_block_ids(3..5);
    stream
        .submit(
            3,
            WorldEvent::BlockUpdates(vec![BlockUpdateEvent {
                dimension: 0,
                position: [0, -63, 0],
                layer: 0,
                network_id: 4,
            }]),
        )
        .unwrap();
    complete_pending_decode_jobs(&mut stream);
    assert_eq!(
        stream
            .authority
            .terrain()
            .sub_chunk(key)
            .unwrap()
            .runtime_id(0, 0, 1, 0),
        Some(4)
    );
}
