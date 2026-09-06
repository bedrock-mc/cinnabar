//! Wire-truth harness for the pinned `.bin` fixtures.
//!
//! Every fixture under `crates/protocol/fixtures/` is produced by gophertunnel
//! at commit `9f42f3679a573fc4b51104569cc4f422036e28ec` (Bedrock 1.26.40 /
//! protocol 2168). The bytes are the authority: this file only asserts that the
//! generated Valentine shapes decode to the values those bytes carry and
//! re-encode to the identical bytes.

use bytes::{BufMut, Bytes, BytesMut};
use protocol::{
    BedrockSession, GAME_VERSION, PROTOCOL_VERSION, PlayerAuthInputSnapshot, PlayerInputFlags,
    PlayerInputMode, ProtocolError, decode_batch, encode, player_auth_input,
};
use valentine::bedrock::version::v1_26_44::{
    ActorRuntimeId, ActorUniqueId, BlockPos, ChunkPos, DimensionType, EnumsGameType,
    EnumsInputMode, EnumsNewInteractionModel, EnumsPacketCompressionAlgorithm,
    EnumsPlayerAuthInputPacketPayloadInputData, EnumsPlayerPermissionLevel,
    EnumsPlayerPositionModeComponentPositionMode, McpePacketData, McpePacketName, PlayerInputTick,
    Vec2, Vec3,
};

const NETWORK_SETTINGS: &[u8] = include_bytes!("../fixtures/network_settings.bin");
const START_GAME: &[u8] = include_bytes!("../fixtures/start_game.bin");
const LEVEL_CHUNK: &[u8] = include_bytes!("../fixtures/level_chunk.bin");
const MOVE_PLAYER: &[u8] = include_bytes!("../fixtures/move_player.bin");
const PLAYER_AUTH_INPUT: &[u8] = include_bytes!("../fixtures/player_auth_input.bin");
const ADD_ACTOR: &[u8] = include_bytes!("../fixtures/add_actor.bin");
const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_BATCH_PACKETS: usize = 1_600;

/// `TeleportCauseCommand` in gophertunnel `minecraft/protocol/packet/move_player.go`.
///
/// 1.26.40 removed the named `MovePlayerPacketTeleportCause` enum from the
/// generated types: `MovePlayerTeleportData::teleportation_cause` is now a plain
/// `i32`, so the fixture's value is checked against the pinned Go constant
/// instead of a Rust variant.
const TELEPORT_CAUSE_COMMAND: i32 = 3;

/// `MovePlayerTeleportData::source_actor_type` is likewise a bare `i32` in
/// 1.26.40, so the fixture retains its numeric value directly.
const SOURCE_ACTOR_TYPE: i32 = 87;

fn session() -> BedrockSession {
    BedrockSession { shield_item_id: 0 }
}

fn decode_one(fixture: &'static [u8], id: McpePacketName) -> protocol::Packet {
    let packets = decode_batch(Bytes::from_static(fixture), &session()).expect("decode fixture");
    assert_eq!(packets.len(), 1);
    let packet = packets.into_iter().next().expect("one packet");
    assert_eq!(packet.header.id, id);
    assert_eq!(packet.header.from_subclient, 1);
    assert_eq!(packet.header.to_subclient, 2);
    packet
}

fn assert_exact_round_trip(packet: &protocol::Packet, fixture: &[u8]) {
    let encoded = encode(packet, &session()).expect("encode fixture");
    assert_eq!(encoded.as_ref(), fixture);
}

#[test]
fn protocol_constants_are_pinned_to_1_26_44() {
    assert_eq!(GAME_VERSION, "1.26.44");
    assert_eq!(PROTOCOL_VERSION, 2168);
}

#[test]
fn network_settings_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(NETWORK_SETTINGS, McpePacketName::NetworkSettingsPacket);
    match &packet.data {
        McpePacketData::NetworkSettingsPacket(settings) => {
            assert_eq!(settings.compression_threshold, 512);
            // Restated, not weakened: the fixture still carries algorithm 0.
            // gophertunnel calls 0 `CompressionAlgorithmFlate`
            // (packet/network_settings.go); the 1.26.40 generated enum renamed
            // the same discriminant from `Deflate` to `ZLib`.
            assert_eq!(
                settings.compression_algorithm,
                EnumsPacketCompressionAlgorithm::ZLib
            );
            // `client_throttle` is `client_throttle_enabled` in 1.26.40.
            assert!(settings.client_throttle_enabled);
            assert_eq!(settings.client_throttle_threshold, 8);
            assert_eq!(settings.client_throttle_scalar, 0.5);
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, NETWORK_SETTINGS);
}

