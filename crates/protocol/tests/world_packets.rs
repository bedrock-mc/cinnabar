use bytes::{Buf, BytesMut};
use protocol::{
    BiomeDefinitionEvent, BiomeDefinitionsEvent, DaylightCycleUpdateEvent, DimensionRange,
    GameData, HASHED_AIR_NETWORK_ID, LevelChunkMode, MAX_BIOME_DEFINITIONS, MAX_BIOME_NAME_BYTES,
    MAX_SUB_CHUNK_REQUESTS, MovePlayerEvent, PlayerMovementCorrectionEvent,
    SEQUENTIAL_AIR_NETWORK_ID, SetTimeEvent, SubChunkResult, WeatherChannel, WeatherUpdateEvent,
    WorldBootstrap, WorldEnvironmentBootstrap, WorldEvent, WorldPacketError, air_network_id,
    into_world_event, request_sub_chunk_column, vanilla_dimension_range,
};
use valentine::bedrock::codec::{BedrockCodec, BedrockSized};
use valentine::bedrock::version::v1_26_40::{
    ActorRuntimeId, BiomeDefinitionData, BiomeDefinitionListPacket,
    BiomeDefinitionListPacketMapofBiomenamestodataItem, BiomeStringList, BlockPos, ChangeDimensionPacket,
    ChunkPos, ChunkRadiusUpdatedPacket, CorrectPlayerMovePredictionPacket,
    CorrectPlayerMovePredictionPacketPredictionType, DimensionType, GameRule,
    GameRuleRuleValue, GameRulesChangedPacket, GameRulesChangedPacketData, LevelChunkPacket,
    LevelEventPacket, McpePacketData, MovePlayerPacket, MovePlayerPacketPositionMode,
    NetworkChunkPublisherUpdatePacket, PlayerInputTick, RespawnPacket, RespawnPacketState,
    SetTimePacket, SubChunkPacket, SubChunkPacketPayloadSubChunkPacketData,
    SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult,
    SubChunkPacketPayloadSubChunkPosOffset, SubChunkPos, UpdateBlockPacket,
    UpdateSubChunkBlocksChangedInfo, UpdateSubChunkBlocksPacket, UpdateSubChunkNetworkBlockInfo,
    Vec2, Vec3,
};

/// `LevelEventStartRaining`, from gophertunnel `packet/level_event.go`
/// @ be6713da4dc051a4197f897d04835e89e9c54321.
const LEVEL_EVENT_START_RAINING: i32 = 3001;
/// `LevelEventStartThunderstorm`.
const LEVEL_EVENT_START_THUNDERSTORM: i32 = 3002;
/// `LevelEventStopRaining`.
const LEVEL_EVENT_STOP_RAINING: i32 = 3003;
/// `LevelEventStopThunderstorm`.
const LEVEL_EVENT_STOP_THUNDERSTORM: i32 = 3004;
/// `LevelEventSoundClick`, an event this crate deliberately ignores.
const LEVEL_EVENT_SOUND_CLICK: i32 = 1000;

fn biome_definition(
    name_index: i16,
    biome_id: i16,
) -> BiomeDefinitionListPacketMapofBiomenamestodataItem {
    // 1.26.40 splits the entry into the string-table index (`key`) and the
    // definition payload (`value`). gophertunnel protocol/biome.go still writes
    // Int16 NameIndex then Int16 BiomeID, so `key` carries the same two bytes.
    BiomeDefinitionListPacketMapofBiomenamestodataItem {
        key: name_index as u16,
        value: BiomeDefinitionData {
            id: biome_id as u16,
            temperature: 0.8,
            downfall: 0.4,
            foliagesnow: 0.125,
            mapwatercolor_argb: 0xff11_2233_u32 as i32,
            ..Default::default()
        },
    }
}

fn biome_packet(
    definitions: Vec<BiomeDefinitionListPacketMapofBiomenamestodataItem>,
    strings: Vec<String>,
) -> BiomeDefinitionListPacket {
    BiomeDefinitionListPacket {
        mapof_biomenamestodata: definitions,
        stringlist: BiomeStringList { strings },
    }
}

fn game_data() -> GameData {
    GameData {
        start_game: Default::default(),
        item_registry: Default::default(),
        biome_definitions: None,
        entity_identifiers: None,
        creative_content: None,
    }
}

