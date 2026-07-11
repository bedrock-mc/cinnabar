use bytes::{Buf, BytesMut};
use protocol::{
    BiomeDefinitionEvent, BiomeDefinitionsEvent, DimensionRange, GameData, HASHED_AIR_NETWORK_ID,
    LevelChunkMode, MAX_BIOME_DEFINITIONS, MAX_BIOME_NAME_BYTES, MovePlayerEvent,
    SEQUENTIAL_AIR_NETWORK_ID, SubChunkResult, WorldBootstrap, WorldEvent, WorldPacketError,
    air_network_id, into_world_event, request_sub_chunk_column, vanilla_dimension_range,
};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::version::v1_26_30::{
    BiomeDefinition, BiomeDefinitionListPacket, BlockCoordinates, BlockUpdate,
    BlockUpdateTransitionType, ChangeDimensionPacket, ChunkRadiusUpdatePacket, LevelChunkPacket,
    McpePacketData, MovePlayerPacket, NetworkChunkPublisherUpdatePacket, StartGamePacketDimension,
    SubChunkEntryWithCachingItem, SubChunkEntryWithCachingItemResult,
    SubChunkEntryWithoutCachingItem, SubChunkEntryWithoutCachingItemResult, SubchunkPacket,
    SubchunkPacketEntries, UpdateBlockFlags, UpdateBlockPacket, UpdateSubchunkBlocksPacket, Vec3F,
    Vec3I,
};

fn biome_definition(name_index: i16, biome_id: i16) -> BiomeDefinition {
    BiomeDefinition {
        name_index,
        biome_id: biome_id as u16,
        temperature: 0.8,
        downfall: 0.4,
        snow_foliage: 0.125,
        map_water_colour: 0xff11_2233_u32 as i32,
        ..Default::default()
    }
}

#[test]
fn biome_definition_ids_preserve_the_u16_wire_contract() {
    let packet = BiomeDefinitionListPacket {
        biome_definitions: vec![
            BiomeDefinition {
                name_index: 0,
                biome_id: u16::MAX,
                ..biome_definition(0, 0)
            },
            BiomeDefinition {
                name_index: 1,
                biome_id: 0xfffe,
                ..biome_definition(0, 0)
            },
            BiomeDefinition {
                name_index: 2,
                biome_id: 600,
                ..biome_definition(0, 0)
            },
        ],
        string_list: vec![
            "plains".into(),
            "custom:high".into(),
            "custom:normal".into(),
        ],
    };

    let WorldEvent::BiomeDefinitions(event) = into_world_event(packet.into(), 0).unwrap().unwrap()
    else {
        panic!("expected biome definitions")
    };
    assert_eq!(event.definitions[0].biome_id, None);
    assert_eq!(event.definitions[1].biome_id, Some(0xfffe));
    assert_eq!(event.definitions[2].biome_id, Some(600));
}

#[test]
fn normalizes_live_biomes_by_name_without_synthesizing_packet_order_ids() {
    let packet = BiomeDefinitionListPacket {
        biome_definitions: vec![biome_definition(1, -1), biome_definition(0, 600)],
        string_list: vec!["violet_marsh".into(), "plains".into()],
    };

    let event = into_world_event(packet.into(), 0).unwrap().unwrap();
    assert_eq!(
        event,
        WorldEvent::BiomeDefinitions(BiomeDefinitionsEvent {
            definitions: vec![
                BiomeDefinitionEvent {
                    biome_id: None,
                    name: "minecraft:plains".into(),
                    temperature: 0.8,
                    downfall: 0.4,
                    snow_foliage: 0.125,
                    map_water_color: 0xff11_2233,
                },
                BiomeDefinitionEvent {
                    biome_id: Some(600),
                    name: "violet_marsh".into(),
                    temperature: 0.8,
                    downfall: 0.4,
                    snow_foliage: 0.125,
                    map_water_color: 0xff11_2233,
                },
            ]
            .into(),
        })
    );
    let WorldEvent::BiomeDefinitions(event) = event else {
        unreachable!("equality above proves the event variant")
    };
    assert_eq!(
        event
            .definitions
            .iter()
            .map(|definition| definition.biome_id)
            .collect::<Vec<_>>(),
        [None, Some(600)],
        "random definition packet order/name_index must not become palette IDs"
    );
}

