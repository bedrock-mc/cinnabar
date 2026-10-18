use protocol_ids::{HASHED_AIR_NETWORK_ID, SEQUENTIAL_AIR_NETWORK_ID};
use render_api::sub_chunk_requests::MAX_SUB_CHUNK_COLUMN_HEIGHT;

#[path = "world_packets/block_sync.rs"]
mod block_sync;
#[path = "world_packets/crafting_rules.rs"]
mod crafting_rules;
#[path = "world_packets/dimension.rs"]
mod dimension;
#[path = "world_packets/lighting_diagnostics.rs"]
mod lighting_diagnostics;
#[path = "world_packets/move_player.rs"]
mod move_player;
#[path = "world_packets/weather_cycle.rs"]
mod weather_cycle;
#[path = "world_packets/world_clocks.rs"]
mod world_clocks;
use bedrock_protocol_1_26_51::enums::{PlayerRespawnState, SubChunkRequestResult};
use bedrock_protocol_1_26_51::packets::{
    self, BiomeDefinitionList, ChangeDimension, ChunkRadiusUpdated, GameRulesChanged, LevelChunk,
    LevelEvent, NetworkChunkPublisherUpdate, Respawn, SetTime, StartGame, SubChunk, UpdateBlock,
    UpdateSubChunkBlocks,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, BiomeDefinitionData, BiomeStringList, BlockPos, ChunkPos, DimensionType,
    GameRule, GameRuleValue, GameRulesChangedData, SubChunkData, SubChunkPos, SubChunkPosOffset,
    UpdateSubChunkBlocksChangedInfo, UpdateSubChunkNetworkBlockInfo,
};
use bedrock_protocol_1_26_51::wire;
use protocol::{
    BiomeDefinitionEvent, BiomeDefinitionsEvent, DaylightCycleUpdateEvent, DimensionRange,
    GameRulesEvent, HudRules, LevelChunkMode, MAX_BIOME_DEFINITIONS, MAX_BIOME_NAME_BYTES,
    SetTimeEvent, SubChunkResult, WeatherChannel, WeatherUpdateEvent, WorldBootstrap,
    WorldEnvironmentBootstrap, WorldEvent, WorldPacketError, air_network_id, into_world_event,
    request_sub_chunk_column, vanilla_dimension_range,
};

/// `LevelEventStartRaining`, from gophertunnel `packet/level_event.go`
/// @ be6713da4dc051a4197f897d04835e89e9c54321.
const LEVEL_EVENT_START_RAINING: i32 = protocol_ids::LevelEvent::StartRaining.wire_value();
/// `LevelEventStartThunderstorm`.
const LEVEL_EVENT_START_THUNDERSTORM: i32 =
    protocol_ids::LevelEvent::StartThunderstorm.wire_value();
/// `LevelEventStopRaining`.
const LEVEL_EVENT_STOP_RAINING: i32 = protocol_ids::LevelEvent::StopRaining.wire_value();
/// `LevelEventStopThunderstorm`.
const LEVEL_EVENT_STOP_THUNDERSTORM: i32 = protocol_ids::LevelEvent::StopThunderstorm.wire_value();
/// `LevelEventSoundClick`, an event this crate deliberately ignores.
const LEVEL_EVENT_SOUND_CLICK: i32 = protocol_ids::LevelEvent::SoundClick.wire_value();

/// One biome-map entry: the string-table index and its definition. gophertunnel
/// protocol/biome.go writes the index as a signed Int16 over the same two bytes.
type BiomeEntry = (wire::U16LE, BiomeDefinitionData);

fn block_pos(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos {
        x: wire::ZigZag32(x),
        y: wire::ZigZag32(y),
        z: wire::ZigZag32(z),
    }
}

fn biome_definition(name_index: i16, biome_id: i16) -> BiomeEntry {
    (
        wire::U16LE(name_index as u16),
        BiomeDefinitionData {
            id: wire::U16LE(biome_id as u16),
            temperature: wire::F32LE(0.8),
            downfall: wire::F32LE(0.4),
            foliage_snow: wire::F32LE(0.125),
            map_water_color_argb: wire::I32LE(0xff11_2233_u32 as i32),
            ..Default::default()
        },
    )
}

fn biome_packet(definitions: Vec<BiomeEntry>, strings: Vec<String>) -> BiomeDefinitionList {
    BiomeDefinitionList {
        map_of_biome_names_to_data: definitions,
        string_list: BiomeStringList {
            strings: strings.into_iter().map(Into::into).collect(),
        },
    }
}