#[test]
fn start_game_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(START_GAME, McpePacketName::StartGamePacket);
    match &packet.data {
        McpePacketData::StartGamePacket(start) => {
            // `ActorUniqueId`/`ActorRuntimeId` are single-field wrapper structs.
            assert_eq!(start.entity_id, ActorUniqueId { actor_unique_id: 1 });
            assert_eq!(
                start.runtime_id,
                ActorRuntimeId {
                    actor_runtime_id: 2
                }
            );
            // `player_gamemode` -> `game_type`.
            assert_eq!(start.game_type, EnumsGameType::Creative);
            // `player_position` -> `position`, `Vec3F` -> `Vec3`.
            assert_eq!(
                start.position,
                Vec3 {
                    x: 1.25,
                    y: 64.0,
                    z: -2.5,
                }
            );
            // `Vec2F { x, z }` -> `Vec2 { x, y }`: the second component is now
            // named `y` and still carries the yaw.
            assert_eq!(start.rotation.x, 10.5);
            assert_eq!(start.rotation.y, 20.25);

            // The inline world fields moved into `settings: LevelSettings`.
            let settings = &start.settings;
            assert_eq!(settings.seed, 12_345);
            // `dimension: StartGamePacketDimension::Overworld` is now the
            // spawn settings' plain dimension id; 0 is the overworld.
            assert_eq!(settings.spawn_settings.dimension, 0);
            // `spawn_position` -> `default_spawn_block_position`,
            // `BlockCoordinates` -> `BlockPos`.
            assert_eq!(
                settings.default_spawn_block_position,
                BlockPos { x: 8, y: 64, z: -8 }
            );
            // `game_version` -> `base_game_version`; the fixture is 1.26.40.
            assert_eq!(settings.base_game_version, "1.26.40");

            // 1.26.40 wire changes carried by this fixture.
            //
            // PlayerPermissions moved from a varint to a single byte:
            // `io.Uint8(&pk.PlayerPermissions)` in the same file; the generated
            // enum encodes/decodes one `i8`.
            assert_eq!(
                settings.player_permissions,
                EnumsPlayerPermissionLevel::Member
            );
            // The GameRule list is carried by `rule_data`, and this fixture
            // sends none.
            assert!(settings.rule_data.rules_list.is_empty());
            // `IsLoggingChat` is gone from the 1.26.40 LevelSettings entirely;
            // there is no field left to assert on. The exact round-trip below
            // is what proves no byte is written for it.

            assert_eq!(start.level_id, "fixture-level");
            // `world_name` -> `level_name`.
            assert_eq!(start.level_name, "Fixture World");
            // The two movement fields live in `movement_settings`.
            assert_eq!(start.movement_settings.rewind_history_size, 20);
            assert!(start.movement_settings.server_authoritative_block_breaking);
            // `current_tick` -> `level_current_time`.
            assert_eq!(start.level_current_time, 123_456_789);
            assert_eq!(
                start.multiplayer_correlation_id,
                "00000000-0000-0000-0000-000000000001"
            );
            // `server_authoritative_inventory` -> `enable_item_stack_net_manager`.
            assert!(start.enable_item_stack_net_manager);
            // `engine` -> `server_version`.
            assert_eq!(start.server_version, "1.26.40");
            assert!(start.block_network_ids_are_hashes);
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, START_GAME);
}

#[test]
fn level_chunk_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(LEVEL_CHUNK, McpePacketName::LevelChunkPacket);
    match &packet.data {
        McpePacketData::LevelChunkPacket(chunk) => {
            // `x`/`z` -> `chunk_position: ChunkPos`.
            assert_eq!(chunk.chunk_position, ChunkPos { x: 3, z: -4 });
            // `dimension: i32` -> `dimension_id: DimensionType { value }`.
            assert_eq!(chunk.dimension_id, DimensionType { value: 0 });
            // Restated, not weakened. gophertunnel
            // packet/level_chunk.go now writes
            // `io.Varuint32(&pk.SubChunkCount)` followed by
            // `protocol.OptionalFunc(io, &pk.SubChunkLimit, io.Varint32)`, so
            // the old `-2` request-mode sentinel packed into the count is gone.
            // The fixture's request-mode chunk is therefore count 0 plus an
            // explicit limit, and the old `highest_subchunk_count == Some(24)`
            // is now that limit.
            assert_eq!(chunk.subchunks_count, 0);
            assert_eq!(chunk.client_request_sub_chunk_limit, Some(24));
            // `blobs: Option<..>` is gone: the same file writes
            // `protocol.FuncSlice(io, &pk.BlobHashes, io.Uint64)`
            // unconditionally, gated by nothing. The old `blobs.is_none()`
            // therefore restates as "cache disabled and no hashes present".
            assert!(!chunk.cache_enabled);
            assert!(chunk.cache_metadata.is_empty());
            // `payload` -> `serialized_chunk_data`.
            assert_eq!(chunk.serialized_chunk_data, [0xde, 0xad, 0xbe, 0xef]);
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, LEVEL_CHUNK);
}