fn bool_rule(name: &str, value: bool) -> GameRule {
    GameRule {
        rule_name: name.to_owned(),
        rule_can_be_modified: true,
        rule_value: GameRuleRuleValue::Bool(value),
    }
}

#[test]
fn biome_definition_ids_preserve_the_u16_wire_contract() {
    let packet = biome_packet(
        vec![
            biome_definition(0, u16::MAX as i16),
            biome_definition(1, 0xfffe_u16 as i16),
            biome_definition(2, 600),
        ],
        vec![
            "plains".into(),
            "custom:high".into(),
            "custom:normal".into(),
        ],
    );

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
    let packet = biome_packet(
        vec![biome_definition(1, -1), biome_definition(0, 600)],
        vec!["violet_marsh".into(), "plains".into()],
    );

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
    // The generated key is unsigned, but gophertunnel declares the same bytes
    // as a signed Int16, so 0xffff still has to fail as index -1.
    let invalid_index = biome_packet(
        vec![biome_definition(-1, 1)],
        vec!["minecraft:plains".into()],
    );
    assert_eq!(
        into_world_event(invalid_index.into(), 0).unwrap_err(),
        WorldPacketError::InvalidBiomeNameIndex {
            index: -1,
            string_count: 1,
        }
    );

    let long_name = biome_packet(
        vec![biome_definition(0, 1)],
        vec!["x".repeat(MAX_BIOME_NAME_BYTES + 1)],
    );
    assert_eq!(
        into_world_event(long_name.into(), 0).unwrap_err(),
        WorldPacketError::BiomeNameTooLong {
            bytes: MAX_BIOME_NAME_BYTES + 1,
            max: MAX_BIOME_NAME_BYTES,
        }
    );

    let mut non_finite = biome_definition(0, 1);
    non_finite.value.downfall = f32::NAN;
    let non_finite = biome_packet(vec![non_finite], vec!["minecraft:plains".into()]);
    assert_eq!(
        into_world_event(non_finite.into(), 0).unwrap_err(),
        WorldPacketError::NonFiniteBiomeClimate {
            definition: 0,
            field: "downfall",
        }
    );

    let oversized = biome_packet(
        vec![biome_definition(0, 1); MAX_BIOME_DEFINITIONS + 1],
        vec!["minecraft:plains".into()],
    );
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
    let mut game_data = game_data();
    // The world block of StartGame moved into the nested LevelSettings in
    // 1.26.40; the local-player fields stay on the packet itself.
    game_data.start_game.settings.spawn_settings.dimension = 1;
    game_data.start_game.runtime_id = ActorRuntimeId {
        actor_runtime_id: 0x1_0000_0001,
    };
    game_data.start_game.position = Vec3 {
        x: 1.25,
        y: 72.0,
        z: -8.5,
    };
    game_data.start_game.settings.default_spawn_block_position = BlockPos {
        x: -104,
        y: 114,
        z: 61,
    };
    game_data.start_game.settings.day_cycle_stop_time = 18_000;
    game_data.start_game.level_current_time = 123_456;
    game_data
        .start_game
        .settings
        .rule_data
        .rules_list
        .push(bool_rule("DoDaylightCycle", false));
    game_data.start_game.settings.rain_level = 0.25;
    game_data.start_game.settings.lightning_level = 0.75;
    game_data.start_game.block_network_ids_are_hashes = true;

    assert_eq!(
        WorldBootstrap::from_game_data(&game_data),
        WorldBootstrap {
            dimension: 1,
            local_player_runtime_id: 0x1_0000_0001,
            local_player_unique_id: 0,
            player_position: [1.25, 72.0, -8.5],
            world_spawn_position: [-104, 114, 61],
            air_network_id: HASHED_AIR_NETWORK_ID,
            block_network_ids_are_hashes: true,
        }
    );
    assert_eq!(
        WorldEnvironmentBootstrap::from_game_data(&game_data),
        WorldEnvironmentBootstrap {
            initial_time: 123_456,
            day_cycle_lock_time: 18_000,
            daylight_cycle_enabled: false,
            rain_level: 0.25,
            lightning_level: 0.75,
        }
    );
}