#[path = "world_packets/seasonal_biomes.rs"]
mod seasonal_biomes;

/// The StartGame packet the startup facts below normalize from.
struct StartupPackets {
    start_game: StartGame,
}

fn game_data() -> StartupPackets {
    StartupPackets {
        start_game: Default::default(),
    }
}

fn bool_rule(name: &str, value: bool) -> GameRule {
    GameRule {
        rule_name: name.into(),
        rule_can_be_modified: true,
        rule_value: GameRuleValue::Bool(value),
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

    let WorldEvent::BiomeDefinitions(event) = into_world_event(protocol::Packet::new(packet), 0)
        .unwrap()
        .unwrap()
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

    let event = into_world_event(protocol::Packet::new(packet), 0)
        .unwrap()
        .unwrap();
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
                    max_snow_accumulation: None,
                    map_water_color: 0xff11_2233,
                },
                BiomeDefinitionEvent {
                    biome_id: Some(600),
                    name: "violet_marsh".into(),
                    temperature: 0.8,
                    downfall: 0.4,
                    snow_foliage: 0.125,
                    max_snow_accumulation: None,
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
        into_world_event(protocol::Packet::new(invalid_index), 0).unwrap_err(),
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
        into_world_event(protocol::Packet::new(long_name), 0).unwrap_err(),
        WorldPacketError::BiomeNameTooLong {
            bytes: MAX_BIOME_NAME_BYTES + 1,
            max: MAX_BIOME_NAME_BYTES,
        }
    );

    let mut non_finite = biome_definition(0, 1);
    non_finite.1.downfall = wire::F32LE(f32::NAN);
    let non_finite = biome_packet(vec![non_finite], vec!["minecraft:plains".into()]);
    assert_eq!(
        into_world_event(protocol::Packet::new(non_finite), 0).unwrap_err(),
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
        into_world_event(protocol::Packet::new(oversized), 0).unwrap_err(),
        WorldPacketError::TooManyBiomeDefinitions {
            count: MAX_BIOME_DEFINITIONS + 1,
            max: MAX_BIOME_DEFINITIONS,
        }
    );
}

#[test]
fn chooses_air_value_from_start_game_hash_mode() {
    assert_eq!(air_network_id(false), SEQUENTIAL_AIR_NETWORK_ID);
    assert_eq!(air_network_id(true), HASHED_AIR_NETWORK_ID);
}

#[test]
fn normalizes_start_game_bootstrap_without_generated_types() {
    let mut game_data = game_data();
    game_data.start_game.settings.spawn_settings.dimension = wire::ZigZag32(1);
    game_data.start_game.runtime_id = ActorRuntimeID(0x1_0000_0001);
    game_data.start_game.position = [1.25, 72.0, -8.5].into();
    game_data.start_game.settings.default_spawn_block_position = block_pos(-104, 114, 61);
    game_data.start_game.settings.day_cycle_stop_time = wire::ZigZag32(18_000);
    game_data.start_game.level_current_time = wire::U64LE(123_456);
    game_data
        .start_game
        .settings
        .rule_data
        .rules_list
        .push(bool_rule("DoDaylightCycle", false));
    game_data.start_game.settings.rain_level = wire::F32LE(0.25);
    game_data.start_game.settings.lightning_level = wire::F32LE(0.75);
    game_data.start_game.block_network_ids_are_hashes = true;

    assert_eq!(
        WorldBootstrap::from_start_game(&game_data.start_game),
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
        WorldEnvironmentBootstrap::from_start_game(&game_data.start_game),
        WorldEnvironmentBootstrap {
            initial_time: 123_456,
            day_cycle_lock_time: 18_000,
            daylight_cycle_enabled: false,
            weather_cycle_enabled: true,
            rain_level: 0.25,
            lightning_level: 0.75,
        }
    );
}