#[test]
fn move_player_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(MOVE_PLAYER, McpePacketName::MovePlayerPacket);
    match &packet.data {
        McpePacketData::MovePlayerPacket(movement) => {
            // `runtime_id` -> `player_runtime_id: ActorRuntimeId`.
            assert_eq!(
                movement.player_runtime_id,
                ActorRuntimeId {
                    actor_runtime_id: 42
                }
            );
            assert_eq!(
                movement.position,
                Vec3 {
                    x: 1.25,
                    y: 64.0,
                    z: -2.5,
                }
            );
            // `pitch`/`yaw` -> `rotation: Vec2 { x, y }`.
            assert_eq!(movement.rotation, Vec2 { x: 10.5, y: 20.25 });
            // `head_yaw` -> `y_head_rotation`.
            assert_eq!(movement.y_head_rotation, 30.75);
            // `mode` -> `position_mode`.
            assert_eq!(
                movement.position_mode,
                EnumsPlayerPositionModeComponentPositionMode::Teleport
            );
            assert!(movement.on_ground);
            assert_eq!(
                movement.riding_runtime_id,
                ActorRuntimeId {
                    actor_runtime_id: 0
                }
            );
            // TeleportData is now optional on the wire:
            // `protocol.OptionalMarshaler(io, &pk.TeleportData)` in
            // gophertunnel packet/move_player.go writes a presence bool first.
            // The fixture sets it, so the payload must still be present.
            let teleport = movement.teleport_data.as_ref().expect("teleport data");
            assert_eq!(teleport.teleportation_cause, TELEPORT_CAUSE_COMMAND);
            assert_eq!(teleport.source_actor_type, SOURCE_ACTOR_TYPE);
            // `tick: u64` -> `tick: PlayerInputTick`.
            assert_eq!(movement.tick, PlayerInputTick { inputtick: 1_234 });
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, MOVE_PLAYER);
}

#[test]
fn player_auth_input_fixture_decodes_and_round_trips_exactly() {
    let fixture = decode_one(PLAYER_AUTH_INPUT, McpePacketName::PlayerAuthInputPacket);
    let McpePacketData::PlayerAuthInputPacket(input) = &fixture.data else {
        panic!("unexpected fixture payload");
    };
    // `tick` -> `client_tick: PlayerInputTick`.
    assert_eq!(input.client_tick, PlayerInputTick { inputtick: 1_234 });
    assert_eq!(input.input_mode, EnumsInputMode::Mouse);
    // `interaction_model: Unknown(-1)` -> `new_interaction_model: Crosshair`.
    // gophertunnel packet/player_auth_input.go writes
    // `io.Varint32(&pk.InteractionModel)` and the fixture carries `0x02`,
    // i.e. zigzag 1 == `InteractionModelCrosshair`. The old `Unknown(-1)`
    // was the protocol-1001 generated definition disagreeing on signedness.
    assert_eq!(
        input.new_interaction_model,
        EnumsNewInteractionModel::Crosshair
    );
    // Restated, not weakened: the input flags stopped being a bitset.
    // `protocol.InputFlagList(io, &pk.InputData, InputFlagCount)`
    // (gophertunnel minecraft/protocol/input_flags.go:78) writes a presence
    // bool, a count, and then one zigzag varint per set flag ID. The old
    // `UP | LEFT | JUMPING | SPRINTING` bitset is now exactly this list, in
    // ascending flag-ID order.
    assert_eq!(
        input.input_data,
        Some(vec![
            EnumsPlayerAuthInputPacketPayloadInputData::Jumping,
            EnumsPlayerAuthInputPacketPayloadInputData::Up,
            EnumsPlayerAuthInputPacketPayloadInputData::Left,
            EnumsPlayerAuthInputPacketPayloadInputData::Sprinting,
        ])
    );
    // `pitch`/`yaw` -> `player_rotation: Vec2 { x, y }`.
    assert_eq!(input.player_rotation, Vec2 { x: 10.5, y: 20.25 });
    assert_eq!(
        input.position,
        Vec3 {
            x: 1.25,
            y: 64.0,
            z: -2.5,
        }
    );
    assert_eq!(input.player_head_rotation, 30.75);
    // `delta` -> `pos_delta`.
    assert_eq!(
        input.pos_delta,
        Vec3 {
            x: 0.25,
            y: 0.0,
            z: -0.5,
        }
    );
    assert_eq!(input.move_vector, Vec2 { x: -1.0, y: 1.0 });
    assert_eq!(input.analog_move_vector, Vec2 { x: -1.0, y: 1.0 });
    assert_eq!(input.raw_move_vector, Vec2 { x: -1.0, y: 1.0 });
    assert_eq!(
        input.camera_orientation,
        Vec3 {
            x: 0.25,
            y: -0.5,
            z: -0.75,
        }
    );
    // DoubleOptionalFunc is represented without synthetic constant fields:
    // the outer and inner presence bytes decode as `Some(None)`.
    assert_eq!(input.item_use_transaction, Some(None));
    assert_eq!(input.item_stack_request, Some(None));
    assert_eq!(input.player_block_actions, Some(None));
    assert_eq!(input.vehicle_rotation, Some(None));
    assert_eq!(input.client_predicted_vehicle, Some(None));

    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT);
}