#[test]
fn start_game_daylight_cycle_defaults_enabled_and_requires_a_boolean_rule() {
    let mut game_data = game_data();
    game_data.start_game.level_current_time = 6_000;
    game_data.start_game.settings.day_cycle_stop_time = 0;

    let bootstrap = WorldEnvironmentBootstrap::from_game_data(&game_data);
    assert_eq!(bootstrap.initial_time, 6_000);
    assert_eq!(bootstrap.day_cycle_lock_time, 0);
    assert!(
        bootstrap.daylight_cycle_enabled,
        "an absent doDaylightCycle rule must not turn relay default zero into a clock lock"
    );

    // 1.26.40's GameRule value is a tagged union, so a same-named integer rule
    // can no longer masquerade as the boolean cycle switch.
    game_data
        .start_game
        .settings
        .rule_data
        .rules_list
        .push(GameRule {
            rule_name: "DODAYLIGHTCYCLE".to_owned(),
            rule_can_be_modified: false,
            rule_value: GameRuleRuleValue::Int32(0),
        });
    assert!(
        WorldEnvironmentBootstrap::from_game_data(&game_data).daylight_cycle_enabled,
        "a non-boolean rule with the same name is not an authoritative cycle switch"
    );
}

#[test]
fn clamps_initial_weather_levels_and_fails_non_finite_values_closed() {
    let mut game_data = game_data();
    game_data.start_game.settings.rain_level = -0.25;
    game_data.start_game.settings.lightning_level = 1.25;
    let bootstrap = WorldEnvironmentBootstrap::from_game_data(&game_data);
    assert_eq!(bootstrap.rain_level, 0.0);
    assert_eq!(bootstrap.lightning_level, 1.0);

    game_data.start_game.settings.rain_level = f32::NAN;
    game_data.start_game.settings.lightning_level = f32::INFINITY;
    let bootstrap = WorldEnvironmentBootstrap::from_game_data(&game_data);
    assert_eq!(bootstrap.rain_level, 0.0);
    assert_eq!(bootstrap.lightning_level, 0.0);
}

#[test]
fn normalizes_move_player_to_the_bounded_world_surface() {
    let packet = MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: 73,
        },
        position: Vec3 {
            x: -12.25,
            y: 65.5,
            z: 4096.75,
        },
        // Vec2 is (pitch, yaw); its second component is `y`, not `z`.
        rotation: Vec2 {
            x: -34.5,
            y: 271.25,
        },
        y_head_rotation: 99.0,
        ..Default::default()
    };

    assert_eq!(
        into_world_event(packet.into(), 2).unwrap(),
        Some(WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 73,
            position: [-12.25, 65.5, 4096.75],
            pitch: -34.5,
            yaw: 271.25,
            head_yaw: 99.0,
            mode: protocol::MovePlayerMode::Normal,
            on_ground: false,
            teleported: false,
            source_tick: 0,
        }))
    );
}

#[test]
fn move_player_normalization_preserves_mode_tick_head_yaw_and_ground() {
    let packet = MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: 73,
        },
        position: Vec3 {
            x: -12.25,
            y: 65.5,
            z: 4096.75,
        },
        rotation: Vec2 {
            x: -34.5,
            y: 271.25,
        },
        y_head_rotation: 99.0,
        position_mode: MovePlayerPacketPositionMode::Teleport,
        on_ground: true,
        tick: PlayerInputTick { inputtick: -12 },
        ..Default::default()
    };

    assert_eq!(
        into_world_event(packet.into(), 2).unwrap(),
        Some(WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 73,
            position: [-12.25, 65.5, 4096.75],
            pitch: -34.5,
            yaw: 271.25,
            head_yaw: 99.0,
            mode: protocol::MovePlayerMode::Teleport,
            on_ground: true,
            teleported: true,
            source_tick: -12,
        }))
    );
}