#[test]
fn start_game_daylight_cycle_defaults_enabled_and_requires_a_boolean_rule() {
    let mut game_data = game_data();
    game_data.start_game.level_current_time = wire::U64LE(6_000);
    game_data.start_game.settings.day_cycle_stop_time = wire::ZigZag32(0);

    let bootstrap = WorldEnvironmentBootstrap::from_start_game(&game_data.start_game);
    assert_eq!(bootstrap.initial_time, 6_000);
    assert_eq!(bootstrap.day_cycle_lock_time, 0);
    assert!(
        bootstrap.daylight_cycle_enabled,
        "an absent doDaylightCycle rule must not turn relay default zero into a clock lock"
    );

    // GameRule's value is a tagged union, so a same-named integer rule cannot
    // masquerade as the boolean cycle switch.
    game_data
        .start_game
        .settings
        .rule_data
        .rules_list
        .push(GameRule {
            rule_name: "DODAYLIGHTCYCLE".into(),
            rule_can_be_modified: false,
            rule_value: GameRuleValue::Int32(wire::I32LE(0)),
        });
    assert!(
        WorldEnvironmentBootstrap::from_start_game(&game_data.start_game).daylight_cycle_enabled,
        "a non-boolean rule with the same name is not an authoritative cycle switch"
    );
}

#[test]
fn clamps_initial_weather_levels_and_fails_non_finite_values_closed() {
    let mut game_data = game_data();
    game_data.start_game.settings.rain_level = wire::F32LE(-0.25);
    game_data.start_game.settings.lightning_level = wire::F32LE(1.25);
    let bootstrap = WorldEnvironmentBootstrap::from_start_game(&game_data.start_game);
    assert_eq!(bootstrap.rain_level, 0.0);
    assert_eq!(bootstrap.lightning_level, 1.0);

    game_data.start_game.settings.rain_level = wire::F32LE(f32::NAN);
    game_data.start_game.settings.lightning_level = wire::F32LE(f32::INFINITY);
    let bootstrap = WorldEnvironmentBootstrap::from_start_game(&game_data.start_game);
    assert_eq!(bootstrap.rain_level, 0.0);
    assert_eq!(bootstrap.lightning_level, 0.0);
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
    let inline = LevelChunk {
        chunk_position: ChunkPos {
            x: wire::ZigZag32(-2),
            z: wire::ZigZag32(7),
        },
        dimension_id: DimensionType(0),
        sub_chunks_count: wire::VarUInt(3),
        serialized_chunk_data: vec![1, 2, 3].into(),
        ..Default::default()
    };
    let event = into_world_event(protocol::Packet::new(inline), 0)
        .unwrap()
        .unwrap();
    let WorldEvent::LevelChunk(event) = event else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.x, -2);
    assert_eq!(event.z, 7);
    assert_eq!(event.dimension, 0);
    assert_eq!(event.mode, LevelChunkMode::Inline { count: 3 });
    assert_eq!(event.payload, vec![1, 2, 3]);

    // gophertunnel packet/level_chunk.go selects client-request mode with
    // `SubChunkLimit Optional[int32]`, whose documented `-1` means "no limit".
    let limited = LevelChunk {
        chunk_position: ChunkPos {
            x: wire::ZigZag32(1),
            z: wire::ZigZag32(2),
        },
        dimension_id: DimensionType(1),
        client_request_sub_chunk_limit: Some(wire::ZigZag32(8)),
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) = into_world_event(protocol::Packet::new(limited), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.mode, LevelChunkMode::LimitedRequests { highest: 8 });

    let limitless = LevelChunk {
        client_request_sub_chunk_limit: Some(wire::ZigZag32(-1)),
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) = into_world_event(protocol::Packet::new(limitless), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.mode, LevelChunkMode::LimitlessRequests);
}

#[test]
fn rejects_malformed_or_cached_level_chunks() {
    let malformed = LevelChunk {
        client_request_sub_chunk_limit: Some(wire::ZigZag32(-3)),
        ..Default::default()
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(malformed), 0),
        Err(WorldPacketError::InvalidSubChunkCount(-3))
    );

    // BlobHashes are written unconditionally now, so `CacheEnabled` is the only
    // trustworthy cached-transfer marker (gophertunnel packet/level_chunk.go).
    let cached = LevelChunk {
        sub_chunks_count: wire::VarUInt(1),
        cache_enabled: true,
        ..Default::default()
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(cached), 0),
        Err(WorldPacketError::CachedChunksUnsupported)
    );

    // A world taller than vanilla overworld is accepted: custom servers send
    // standard dimension ids with taller columns.
    let taller_than_overworld = LevelChunk {
        dimension_id: DimensionType(0),
        sub_chunks_count: wire::VarUInt(25),
        serialized_chunk_data: vec![0; 3].into(),
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) =
        into_world_event(protocol::Packet::new(taller_than_overworld), 0)
            .unwrap()
            .unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(event.mode, LevelChunkMode::Inline { count: 25 });

    let over_request_bound = LevelChunk {
        dimension_id: DimensionType(0),
        sub_chunks_count: wire::VarUInt((MAX_SUB_CHUNK_COLUMN_HEIGHT + 1) as u32),
        ..Default::default()
    };
    let WorldEvent::LevelChunk(event) =
        into_world_event(protocol::Packet::new(over_request_bound), 0)
            .unwrap()
            .unwrap()
    else {
        panic!("expected LevelChunk event")
    };
    assert_eq!(
        event.mode,
        LevelChunkMode::Inline {
            count: MAX_SUB_CHUNK_COLUMN_HEIGHT + 1
        }
    );
}