#[test]
fn rejects_invalid_or_unbounded_live_biome_definitions() {
    let invalid_index = BiomeDefinitionListPacket {
        biome_definitions: vec![biome_definition(-1, 1)],
        string_list: vec!["minecraft:plains".into()],
    };
    assert_eq!(
        into_world_event(invalid_index.into(), 0).unwrap_err(),
        WorldPacketError::InvalidBiomeNameIndex {
            index: -1,
            string_count: 1,
        }
    );

    let long_name = BiomeDefinitionListPacket {
        biome_definitions: vec![biome_definition(0, 1)],
        string_list: vec!["x".repeat(MAX_BIOME_NAME_BYTES + 1)],
    };
    assert_eq!(
        into_world_event(long_name.into(), 0).unwrap_err(),
        WorldPacketError::BiomeNameTooLong {
            bytes: MAX_BIOME_NAME_BYTES + 1,
            max: MAX_BIOME_NAME_BYTES,
        }
    );

    let mut non_finite = biome_definition(0, 1);
    non_finite.downfall = f32::NAN;
    let non_finite = BiomeDefinitionListPacket {
        biome_definitions: vec![non_finite],
        string_list: vec!["minecraft:plains".into()],
    };
    assert_eq!(
        into_world_event(non_finite.into(), 0).unwrap_err(),
        WorldPacketError::NonFiniteBiomeClimate {
            definition: 0,
            field: "downfall",
        }
    );

    let oversized = BiomeDefinitionListPacket {
        biome_definitions: vec![biome_definition(0, 1); MAX_BIOME_DEFINITIONS + 1],
        string_list: vec!["minecraft:plains".into()],
    };
    assert_eq!(
        into_world_event(oversized.into(), 0).unwrap_err(),
        WorldPacketError::TooManyBiomeDefinitions {
            count: MAX_BIOME_DEFINITIONS + 1,
            max: MAX_BIOME_DEFINITIONS,
        }
    );
}

#[test]
fn chooses_air_value_from_start_game_hash_mode() {
    assert_eq!(air_network_id(false), SEQUENTIAL_AIR_NETWORK_ID);
    assert_eq!(SEQUENTIAL_AIR_NETWORK_ID, 12_530);
    assert_eq!(air_network_id(true), HASHED_AIR_NETWORK_ID);
    assert_eq!(HASHED_AIR_NETWORK_ID, 0xdbf4_4120);
}

#[test]
fn normalizes_start_game_bootstrap_without_generated_types() {
    let mut game_data = GameData {
        start_game: Default::default(),
        item_registry: Default::default(),
        biome_definitions: None,
        entity_identifiers: None,
        creative_content: None,
    };
    game_data.start_game.dimension = StartGamePacketDimension::Nether;
    game_data.start_game.runtime_entity_id = 0x1_0000_0001;
    game_data.start_game.player_position = Vec3F {
        x: 1.25,
        y: 72.0,
        z: -8.5,
    };
    game_data.start_game.spawn_position = BlockCoordinates {
        x: -104,
        y: 114,
        z: 61,
    };
    game_data.start_game.block_network_ids_are_hashes = true;

    assert_eq!(
        WorldBootstrap::from_game_data(&game_data),
        WorldBootstrap {
            dimension: 1,
            local_player_runtime_id: 0x1_0000_0001,
            player_position: [1.25, 72.0, -8.5],
            world_spawn_position: [-104, 114, 61],
            air_network_id: HASHED_AIR_NETWORK_ID,
            block_network_ids_are_hashes: true,
        }
    );
}

#[test]
fn normalizes_move_player_to_the_bounded_world_surface() {
    let packet = MovePlayerPacket {
        runtime_id: 73,
        position: Vec3F {
            x: -12.25,
            y: 65.5,
            z: 4096.75,
        },
        pitch: -34.5,
        yaw: 271.25,
        head_yaw: 99.0,
        ..Default::default()
    };

    assert_eq!(
        into_world_event(packet.into(), 2).unwrap(),
        Some(WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 73,
            position: [-12.25, 65.5, 4096.75],
            pitch: -34.5,
            yaw: 271.25,
        }))
    );
}