#[test]
fn move_player_modes_map_onto_the_renamed_position_mode_variants() {
    // gophertunnel packet/move_player.go: MoveModeNormal=0, MoveModeReset=1,
    // MoveModeTeleport=2, MoveModeRotation=3.
    for (wire, expected) in [
        (
            MovePlayerPacketPositionMode::Normal,
            protocol::MovePlayerMode::Normal,
        ),
        (
            MovePlayerPacketPositionMode::Respawn,
            protocol::MovePlayerMode::Reset,
        ),
        (
            MovePlayerPacketPositionMode::Teleport,
            protocol::MovePlayerMode::Teleport,
        ),
        (
            MovePlayerPacketPositionMode::OnlyHeadRot,
            protocol::MovePlayerMode::Rotation,
        ),
        (
            MovePlayerPacketPositionMode::Unknown(9),
            protocol::MovePlayerMode::Unknown(9),
        ),
    ] {
        let packet = MovePlayerPacket {
            position_mode: wire,
            ..Default::default()
        };
        let Some(WorldEvent::MovePlayer(event)) = into_world_event(packet.into(), 0).unwrap()
        else {
            panic!("expected a move player event")
        };
        assert_eq!(event.mode, expected);
    }
}

#[test]
fn normalizes_server_authoritative_movement_correction_to_the_local_player_surface() {
    let packet = CorrectPlayerMovePredictionPacket {
        pos: Vec3 {
            x: 27.5,
            y: 111.0,
            z: 91.5,
        },
        pos_delta: Vec3 {
            x: 0.25,
            y: -1.5,
            z: 2.75,
        },
        rotation: Vec2 {
            x: -12.25,
            y: 143.5,
        },
        on_ground: true,
        tick: PlayerInputTick { inputtick: 4_096 },
        ..Default::default()
    };

    assert_eq!(
        into_world_event(packet.into(), 0).unwrap(),
        Some(WorldEvent::PlayerMovementCorrection(
            PlayerMovementCorrectionEvent {
                position: [27.5, 111.0, 91.5],
                delta: [0.25, -1.5, 2.75],
                pitch: -12.25,
                yaw: 143.5,
                on_ground: true,
                tick: 4_096,
            }
        ))
    );
}

#[test]
fn rejects_negative_server_authoritative_movement_correction_tick() {
    let packet = CorrectPlayerMovePredictionPacket {
        tick: PlayerInputTick { inputtick: -1 },
        ..Default::default()
    };

    assert_eq!(
        into_world_event(packet.into(), 0),
        Err(WorldPacketError::NegativeMovementCorrectionTick(-1))
    );
}

#[test]
fn vehicle_prediction_correction_does_not_move_the_local_player_camera() {
    let packet = CorrectPlayerMovePredictionPacket {
        prediction_type: CorrectPlayerMovePredictionPacketPredictionType::Vehicle,
        pos: Vec3 {
            x: 300.0,
            y: 90.0,
            z: -200.0,
        },
        ..Default::default()
    };

    assert_eq!(into_world_event(packet.into(), 0).unwrap(), None);
}

#[test]
fn move_player_uses_varuint64_for_runtime_and_ridden_ids_above_u32() {
    const RUNTIME_ID: i64 = 0x1_0000_0001;
    const RIDDEN_RUNTIME_ID: i64 = 0x2_0000_0002;
    let packet = MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: RUNTIME_ID,
        },
        position: Vec3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: RIDDEN_RUNTIME_ID,
        },
        ..Default::default()
    };
    let mut encoded = BytesMut::new();
    packet.encode(&mut encoded).unwrap();

    assert_eq!(&encoded[..5], &[0x81, 0x80, 0x80, 0x80, 0x10]);
    // 5 (runtime id) + 12 (position) + 8 (rotation) + 4 (head rotation)
    // + 1 (position mode) + 1 (on ground) == 31.
    assert_eq!(&encoded[31..36], &[0x82, 0x80, 0x80, 0x80, 0x20]);
    assert_eq!(packet.encoded_size(), encoded.len());

    let mut encoded = encoded.freeze();
    let decoded = MovePlayerPacket::decode(&mut encoded, ()).unwrap();
    assert_eq!(decoded.player_runtime_id.actor_runtime_id, RUNTIME_ID);
    assert_eq!(
        decoded.riding_runtime_id.actor_runtime_id,
        RIDDEN_RUNTIME_ID
    );
    assert!(!encoded.has_remaining());

    assert_eq!(
        into_world_event(decoded.into(), 0).unwrap(),
        Some(WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: RUNTIME_ID as u64,
            position: [1.0, 2.0, 3.0],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            mode: protocol::MovePlayerMode::Normal,
            on_ground: false,
            teleported: false,
            source_tick: 0,
        }))
    );
}