#[test]
fn player_auth_input_builder_matches_gophertunnel_bytes_exactly() {
    let mut built = player_auth_input(PlayerAuthInputSnapshot {
        tick: 1_234,
        position: [1.25, 64.0, -2.5],
        delta: [0.25, 0.0, -0.5],
        move_vector: [-1.0, 1.0],
        analogue_move_vector: [-1.0, 1.0],
        raw_move_vector: [-1.0, 1.0],
        pitch: 10.5,
        yaw: 20.25,
        head_yaw: 30.75,
        camera_orientation: [0.25, -0.5, -0.75],
        flags: PlayerInputFlags::UP
            | PlayerInputFlags::LEFT
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::SPRINTING,
        input_mode: PlayerInputMode::Mouse,
    })
    .expect("valid movement snapshot");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built, &session()).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT,
        "the builder must emit the pinned gophertunnel bytes. A mismatch at \
         body offset 0x20 is `constant_4`, the `protocol.InputFlagList` \
         presence bool (input_flags.go:78); mismatches at body offsets 0x3f, \
         0x41, 0x43, 0x45 and 0x47 are the `protocol.DoubleOptionalFunc` outer \
         bools (io.go:212). gophertunnel always writes all six as true."
    );
}

#[test]
fn add_actor_fixture_maps_to_add_entity_and_round_trips_exactly() {
    let packet = decode_one(ADD_ACTOR, McpePacketName::AddActorPacket);
    match &packet.data {
        McpePacketData::AddActorPacket(entity) => {
            // `unique_id`/`runtime_id` -> `target_actor_id`/`target_runtime_id`,
            // both single-field wrapper structs now.
            assert_eq!(
                entity.target_actor_id,
                ActorUniqueId {
                    actor_unique_id: -77
                }
            );
            assert_eq!(
                entity.target_runtime_id,
                ActorRuntimeId {
                    actor_runtime_id: 77
                }
            );
            // `entity_type` -> `actor_type`.
            assert_eq!(entity.actor_type, "minecraft:pig");
            assert_eq!(
                entity.position,
                Vec3 {
                    x: 2.0,
                    y: 65.0,
                    z: -3.0,
                }
            );
            assert_eq!(
                entity.velocity,
                Vec3 {
                    x: 0.1,
                    y: 0.2,
                    z: 0.3,
                }
            );
            // `pitch`/`yaw` -> `rotation: Vec2 { x, y }`.
            assert_eq!(entity.rotation, Vec2 { x: 1.0, y: 2.0 });
            // `head_yaw`/`body_yaw` -> `y_head_rotation`/`y_body_rotation`.
            assert_eq!(entity.y_head_rotation, 3.0);
            assert_eq!(entity.y_body_rotation, 4.0);
            // `attributes` -> `attributes_list`.
            assert!(entity.attributes_list.is_empty());
            // `metadata` -> `actor_data.data`.
            assert!(entity.actor_data.data.is_empty());
            // `properties.ints`/`.floats` -> `synched_properties.*_entries_list`.
            assert!(entity.synched_properties.int_entries_list.is_empty());
            assert!(entity.synched_properties.float_entries_list.is_empty());
            // `links` -> `actor_links`.
            assert!(entity.actor_links.is_empty());
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, ADD_ACTOR);
}

#[test]
fn decode_preserves_sender_and_target_subclients() {
    let packet = decode_one(NETWORK_SETTINGS, McpePacketName::NetworkSettingsPacket);
    assert_eq!(
        (packet.header.from_subclient, packet.header.to_subclient),
        (1, 2)
    );
    assert_exact_round_trip(&packet, NETWORK_SETTINGS);
}

#[test]
fn decode_rejects_input_over_16_mib() {
    let mut oversized = vec![0; MAX_BATCH_BYTES + 1];
    oversized[0] = 0xfe;
    let err = decode_batch(Bytes::from(oversized), &session()).expect_err("oversized batch");
    assert!(matches!(
        err,
        ProtocolError::BatchTooLarge {
            actual,
            max: MAX_BATCH_BYTES,
        } if actual == MAX_BATCH_BYTES + 1
    ));
}

#[test]
fn decode_accepts_1600_packets_and_rejects_1601() {
    let inner = &NETWORK_SETTINGS[1..];
    let mut at_limit = BytesMut::with_capacity(1 + inner.len() * MAX_BATCH_PACKETS);
    at_limit.put_u8(0xfe);
    for _ in 0..MAX_BATCH_PACKETS {
        at_limit.extend_from_slice(inner);
    }
    let packets = decode_batch(at_limit.freeze(), &session()).expect("1,600 packets");
    assert_eq!(packets.len(), MAX_BATCH_PACKETS);

    let mut over_limit = BytesMut::with_capacity(1 + inner.len() * (MAX_BATCH_PACKETS + 1));
    over_limit.put_u8(0xfe);
    for _ in 0..=MAX_BATCH_PACKETS {
        over_limit.extend_from_slice(inner);
    }
    let err = decode_batch(over_limit.freeze(), &session()).expect_err("1,601 packets");
    assert!(matches!(
        err,
        ProtocolError::TooManyPackets {
            max: MAX_BATCH_PACKETS
        }
    ));
}

#[test]
fn decode_rejects_truncated_length() {
    assert!(decode_batch(Bytes::from_static(&[0xfe, 0x80]), &session()).is_err());
}

#[test]
fn decode_rejects_truncated_header() {
    assert!(decode_batch(Bytes::from_static(&[0xfe, 0x01, 0x80]), &session()).is_err());
}

#[test]
fn decode_rejects_truncated_body() {
    let truncated = Bytes::copy_from_slice(&NETWORK_SETTINGS[..NETWORK_SETTINGS.len() - 1]);
    assert!(decode_batch(truncated, &session()).is_err());
}

#[test]
fn decode_rejects_trailing_byte_inside_declared_entry() {
    assert!(
        NETWORK_SETTINGS[1] < 0x7f,
        "fixture length must use one byte"
    );
    let mut malformed = NETWORK_SETTINGS.to_vec();
    malformed[1] += 1;
    malformed.push(0);
    let err = decode_batch(Bytes::from(malformed), &session()).expect_err("trailing entry byte");
    assert!(matches!(
        err,
        ProtocolError::TrailingPacketBytes { remaining: 1 }
    ));
}

#[test]
fn encode_rejects_header_data_id_mismatch() {
    let mut packet = decode_one(NETWORK_SETTINGS, McpePacketName::NetworkSettingsPacket);
    packet.header.id = McpePacketName::StartGamePacket;
    let err = encode(&packet, &session()).expect_err("mismatched header ID");
    assert!(matches!(err, ProtocolError::HeaderIdMismatch { .. }));
}

#[test]
fn encode_rejects_out_of_range_subclient_ids() {
    let packet = decode_one(NETWORK_SETTINGS, McpePacketName::NetworkSettingsPacket);

    let mut invalid_sender = packet.clone();
    invalid_sender.header.from_subclient = 4;
    assert!(matches!(
        encode(&invalid_sender, &session()).expect_err("invalid sender subclient"),
        ProtocolError::InvalidSubclient {
            sender: 4,
            target: 2,
        }
    ));

    let mut invalid_target = packet;
    invalid_target.header.to_subclient = 4;
    assert!(matches!(
        encode(&invalid_target, &session()).expect_err("invalid target subclient"),
        ProtocolError::InvalidSubclient {
            sender: 1,
            target: 4,
        }
    ));
}

const PLAYER_AUTH_INPUT_BLOCK_ACTIONS: &[u8] =
    include_bytes!("../fixtures/player_auth_input_block_actions.bin");
const PLAYER_AUTH_INPUT_BREAK_BLOCK: &[u8] =
    include_bytes!("../fixtures/player_auth_input_break_block.bin");

/// The movement half shared by every PlayerAuthInput fixture.
fn fixture_movement_snapshot() -> PlayerAuthInputSnapshot {
    PlayerAuthInputSnapshot {
        tick: 1_234,
        position: [1.25, 64.0, -2.5],
        delta: [0.25, 0.0, -0.5],
        move_vector: [-1.0, 1.0],
        analogue_move_vector: [-1.0, 1.0],
        raw_move_vector: [-1.0, 1.0],
        pitch: 10.5,
        yaw: 20.25,
        head_yaw: 30.75,
        camera_orientation: [0.25, -0.5, -0.75],
        flags: PlayerInputFlags::UP
            | PlayerInputFlags::LEFT
            | PlayerInputFlags::JUMPING
            | PlayerInputFlags::SPRINTING,
        input_mode: PlayerInputMode::Mouse,
    }
}

fn fixture_block_actions() -> protocol::BlockActions {
    let mut actions = protocol::BlockActions::new();
    actions
        .push(protocol::BlockAction {
            kind: protocol::BlockActionKind::StartDestroy,
            position: [13, 71, -29],
            face: 5,
        })
        .unwrap();
    actions
        .push(protocol::BlockAction {
            kind: protocol::BlockActionKind::PredictDestroy,
            position: [-8, 63, 21],
            face: 1,
        })
        .unwrap();
    actions
}

#[test]
fn player_auth_input_block_actions_fixture_decodes_and_round_trips_exactly() {
    use valentine::bedrock::version::v1_26_44::{EnumsPlayerActionType, PlayerBlockActionData};

    let fixture = decode_one(
        PLAYER_AUTH_INPUT_BLOCK_ACTIONS,
        McpePacketName::PlayerAuthInputPacket,
    );
    let McpePacketData::PlayerAuthInputPacket(input) = &fixture.data else {
        panic!("unexpected fixture payload");
    };
    // The flag list gains exactly PerformBlockActions (ordinal 35), after
    // Sprinting (20), in ascending order.
    assert_eq!(
        input.input_data,
        Some(vec![
            EnumsPlayerAuthInputPacketPayloadInputData::Jumping,
            EnumsPlayerAuthInputPacketPayloadInputData::Up,
            EnumsPlayerAuthInputPacketPayloadInputData::Left,
            EnumsPlayerAuthInputPacketPayloadInputData::Sprinting,
            EnumsPlayerAuthInputPacketPayloadInputData::PerformBlockActions,
        ])
    );
    // Every action writes its action id, block position, and face; the outer
    // presence byte stays true and the inner Option now carries the list.
    assert_eq!(
        input.player_block_actions,
        Some(Some(vec![
            PlayerBlockActionData {
                player_action_type: EnumsPlayerActionType::StartDestroyBlock,
                position: BlockPos {
                    x: 13,
                    y: 71,
                    z: -29,
                },
                facing: 5,
            },
            PlayerBlockActionData {
                player_action_type: EnumsPlayerActionType::PredictDestroyBlock,
                position: BlockPos {
                    x: -8,
                    y: 63,
                    z: 21,
                },
                facing: 1,
            },
        ]))
    );
    assert_eq!(input.item_use_transaction, Some(None));
    assert_eq!(input.client_tick, PlayerInputTick { inputtick: 1_234 });
    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT_BLOCK_ACTIONS);
}