fn sub_chunk_pos(x: i32, y: i32, z: i32) -> SubChunkPos {
    SubChunkPos {
        subchunk_position_x: wire::I32LE(x),
        subchunk_position_y: wire::I32LE(y),
        subchunk_position_z: wire::I32LE(z),
    }
}

fn sub_chunk_entry(
    offset: [i8; 3],
    result: SubChunkRequestResult,
    payload: Option<Vec<u8>>,
) -> SubChunkData {
    SubChunkData {
        sub_chunk_pos_offset: SubChunkPosOffset {
            subchunk_offset_x: wire::I8(offset[0]),
            subchunk_offset_y: wire::I8(offset[1]),
            subchunk_offset_z: wire::I8(offset[2]),
        },
        sub_chunk_request_result: result,
        serialized_sub_chunk: payload.map(Into::into),
        ..Default::default()
    }
}

#[test]
fn resolves_non_cached_sub_chunk_entries_to_absolute_keys() {
    let packet = SubChunk {
        dimension_type: DimensionType(2),
        center_pos: sub_chunk_pos(10, -4, -8),
        sub_chunk_data: vec![
            sub_chunk_entry(
                [-2, 3, 4],
                SubChunkRequestResult::Success,
                Some(vec![9, 0, 0xff]),
            ),
            sub_chunk_entry([0, 1, 0], SubChunkRequestResult::Successallair, None),
            sub_chunk_entry(
                [1, 0, 0],
                SubChunkRequestResult::Levelchunkdoesntexist,
                None,
            ),
        ],
        ..Default::default()
    };

    let WorldEvent::SubChunks(batch) = into_world_event(protocol::Packet::new(packet), 0)
        .unwrap()
        .unwrap()
    else {
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
    // One entry type carries `BlobHash Optional[uint64]`, so the packet-level
    // CacheEnabled flag is the mode switch (gophertunnel packet/sub_chunk.go).
    let cached = SubChunk {
        cache_enabled: true,
        sub_chunk_data: vec![SubChunkData {
            sub_chunk_request_result: SubChunkRequestResult::Success,
            serialized_sub_chunk: Some(vec![9, 0, 0].into()),
            blob_id: Some(wire::U64LE(7)),
            ..Default::default()
        }],
        ..Default::default()
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(cached), 0),
        Err(WorldPacketError::CachedChunksUnsupported)
    );

    let overflow = SubChunk {
        center_pos: sub_chunk_pos(i32::MAX, 0, 0),
        sub_chunk_data: vec![sub_chunk_entry(
            [1, 0, 0],
            SubChunkRequestResult::Successallair,
            None,
        )],
        ..Default::default()
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(overflow), 0),
        Err(WorldPacketError::SubChunkPositionOverflow {
            origin: [i32::MAX, 0, 0],
            offset: [1, 0, 0],
        })
    );
}

#[test]
fn normalizes_single_and_batched_block_updates_with_layers() {
    let single = UpdateBlock {
        block_position: block_pos(31, -1, -17),
        block_runtime_id: wire::VarUInt(0xdead_beef_u32),
        flags: wire::VarUInt(0),
        layer: wire::VarUInt(1),
    };
    let WorldEvent::BlockUpdates(updates) = into_world_event(protocol::Packet::new(single), 2)
        .unwrap()
        .unwrap()
    else {
        panic!("expected BlockUpdates event")
    };
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].dimension, 2);
    assert_eq!(updates[0].position, [31, -1, -17]);
    assert_eq!(updates[0].layer, 1);
    assert_eq!(updates[0].network_id, 0xdead_beef);

    let entry = |x, y, z, runtime_id| UpdateSubChunkNetworkBlockInfo {
        pos: block_pos(x, y, z),
        runtime_id: wire::VarUInt(runtime_id),
        ..Default::default()
    };
    let batch = UpdateSubChunkBlocks {
        sub_chunk_block_position: block_pos(1, -4, -2),
        blocks_changed: UpdateSubChunkBlocksChangedInfo {
            blocks_changed_standards: vec![entry(16, -64, -32, 4)],
            blocks_changed_extras: vec![entry(17, -63, -31, 5)],
        },
    };
    let WorldEvent::SyncedBlockUpdates(updates) = into_world_event(protocol::Packet::new(batch), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected BlockUpdates event")
    };
    assert_eq!(updates.len(), 2);
    assert_eq!(updates[0].update.layer, 0);
    assert_eq!(updates[0].update.network_id, 4);
    assert_eq!(updates[1].update.layer, 1);
    assert_eq!(updates[1].update.network_id, 5);
}