#[test]
fn move_player_uses_varuint64_for_runtime_and_ridden_ids_above_u32() {
    const RUNTIME_ID: u64 = 0x1_0000_0001;
    const RIDDEN_RUNTIME_ID: u64 = 0x2_0000_0002;
    let packet = MovePlayerPacket {
        runtime_id: RUNTIME_ID,
        position: Vec3F {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        ridden_runtime_id: RIDDEN_RUNTIME_ID,
        ..Default::default()
    };
    let mut encoded = BytesMut::new();
    packet.encode(&mut encoded).unwrap();

    assert_eq!(&encoded[..5], &[0x81, 0x80, 0x80, 0x80, 0x10]);
    assert_eq!(&encoded[31..36], &[0x82, 0x80, 0x80, 0x80, 0x20]);
    assert_eq!(packet.encoded_size(), encoded.len());

    let mut encoded = encoded.freeze();
    let decoded = MovePlayerPacket::decode(&mut encoded, ()).unwrap();
    assert_eq!(decoded.runtime_id, RUNTIME_ID);
    assert_eq!(decoded.ridden_runtime_id, RIDDEN_RUNTIME_ID);
    assert!(!encoded.has_remaining());

    assert_eq!(
        into_world_event(decoded.into(), 0).unwrap(),
        Some(WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: RUNTIME_ID,
            position: [1.0, 2.0, 3.0],
            pitch: 0.0,
            yaw: 0.0,
        }))
    );
}

#[test]
fn move_player_rejects_overflowing_runtime_and_ridden_varuint64_fields() {
    let packet = MovePlayerPacket::default();
    let mut valid = BytesMut::new();
    packet.encode(&mut valid).unwrap();
    let overflow = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02];

    let mut malformed_runtime = BytesMut::new();
    malformed_runtime.extend_from_slice(&overflow);
    malformed_runtime.extend_from_slice(&valid[1..]);
    assert!(MovePlayerPacket::decode(&mut malformed_runtime.freeze(), ()).is_err());

    let ridden_offset = 1 + 12 + 12 + 1 + 1;
    let mut malformed_ridden = BytesMut::new();
    malformed_ridden.extend_from_slice(&valid[..ridden_offset]);
    malformed_ridden.extend_from_slice(&overflow);
    malformed_ridden.extend_from_slice(&valid[ridden_offset + 1..]);
    assert!(MovePlayerPacket::decode(&mut malformed_ridden.freeze(), ()).is_err());
}

#[test]
fn exposes_vanilla_dimension_subchunk_ranges() {
    assert_eq!(
        vanilla_dimension_range(0),
        Some(DimensionRange {
            base_sub_chunk_y: -4,
            sub_chunk_count: 24,
        })
    );
    assert_eq!(
        vanilla_dimension_range(1),
        Some(DimensionRange {
            base_sub_chunk_y: 0,
            sub_chunk_count: 8,
        })
    );
    assert_eq!(
        vanilla_dimension_range(2),
        Some(DimensionRange {
            base_sub_chunk_y: 0,
            sub_chunk_count: 16,
        })
    );
    assert_eq!(vanilla_dimension_range(42), None);
}

#[test]
fn normalizes_inline_and_request_mode_level_chunks() {
    let inline = LevelChunkPacket {
        x: -2,
        z: 7,
        dimension: 0,
        sub_chunk_count: 3,
        payload: vec![1, 2, 3],
        ..Default::default()
    };
    let event = into_world_event(inline.into(), 0).unwrap().unwrap();
    let WorldEvent::LevelChunk(event) = event else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.x, -2);
    assert_eq!(event.z, 7);
    assert_eq!(event.dimension, 0);
    assert_eq!(event.mode, LevelChunkMode::Inline { count: 3 });
    assert_eq!(event.payload, vec![1, 2, 3]);

    let limited = LevelChunkPacket {
        x: 1,
        z: 2,
        dimension: 1,
        sub_chunk_count: -2,
        highest_subchunk_count: Some(8),
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) = into_world_event(limited.into(), 0).unwrap().unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.mode, LevelChunkMode::LimitedRequests { highest: 8 });

    let limitless = LevelChunkPacket {
        sub_chunk_count: -1,
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) = into_world_event(limitless.into(), 0).unwrap().unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.mode, LevelChunkMode::LimitlessRequests);
}