#[test]
fn player_auth_input_builder_embeds_block_actions_byte_exactly() {
    let interactions = protocol::PlayerAuthInputInteractions {
        block_actions: fixture_block_actions(),
        block_destroy: None,
    };
    let mut built =
        protocol::player_auth_input_with_interactions(fixture_movement_snapshot(), &interactions)
            .expect("valid block actions");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built, &session()).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT_BLOCK_ACTIONS,
        "block actions must produce the pinned gophertunnel bytes: the derived \
         PerformBlockActions flag in ascending list position and one \
         action/position/face triple per entry"
    );
}

#[test]
fn player_auth_input_break_block_fixture_decodes_and_round_trips_exactly() {
    use valentine::bedrock::version::v1_26_44::{
        EnumsItemUseInventoryTransactionActionType,
        EnumsItemUseInventoryTransactionClientCooldownState,
        EnumsItemUseInventoryTransactionPredictedResult,
        EnumsItemUseInventoryTransactionTriggerType,
    };

    let fixture = decode_one(
        PLAYER_AUTH_INPUT_BREAK_BLOCK,
        McpePacketName::PlayerAuthInputPacket,
    );
    let McpePacketData::PlayerAuthInputPacket(input) = &fixture.data else {
        panic!("unexpected fixture payload");
    };
    assert_eq!(
        input.input_data,
        Some(vec![
            EnumsPlayerAuthInputPacketPayloadInputData::Jumping,
            EnumsPlayerAuthInputPacketPayloadInputData::Up,
            EnumsPlayerAuthInputPacketPayloadInputData::Left,
            EnumsPlayerAuthInputPacketPayloadInputData::Sprinting,
            EnumsPlayerAuthInputPacketPayloadInputData::PerformItemInteraction,
        ])
    );
    assert_eq!(input.player_block_actions, Some(None));
    let Some(Some(packed)) = &input.item_use_transaction else {
        panic!("expected an embedded item-use transaction");
    };
    assert_eq!(packed.legacy_request_id.id, 0);
    assert!(packed.legacy_set_item_slots.is_none());
    let transaction = packed
        .item_use_transaction
        .as_ref()
        .expect("outer transaction presence");
    // gophertunnel's PlayerInventoryAction writes the action list behind a
    // second optional layer that the break-block fixture leaves absent.
    assert_eq!(transaction.actions.actions, None);
    assert_eq!(
        transaction.action_type,
        EnumsItemUseInventoryTransactionActionType::Destroy
    );
    assert_eq!(
        transaction.trigger_type,
        EnumsItemUseInventoryTransactionTriggerType::PlayerInput
    );
    assert_eq!(
        transaction.position,
        BlockPos {
            x: 24,
            y: 68,
            z: -41
        }
    );
    assert_eq!(transaction.face, 3);
    assert_eq!(transaction.slot, 5);
    assert_eq!(transaction.item.id, 9);
    assert_eq!(transaction.item.stacksize, 3);
    assert_eq!(transaction.item.net_id_variant, Some(15));
    assert_eq!(
        transaction.from_position,
        Vec3 {
            x: 24.625,
            y: 69.5,
            z: -40.125
        }
    );
    assert_eq!(
        transaction.click_position,
        Vec3 {
            x: 0.625,
            y: 0.375,
            z: 0.875
        }
    );
    assert_eq!(transaction.target_block_id, 654_321);
    assert_eq!(
        transaction.client_interact_prediction,
        EnumsItemUseInventoryTransactionPredictedResult::Failure
    );
    assert_eq!(
        transaction.client_cooldown_state,
        EnumsItemUseInventoryTransactionClientCooldownState::Off
    );
    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT_BREAK_BLOCK);
}