#[test]
fn rejects_negative_or_excessive_update_layers() {
    let layer = 16;
    let packet = UpdateBlock {
        layer: wire::VarUInt(layer),
        ..Default::default()
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0),
        Err(WorldPacketError::InvalidBlockLayer(layer))
    );
}

#[test]
fn normalizes_streaming_radius_publisher_and_dimension_events() {
    let WorldEvent::ChunkRadiusUpdated(radius) = into_world_event(
        protocol::Packet::new(ChunkRadiusUpdated {
            chunk_radius: wire::ZigZag32(16),
        }),
        0,
    )
    .unwrap()
    .unwrap() else {
        panic!("expected radius event")
    };
    assert_eq!(radius, 16);

    let publisher = NetworkChunkPublisherUpdate {
        new_position_for_view: block_pos(32, 70, -48),
        new_radius_for_view: wire::VarUInt(256),
        ..Default::default()
    };
    let WorldEvent::PublisherUpdate(update) = into_world_event(protocol::Packet::new(publisher), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected publisher event")
    };
    assert_eq!(update.center, [32, 70, -48]);
    assert_eq!(update.radius_blocks, 256);

    let dimension = ChangeDimension {
        dimension_id: DimensionType(1),
        position: [1.5, 80.0, -2.5].into(),
        respawn: true,
        loading_screen_id: Some(wire::U32LE(0)),
    };
    let WorldEvent::ChangeDimension(change) = into_world_event(protocol::Packet::new(dimension), 0)
        .unwrap()
        .unwrap()
    else {
        panic!("expected dimension event")
    };
    assert_eq!(change.dimension, 1);
    assert_eq!(change.position, [1.5, 80.0, -2.5]);
    assert!(change.respawn);
    assert_eq!(change.loading_screen_id, Some(0));
}

#[test]
fn normalizes_post_spawn_set_time() {
    let packet = SetTime {
        time: wire::ZigZag32(6000),
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
        Some(WorldEvent::SetTime(SetTimeEvent { time: 6000 }))
    );
}

#[test]
fn normalizes_respawn_as_a_local_position_authority_change() {
    let packet = Respawn {
        position: [8.5, 71.620_01, -4.25].into(),
        // gophertunnel packet/respawn.go: ReadyToSpawn is wire value 1.
        state: PlayerRespawnState::Readytospawn,
        player_runtime_id: ActorRuntimeID(42),
    };
    let Some(WorldEvent::Respawn(respawn)) =
        into_world_event(protocol::Packet::new(packet), 0).unwrap()
    else {
        panic!("expected respawn position-authority event")
    };
    assert_eq!(respawn.position, [8.5, 71.620_01, -4.25]);
    assert_eq!(respawn.state, 1);
    assert_eq!(respawn.runtime_entity_id, 42);

    // Unnamed states keep their raw wire byte instead of being dropped.
    let odd_state = Respawn {
        state: PlayerRespawnState::Unknown(9),
        ..Default::default()
    };
    let Some(WorldEvent::Respawn(respawn)) =
        into_world_event(protocol::Packet::new(odd_state), 0).unwrap()
    else {
        panic!("expected respawn position-authority event")
    };
    assert_eq!(respawn.state, 9);
}

#[test]
fn normalizes_only_boolean_daylight_cycle_rule_changes_case_insensitively() {
    let packet = GameRulesChanged {
        rule_data: GameRulesChangedData {
            rules_list: vec![
                bool_rule("keepinventory", true),
                GameRule {
                    rule_name: "DoDaylightCycle".into(),
                    rule_can_be_modified: true,
                    rule_value: GameRuleValue::Int32(wire::I32LE(0)),
                },
                bool_rule("DODAYLIGHTCYCLE", false),
            ],
        },
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
        Some(WorldEvent::GameRules(GameRulesEvent {
            daylight_cycle: Some(DaylightCycleUpdateEvent { enabled: false }),
            weather_cycle: None,
            hud: HudRules::default(),
            death: Default::default(),
            crafting: Default::default(),
        }))
    );

    let wrong_type = GameRulesChanged {
        rule_data: GameRulesChangedData {
            rules_list: vec![GameRule {
                rule_name: "dodaylightcycle".into(),
                rule_can_be_modified: true,
                rule_value: GameRuleValue::Float(wire::F32LE(0.0)),
            }],
        },
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(wrong_type), 0).unwrap(),
        None
    );
}