/// Runtime and ridden ids must still refuse an over-long varint.
///
/// NOTE — decode strictness regressed with the generator. 1.26.30 decoded both
/// ids with `protocol::wire::read_var_u64`, which rejected any tenth byte
/// carrying bits above 2^63 *and* rejected overlong encodings. 1.26.40 models
/// them as `ActorRuntimeId`, which decodes through the shared `VarLong` and only
/// fails once the shift passes 70 bits — so a ten-byte varint is accepted with
/// its high bits silently dropped. That is a valentine_gen/codec issue, not
/// something this crate can fix without changing wire semantics, so this test
/// asserts the guard that does survive rather than pretending the old one does.
#[test]
fn move_player_rejects_overlong_runtime_and_ridden_varint_ids() {
    let packet = MovePlayerPacket::default();
    let mut valid = BytesMut::new();
    packet.encode(&mut valid).unwrap();
    let overflow = [
        0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02,
    ];

    let mut malformed_runtime = BytesMut::new();
    malformed_runtime.extend_from_slice(&overflow);
    malformed_runtime.extend_from_slice(&valid[1..]);
    assert!(MovePlayerPacket::decode(&mut malformed_runtime.freeze(), ()).is_err());

    let ridden_offset = 1 + 12 + 8 + 4 + 1 + 1;
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
        chunk_position: ChunkPos { x: -2, z: 7 },
        dimension_id: DimensionType { value: 0 },
        subchunks_count: 3,
        serialized_chunk_data: vec![1, 2, 3],
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

    // The 1.26.30 `-2` / `-1` sentinels folded into SubChunkCount are gone.
    // gophertunnel packet/level_chunk.go now selects client-request mode with
    // `SubChunkLimit Optional[int32]`, whose documented `-1` means "no limit".
    let limited = LevelChunkPacket {
        chunk_position: ChunkPos { x: 1, z: 2 },
        dimension_id: DimensionType { value: 1 },
        client_request_sub_chunk_limit: Some(8),
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) = into_world_event(limited.into(), 0).unwrap().unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.mode, LevelChunkMode::LimitedRequests { highest: 8 });

    let limitless = LevelChunkPacket {
        client_request_sub_chunk_limit: Some(-1),
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
        client_request_sub_chunk_limit: Some(-3),
        ..Default::default()
    };
    assert_eq!(
        into_world_event(malformed.into(), 0),
        Err(WorldPacketError::InvalidSubChunkCount(-3))
    );

    // BlobHashes are written unconditionally now, so `CacheEnabled` is the only
    // trustworthy cached-transfer marker (gophertunnel packet/level_chunk.go).
    let cached = LevelChunkPacket {
        subchunks_count: 1,
        cache_enabled: true,
        ..Default::default()
    };
    assert_eq!(
        into_world_event(cached.into(), 0),
        Err(WorldPacketError::CachedChunksUnsupported)
    );

    // A world taller than vanilla overworld is accepted: custom servers send
    // standard dimension ids with taller columns. Only the absolute protocol
    // bound is enforced.
    let taller_than_overworld = LevelChunkPacket {
        dimension_id: DimensionType { value: 0 },
        subchunks_count: 25,
        serialized_chunk_data: vec![0; 3],
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) = into_world_event(taller_than_overworld.into(), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.mode, LevelChunkMode::Inline { count: 25 });

    let over_protocol_bound = LevelChunkPacket {
        dimension_id: DimensionType { value: 0 },
        subchunks_count: (MAX_SUB_CHUNK_REQUESTS + 1) as i32,
        ..Default::default()
    };
    assert_eq!(
        into_world_event(over_protocol_bound.into(), 0),
        Err(WorldPacketError::InlineSubChunkCountExceedsDimension {
            dimension: 0,
            count: MAX_SUB_CHUNK_REQUESTS + 1,
            max: MAX_SUB_CHUNK_REQUESTS,
        })
    );

    // SubChunkCount is a Varuint32 on the wire but is decoded into an i32, so a
    // count above i32::MAX still has to be refused rather than wrapped.
    let wrapped_count = LevelChunkPacket {
        subchunks_count: -3,
        ..Default::default()
    };
    assert_eq!(
        into_world_event(wrapped_count.into(), 0),
        Err(WorldPacketError::InvalidSubChunkCount(-3))
    );
}