#[test]
fn player_auth_input_builder_embeds_creative_break_byte_exactly() {
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    let fixture = decode_one(
        PLAYER_AUTH_INPUT_BREAK_BLOCK,
        McpePacketName::PlayerAuthInputPacket,
    );
    let McpePacketData::PlayerAuthInputPacket(input) = &fixture.data else {
        panic!("unexpected fixture payload");
    };
    let item = &input
        .item_use_transaction
        .as_ref()
        .and_then(Option::as_ref)
        .and_then(|packed| packed.item_use_transaction.as_ref())
        .expect("embedded transaction")
        .item;
    let digest: [u8; 32] = Sha256::digest(&item.user_data_buffer).into();
    let selected_item = protocol::VerifiedNetworkItemStack::try_new(
        protocol::NetworkItemStack {
            network_id: i32::from(item.id),
            metadata: u32::from_ne_bytes(item.auxvalue.to_ne_bytes()),
            stack_network_id: item.net_id_variant.unwrap_or(-1),
            count: item.stacksize,
            nbt_digest: digest,
            block_runtime_id: i32::from_ne_bytes(item.block_runtime_id.to_ne_bytes()),
            extra_data: Arc::from(item.user_data_buffer.clone()),
        },
        digest,
    )
    .expect("fixture item is verified");
    let interactions = protocol::PlayerAuthInputInteractions {
        block_actions: protocol::BlockActions::new(),
        block_destroy: Some(protocol::BlockUseRequest {
            block_position: [24, 68, -41],
            face: 3,
            selected_slot: 5,
            selected_item,
            player_position: [24.625, 69.5, -40.125],
            relative_hit: [0.625, 0.375, 0.875],
            block_runtime_id: 654_321,
        }),
    };
    let mut built =
        protocol::player_auth_input_with_interactions(fixture_movement_snapshot(), &interactions)
            .expect("valid creative break");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built, &session()).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT_BREAK_BLOCK,
        "the embedded break-block transaction must produce the pinned \
         gophertunnel bytes, including the derived PerformItemInteraction flag \
         and the absent inner action list"
    );
}