#[test]
fn normalizes_the_hud_text_rules_beside_the_daylight_cycle() {
    let packet = GameRulesChanged {
        rule_data: GameRulesChangedData {
            rules_list: vec![
                bool_rule("showCoordinates", true),
                bool_rule("showdaysplayed", false),
            ],
        },
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
        Some(WorldEvent::GameRules(GameRulesEvent {
            daylight_cycle: None,
            weather_cycle: None,
            hud: HudRules {
                show_coordinates: Some(true),
                show_days_played: Some(false),
            },
            death: Default::default(),
            crafting: Default::default(),
        }))
    );
    let mut game_data = game_data();
    assert_eq!(
        HudRules::from_start_game(&game_data.start_game),
        HudRules {
            show_coordinates: Some(false),
            show_days_played: Some(false),
        },
        "absent StartGame rules read as their vanilla default, off"
    );
    game_data
        .start_game
        .settings
        .rule_data
        .rules_list
        .push(bool_rule("showcoordinates", true));
    assert_eq!(
        HudRules::from_start_game(&game_data.start_game).show_coordinates,
        Some(true)
    );
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
        let packet = LevelEvent {
            event_id: wire::ZigZag32(event_id),
            data: wire::ZigZag32(48_000),
            ..Default::default()
        };
        assert_eq!(
            into_world_event(protocol::Packet::new(packet), 0).unwrap(),
            Some(WorldEvent::Weather(expected))
        );
    }
}

#[test]
fn ignores_level_events_without_normalized_world_state() {
    // Sleeping-players count: no sound, particle, crack or weather mapping.
    let packet = LevelEvent {
        event_id: wire::ZigZag32(9_801),
        ..Default::default()
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
        None
    );
    let click = LevelEvent {
        event_id: wire::ZigZag32(LEVEL_EVENT_SOUND_CLICK),
        ..Default::default()
    };
    assert!(matches!(
        into_world_event(protocol::Packet::new(click), 0).unwrap(),
        Some(WorldEvent::Audio(_))
    ));
}

#[test]
fn column_sub_chunk_request_keeps_zero_vertical_origin() {
    let packet = request_sub_chunk_column(0, 12, -8, -4, 3).unwrap();
    let packets::Packet::SubChunkRequest(request) = *packet.data else {
        panic!("expected SubChunkRequest packet")
    };
    assert_eq!(request.center_pos, sub_chunk_pos(12, 0, -8));
    assert_eq!(
        request
            .sub_chunk_position_offset_list
            .iter()
            .map(|offset| offset.subchunk_offset_y.0)
            .collect::<Vec<_>>(),
        [-4, -3, -2]
    );
}

#[test]
fn builds_bounded_column_sub_chunk_requests() {
    let packet = request_sub_chunk_column(0, 12, -8, -4, 3).unwrap();
    let packets::Packet::SubChunkRequest(request) = *packet.data else {
        panic!("expected SubchunkRequest packet")
    };
    assert_eq!(request.dimension_type.0, 0);
    assert_eq!(request.center_pos, sub_chunk_pos(12, 0, -8));
    assert_eq!(request.sub_chunk_position_offset_list.len(), 3);
    assert_eq!(
        request
            .sub_chunk_position_offset_list
            .iter()
            .map(|offset| [
                offset.subchunk_offset_x.0,
                offset.subchunk_offset_y.0,
                offset.subchunk_offset_z.0
            ])
            .collect::<Vec<_>>(),
        vec![[0, -4, 0], [0, -3, 0], [0, -2, 0]]
    );

    assert_eq!(
        request_sub_chunk_column(0, 0, 0, 0, MAX_SUB_CHUNK_COLUMN_HEIGHT + 1),
        Err(WorldPacketError::TooManySubChunkRequests {
            count: MAX_SUB_CHUNK_COLUMN_HEIGHT + 1,
            max: MAX_SUB_CHUNK_COLUMN_HEIGHT,
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

mod sub_chunk_requests;