#[test]
fn rejects_malformed_or_cached_level_chunks() {
    let malformed = LevelChunkPacket {
        sub_chunk_count: -3,
        ..Default::default()
    };
    assert_eq!(
        into_world_event(malformed.into(), 0),
        Err(WorldPacketError::InvalidSubChunkCount(-3))
    );

    let missing_highest = LevelChunkPacket {
        sub_chunk_count: -2,
        highest_subchunk_count: None,
        ..Default::default()
    };
    assert_eq!(
        into_world_event(missing_highest.into(), 0),
        Err(WorldPacketError::MissingHighestSubChunk)
    );

    use valentine::bedrock::version::v1_26_30::LevelChunkPacketBlobs;
    let cached = LevelChunkPacket {
        sub_chunk_count: 1,
        blobs: Some(LevelChunkPacketBlobs::default()),
        ..Default::default()
    };
    assert_eq!(
        into_world_event(cached.into(), 0),
        Err(WorldPacketError::CachedChunksUnsupported)
    );

    let taller_than_overworld = LevelChunkPacket {
        dimension: 0,
        sub_chunk_count: 25,
        ..Default::default()
    };
    assert_eq!(
        into_world_event(taller_than_overworld.into(), 0),
        Err(WorldPacketError::InlineSubChunkCountExceedsDimension {
            dimension: 0,
            count: 25,
            max: 24,
        })
    );
}

#[test]
fn resolves_non_cached_sub_chunk_entries_to_absolute_keys() {
    let packet = SubchunkPacket {
        dimension: 2,
        origin: Vec3I {
            x: 10,
            y: -4,
            z: -8,
        },
        entries: SubchunkPacketEntries::SubChunkEntryWithoutCaching(vec![
            SubChunkEntryWithoutCachingItem {
                dx: -2,
                dy: 3,
                dz: 4,
                result: SubChunkEntryWithoutCachingItemResult::Success,
                payload: vec![9, 0, 0xff],
                ..Default::default()
            },
            SubChunkEntryWithoutCachingItem {
                dx: 0,
                dy: 1,
                dz: 0,
                result: SubChunkEntryWithoutCachingItemResult::SuccessAllAir,
                payload: vec![],
                ..Default::default()
            },
            SubChunkEntryWithoutCachingItem {
                dx: 1,
                dy: 0,
                dz: 0,
                result: SubChunkEntryWithoutCachingItemResult::ChunkNotFound,
                payload: vec![],
                ..Default::default()
            },
        ]),
    };

    let WorldEvent::SubChunks(batch) = into_world_event(packet.into(), 0).unwrap().unwrap() else {
        panic!("expected SubChunks event")
    };
    assert_eq!(batch.dimension, 2);
    assert_eq!(batch.entries[0].position, [8, -1, -4]);
    assert_eq!(
        batch.entries[0].result,
        SubChunkResult::Success {
            payload: vec![9, 0, 0xff]
        }
    );
    assert_eq!(batch.entries[1].result, SubChunkResult::AllAir);
    assert!(matches!(
        batch.entries[2].result,
        SubChunkResult::Unavailable(_)
    ));
}

#[test]
fn rejects_cached_sub_chunks_and_checked_origin_overflow() {
    let cached = SubchunkPacket {
        entries: SubchunkPacketEntries::SubChunkEntryWithCaching(vec![
            SubChunkEntryWithCachingItem {
                result: SubChunkEntryWithCachingItemResult::Success,
                payload: Some(vec![9, 0, 0]),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };
    assert_eq!(
        into_world_event(cached.into(), 0),
        Err(WorldPacketError::CachedChunksUnsupported)
    );

    let overflow = SubchunkPacket {
        origin: Vec3I {
            x: i32::MAX,
            y: 0,
            z: 0,
        },
        entries: SubchunkPacketEntries::SubChunkEntryWithoutCaching(vec![
            SubChunkEntryWithoutCachingItem {
                dx: 1,
                result: SubChunkEntryWithoutCachingItemResult::SuccessAllAir,
                ..Default::default()
            },
        ]),
        ..Default::default()
    };
    assert_eq!(
        into_world_event(overflow.into(), 0),
        Err(WorldPacketError::SubChunkPositionOverflow {
            origin: [i32::MAX, 0, 0],
            offset: [1, 0, 0],
        })
    );
}

#[test]
fn normalizes_single_and_batched_block_updates_with_layers() {
    let single = UpdateBlockPacket {
        position: BlockCoordinates {
            x: 31,
            y: -1,
            z: -17,
        },
        block_runtime_id: 0xdead_beef_u32 as i32,
        flags: UpdateBlockFlags::default(),
        layer: 1,
    };
    let WorldEvent::BlockUpdates(updates) = into_world_event(single.into(), 2).unwrap().unwrap()
    else {
        panic!("expected BlockUpdates event")
    };
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].dimension, 2);
    assert_eq!(updates[0].position, [31, -1, -17]);
    assert_eq!(updates[0].layer, 1);
    assert_eq!(updates[0].network_id, 0xdead_beef);

    let entry = |x, y, z, runtime_id| BlockUpdate {
        position: BlockCoordinates { x, y, z },
        runtime_id,
        flags: 0,
        entity_unique_id: 0,
        transition_type: BlockUpdateTransitionType::Entity,
    };
    let batch = UpdateSubchunkBlocksPacket {
        x: 1,
        y: -4,
        z: -2,
        blocks: vec![entry(16, -64, -32, 4)],
        extra: vec![entry(17, -63, -31, 5)],
    };
    let WorldEvent::BlockUpdates(updates) = into_world_event(batch.into(), 0).unwrap().unwrap()
    else {
        panic!("expected BlockUpdates event")
    };
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].layer, 0);
    assert_eq!(updates[0].network_id, 4);
    assert_eq!(updates[1].layer, 1);
    assert_eq!(updates[1].network_id, 5);
}