#[test]
fn directly_asserted_interaction_flags_are_rejected() {
    for flag in [
        PlayerInputFlags::PERFORM_BLOCK_ACTIONS,
        PlayerInputFlags::PERFORM_ITEM_INTERACTION,
    ] {
        let mut snapshot = fixture_movement_snapshot();
        snapshot.flags |= flag;
        assert_eq!(
            protocol::player_auth_input(snapshot),
            Err(protocol::PlayerAuthInputError::Interaction(
                protocol::InteractionEncodeError::InconsistentInteractionFlags
            ))
        );
    }
}

const PLAYER_AUTH_INPUT_BLOCK_ACTIONS_AND_BREAK_BLOCK: &[u8] =
    include_bytes!("../fixtures/player_auth_input_block_actions_and_break_block.bin");

/// The verified selected item carried by the break-block fixtures.
fn fixture_break_block_item() -> protocol::VerifiedNetworkItemStack {
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    let fixture = decode_one(
        PLAYER_AUTH_INPUT_BREAK_BLOCK,
        McpePacketName::PlayerAuthInputPacket,
    );
    let McpePacketData::PlayerAuthInputPacket(input) = &fixture.data else {
        panic!("unexpected fixture payload");
    };
    let item = &input
        .item_use_transaction
        .as_ref()
        .and_then(Option::as_ref)
        .and_then(|packed| packed.item_use_transaction.as_ref())
        .expect("embedded transaction")
        .item;
    let digest: [u8; 32] = Sha256::digest(&item.user_data_buffer).into();
    protocol::VerifiedNetworkItemStack::try_new(
        protocol::NetworkItemStack {
            network_id: i32::from(item.id),
            metadata: u32::from_ne_bytes(item.auxvalue.to_ne_bytes()),
            stack_network_id: item.net_id_variant.unwrap_or(-1),
            count: item.stacksize,
            nbt_digest: digest,
            block_runtime_id: i32::from_ne_bytes(item.block_runtime_id.to_ne_bytes()),
            extra_data: Arc::from(item.user_data_buffer.clone()),
        },
        digest,
    )
    .expect("fixture item is verified")
}

fn fixture_break_block_request() -> protocol::BlockUseRequest {
    protocol::BlockUseRequest {
        block_position: [24, 68, -41],
        face: 3,
        selected_slot: 5,
        selected_item: fixture_break_block_item(),
        player_position: [24.625, 69.5, -40.125],
        relative_hit: [0.625, 0.375, 0.875],
        block_runtime_id: 654_321,
    }
}