fn sub_chunk_entry(
    offset: [i8; 3],
    result: SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult,
    payload: Option<Vec<u8>>,
) -> SubChunkPacketPayloadSubChunkPacketData {
    SubChunkPacketPayloadSubChunkPacketData {
        sub_chunk_pos_offset: SubChunkPacketPayloadSubChunkPosOffset {
            subchunk_offset_x: offset[0],
            subchunk_offset_y: offset[1],
            subchunk_offset_z: offset[2],
        },
        sub_chunk_request_result: result,
        serialized_sub_chunk: payload,
        ..Default::default()
    }
}

#[test]
fn resolves_non_cached_sub_chunk_entries_to_absolute_keys() {
    let packet = SubChunkPacket {
        dimension_type: DimensionType { value: 2 },
        center_pos: SubChunkPos {
            subchunk_position_x: 10,
            subchunk_position_y: -4,
            subchunk_position_z: -8,
        },
        sub_chunk_data: vec![
            sub_chunk_entry(
                [-2, 3, 4],
                SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Success,
                Some(vec![9, 0, 0xff]),
            ),
            sub_chunk_entry(
                [0, 1, 0],
                SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::SuccessAllAir,
                None,
            ),
            sub_chunk_entry(
                [1, 0, 0],
                SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::LevelChunkDoesntExist,
                None,
            ),
        ],
        ..Default::default()
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
    // The cached/non-cached entry split collapsed into one entry type carrying
    // `BlobHash Optional[uint64]`, so the packet-level CacheEnabled flag is the
    // mode switch (gophertunnel packet/sub_chunk.go).
    let cached = SubChunkPacket {
        cache_enabled: true,
        sub_chunk_data: vec![SubChunkPacketPayloadSubChunkPacketData {
            sub_chunk_request_result:
                SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::Success,
            serialized_sub_chunk: Some(vec![9, 0, 0]),
            blob_id: Some(7),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(
        into_world_event(cached.into(), 0),
        Err(WorldPacketError::CachedChunksUnsupported)
    );

    let overflow = SubChunkPacket {
        center_pos: SubChunkPos {
            subchunk_position_x: i32::MAX,
            subchunk_position_y: 0,
            subchunk_position_z: 0,
        },
        sub_chunk_data: vec![sub_chunk_entry(
            [1, 0, 0],
            SubChunkPacketPayloadSubChunkPacketDataSubChunkRequestResult::SuccessAllAir,
            None,
        )],
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
        block_position: BlockPos {
            x: 31,
            y: -1,
            z: -17,
        },
        block_runtime_id: 0xdead_beef_u32 as i32,
        flags: 0,
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

    let entry = |x, y, z, runtime_id| UpdateSubChunkNetworkBlockInfo {
        pos: BlockPos { x, y, z },
        runtime_id,
        ..Default::default()
    };
    let batch = UpdateSubChunkBlocksPacket {
        sub_chunk_block_position: BlockPos { x: 1, y: -4, z: -2 },
        blocks_changed: UpdateSubChunkBlocksChangedInfo {
            blocks_changed_standards: vec![entry(16, -64, -32, 4)],
            blocks_changed_extras: vec![entry(17, -63, -31, 5)],
        },
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
        into_world_event(ChunkRadiusUpdatedPacket { chunk_radius: 16 }.into(), 0)
            .unwrap()
            .unwrap()
    else {
        panic!("expected radius event")
    };
    assert_eq!(radius, 16);

    let publisher = NetworkChunkPublisherUpdatePacket {
        newpositionforview: BlockPos {
            x: 32,
            y: 70,
            z: -48,
        },
        newradiusforview: 256,
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
        dimension_id: DimensionType { value: 1 },
        position: Vec3 {
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
fn normalizes_post_spawn_set_time() {
    let packet = SetTimePacket { time: 6000 };
    assert_eq!(
        into_world_event(packet.into(), 0).unwrap(),
        Some(WorldEvent::SetTime(SetTimeEvent { time: 6000 }))
    );
}

#[test]
fn normalizes_respawn_as_a_local_position_authority_change() {
    let packet = RespawnPacket {
        position: Vec3 {
            x: 8.5,
            y: 71.620_01,
            z: -4.25,
        },
        // gophertunnel packet/respawn.go: ReadyToSpawn is wire value 1.
        state: RespawnPacketState::ReadyToSpawn,
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: 42,
        },
    };
    let Some(WorldEvent::Respawn(respawn)) = into_world_event(packet.into(), 0).unwrap() else {
        panic!("expected respawn position-authority event")
    };
    assert_eq!(respawn.position, [8.5, 71.620_01, -4.25]);
    assert_eq!(respawn.state, 1);
    assert_eq!(respawn.runtime_entity_id, 42);

    // Unnamed states keep their raw wire byte instead of being dropped.
    let odd_state = RespawnPacket {
        state: RespawnPacketState::Unknown(9),
        ..Default::default()
    };
    let Some(WorldEvent::Respawn(respawn)) = into_world_event(odd_state.into(), 0).unwrap() else {
        panic!("expected respawn position-authority event")
    };
    assert_eq!(respawn.state, 9);
}

#[test]
fn normalizes_only_boolean_daylight_cycle_rule_changes_case_insensitively() {
    let packet = GameRulesChangedPacket {
        rule_data: GameRulesChangedPacketData {
            rules_list: vec![
                bool_rule("keepinventory", true),
                GameRule {
                    rule_name: "DoDaylightCycle".to_owned(),
                    rule_can_be_modified: true,
                    rule_value: GameRuleRuleValue::Int32(0),
                },
                bool_rule("DODAYLIGHTCYCLE", false),
            ],
        },
    };
    assert_eq!(
        into_world_event(packet.into(), 0).unwrap(),
        Some(WorldEvent::DaylightCycle(DaylightCycleUpdateEvent {
            enabled: false,
        }))
    );

    let wrong_type = GameRulesChangedPacket {
        rule_data: GameRulesChangedPacketData {
            rules_list: vec![GameRule {
                rule_name: "dodaylightcycle".to_owned(),
                rule_can_be_modified: true,
                rule_value: GameRuleRuleValue::Float(0.0),
            }],
        },
    };
    assert_eq!(into_world_event(wrong_type.into(), 0).unwrap(), None);
}

#[test]
fn normalizes_weather_level_events_to_explicit_channel_targets() {
    let cases = [
        (
            LEVEL_EVENT_START_RAINING,
            WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 1.0,
            },
        ),
        (
            LEVEL_EVENT_STOP_RAINING,
            WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 0.0,
            },
        ),
        (
            LEVEL_EVENT_START_THUNDERSTORM,
            WeatherUpdateEvent {
                channel: WeatherChannel::Lightning,
                level: 1.0,
            },
        ),
        (
            LEVEL_EVENT_STOP_THUNDERSTORM,
            WeatherUpdateEvent {
                channel: WeatherChannel::Lightning,
                level: 0.0,
            },
        ),
    ];

    for (event_id, expected) in cases {
        let packet = LevelEventPacket {
            event_id,
            data: 48_000,
            ..Default::default()
        };
        assert_eq!(
            into_world_event(packet.into(), 0).unwrap(),
            Some(WorldEvent::Weather(expected))
        );
    }
}

#[test]
fn ignores_level_events_without_normalized_world_state() {
    let packet = LevelEventPacket {
        event_id: LEVEL_EVENT_SOUND_CLICK,
        ..Default::default()
    };
    assert_eq!(into_world_event(packet.into(), 0).unwrap(), None);
}

#[test]
fn builds_bounded_column_sub_chunk_requests() {
    let packet = request_sub_chunk_column(0, 12, -8, -4, 3).unwrap();
    let McpePacketData::SubChunkRequestPacket(request) = packet.data else {
        panic!("expected SubchunkRequest packet")
    };
    assert_eq!(request.dimension_type.value, 0);
    assert_eq!(
        [
            request.center_pos.subchunk_position_x,
            request.center_pos.subchunk_position_y,
            request.center_pos.subchunk_position_z,
        ],
        [12, -4, -8]
    );
    assert_eq!(request.sub_chunk_position_offset_list.len(), 3);
    assert_eq!(
        request
            .sub_chunk_position_offset_list
            .iter()
            .map(|offset| [
                offset.subchunk_offset_x,
                offset.subchunk_offset_y,
                offset.subchunk_offset_z
            ])
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