#[test]
fn rejects_negative_or_excessive_update_layers() {
    for layer in [-1, 16] {
        let packet = UpdateBlockPacket {
            layer,
            ..Default::default()
        };
        assert_eq!(
            into_world_event(packet.into(), 0),
            Err(WorldPacketError::InvalidBlockLayer(layer))
        );
    }
}

#[test]
fn normalizes_streaming_radius_publisher_and_dimension_events() {
    let WorldEvent::ChunkRadiusUpdated(radius) =
        into_world_event(ChunkRadiusUpdatePacket { chunk_radius: 16 }.into(), 0)
            .unwrap()
            .unwrap()
    else {
        panic!("expected radius event")
    };
    assert_eq!(radius, 16);

    let publisher = NetworkChunkPublisherUpdatePacket {
        coordinates: BlockCoordinates {
            x: 32,
            y: 70,
            z: -48,
        },
        radius: 256,
        ..Default::default()
    };
    let WorldEvent::PublisherUpdate(update) =
        into_world_event(publisher.into(), 0).unwrap().unwrap()
    else {
        panic!("expected publisher event")
    };
    assert_eq!(update.center, [32, 70, -48]);
    assert_eq!(update.radius_blocks, 256);

    let dimension = ChangeDimensionPacket {
        dimension: 1,
        position: Vec3F {
            x: 1.5,
            y: 80.0,
            z: -2.5,
        },
        ..Default::default()
    };
    let WorldEvent::ChangeDimension(change) =
        into_world_event(dimension.into(), 0).unwrap().unwrap()
    else {
        panic!("expected dimension event")
    };
    assert_eq!(change.dimension, 1);
    assert_eq!(change.position, [1.5, 80.0, -2.5]);
}

#[test]
fn ignores_packets_without_world_state() {
    let packet = valentine::bedrock::version::v1_26_30::SetTimePacket { time: 6000 };
    assert_eq!(into_world_event(packet.into(), 0).unwrap(), None);
}

#[test]
fn builds_bounded_column_sub_chunk_requests() {
    let packet = request_sub_chunk_column(0, 12, -8, -4, 3).unwrap();
    let McpePacketData::PacketSubchunkRequest(request) = packet.data else {
        panic!("expected SubchunkRequest packet")
    };
    assert_eq!(request.dimension, 0);
    assert_eq!(
        [request.origin.x, request.origin.y, request.origin.z],
        [12, -4, -8]
    );
    assert_eq!(request.requests.len(), 3);
    assert_eq!(
        request
            .requests
            .iter()
            .map(|offset| [offset.x, offset.y, offset.z])
            .collect::<Vec<_>>(),
        vec![[0, 0, 0], [0, 1, 0], [0, 2, 0]]
    );

    assert_eq!(
        request_sub_chunk_column(0, 0, 0, 0, 129),
        Err(WorldPacketError::TooManySubChunkRequests {
            count: 129,
            max: 128,
        })
    );
    assert_eq!(
        request_sub_chunk_column(0, 0, 0, i32::MAX, 2),
        Err(WorldPacketError::SubChunkRequestYOverflow {
            base_y: i32::MAX,
            offset: 1,
        })
    );
}