/// Both optional carriers in one tick: the flag list orders
/// PerformItemInteraction (34) before PerformBlockActions (35) and the
/// item-use transaction precedes the block-action list on the wire.
#[test]
fn player_auth_input_combined_interactions_fixture_and_builder_match_byte_exactly() {
    let fixture = decode_one(
        PLAYER_AUTH_INPUT_BLOCK_ACTIONS_AND_BREAK_BLOCK,
        McpePacketName::PlayerAuthInputPacket,
    );
    let McpePacketData::PlayerAuthInputPacket(input) = &fixture.data else {
        panic!("unexpected fixture payload");
    };
    assert_eq!(
        input.input_data,
        Some(vec![
            EnumsPlayerAuthInputPacketPayloadInputData::Jumping,
            EnumsPlayerAuthInputPacketPayloadInputData::Up,
            EnumsPlayerAuthInputPacketPayloadInputData::Left,
            EnumsPlayerAuthInputPacketPayloadInputData::Sprinting,
            EnumsPlayerAuthInputPacketPayloadInputData::PerformItemInteraction,
            EnumsPlayerAuthInputPacketPayloadInputData::PerformBlockActions,
        ])
    );
    assert!(matches!(input.item_use_transaction, Some(Some(_))));
    assert_eq!(
        input
            .player_block_actions
            .as_ref()
            .and_then(Option::as_ref)
            .map(Vec::len),
        Some(2)
    );
    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT_BLOCK_ACTIONS_AND_BREAK_BLOCK);

    let interactions = protocol::PlayerAuthInputInteractions {
        block_actions: fixture_block_actions(),
        block_destroy: Some(fixture_break_block_request()),
    };
    let mut built =
        protocol::player_auth_input_with_interactions(fixture_movement_snapshot(), &interactions)
            .expect("valid combined interactions");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built, &session()).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT_BLOCK_ACTIONS_AND_BREAK_BLOCK,
        "both carriers must produce the pinned gophertunnel bytes in the pinned order"
    );
}

/// Invalid embedded destroy requests fail closed through the wrapper with the
/// same validation the standalone builder applies; nothing is encoded.
#[test]
fn invalid_embedded_break_block_requests_are_rejected() {
    use protocol::{BlockUsePacketError, InteractionEncodeError, PlayerAuthInputError};

    let interactions_for =
        |request: protocol::BlockUseRequest| protocol::PlayerAuthInputInteractions {
            block_actions: protocol::BlockActions::new(),
            block_destroy: Some(request),
        };
    let encode_with = |request| {
        protocol::player_auth_input_with_interactions(
            fixture_movement_snapshot(),
            &interactions_for(request),
        )
    };
    let expect_destroy_error = |request, expected: BlockUsePacketError| {
        assert_eq!(
            encode_with(request),
            Err(PlayerAuthInputError::Interaction(
                InteractionEncodeError::InvalidBlockDestroy(expected)
            ))
        );
    };

    let mut bad_face = fixture_break_block_request();
    bad_face.face = 6;
    expect_destroy_error(bad_face, BlockUsePacketError::InvalidFace(6));

    let mut bad_slot = fixture_break_block_request();
    bad_slot.selected_slot = 9;
    expect_destroy_error(bad_slot, BlockUsePacketError::InvalidSelectedSlot(9));

    let mut bad_position = fixture_break_block_request();
    bad_position.player_position[1] = f32::NAN;
    expect_destroy_error(bad_position, BlockUsePacketError::NonFinitePlayerPosition);

    let mut bad_hit = fixture_break_block_request();
    bad_hit.relative_hit = [0.5, 1.5, 0.5];
    expect_destroy_error(bad_hit, BlockUsePacketError::RelativeHitOutOfRange);

    let mut bad_runtime_id = fixture_break_block_request();
    bad_runtime_id.block_runtime_id = u64::from(u32::MAX) + 1;
    expect_destroy_error(
        bad_runtime_id,
        BlockUsePacketError::BlockRuntimeIdOutOfRange(u64::from(u32::MAX) + 1),
    );

    let mut bad_action_face = protocol::BlockActions::new();
    bad_action_face
        .push(protocol::BlockAction {
            kind: protocol::BlockActionKind::StartDestroy,
            position: [0, 0, 0],
            face: 7,
        })
        .unwrap();
    assert_eq!(
        protocol::player_auth_input_with_interactions(
            fixture_movement_snapshot(),
            &protocol::PlayerAuthInputInteractions {
                block_actions: bad_action_face,
                block_destroy: None,
            },
        ),
        Err(PlayerAuthInputError::Interaction(
            InteractionEncodeError::InvalidBlockActionFace(7)
        ))
    );
}

#[test]
fn empty_interactions_reproduce_the_pure_movement_fixture() {
    let mut built = protocol::player_auth_input_with_interactions(
        fixture_movement_snapshot(),
        &protocol::PlayerAuthInputInteractions::default(),
    )
    .expect("valid movement");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built, &session()).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT
    );
}
