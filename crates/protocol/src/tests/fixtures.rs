//! Wire-truth harness for the pinned `.bin` fixtures.
//!
//! Every fixture under `crates/protocol/fixtures/` is produced by
//! `tools/fixturegen` through the pinned gophertunnel (Bedrock 1.26.50 /
//! protocol 2193). The bytes are the authority: this file only asserts that the
//! generated packet shapes decode to the values those bytes carry and
//! re-encode to the identical bytes.

use bedrock_protocol_1_26_51::enums::{
    GameType, InputData, InputMode, ItemUseInventoryTransactionTriggerType, NewInteractionModel,
    PacketCompressionAlgorithm, PlayerPermissionLevel, PlayerPositionModeComponentPositionMode,
};
use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, ActorUniqueID, BlockPos, ChunkPos, DimensionType, PlayerInputTick,
};
use bedrock_protocol_1_26_51::wire;
use bytes::{BufMut, Bytes, BytesMut};
use protocol::{
    GAME_VERSION, PROTOCOL_VERSION, PlayerAuthInputSnapshot, PlayerInputFlags, PlayerInputMode,
    ProtocolError, decode_batch, encode, player_auth_input,
};

const NETWORK_SETTINGS: &[u8] = include_bytes!("../../fixtures/network_settings.bin");
const START_GAME: &[u8] = include_bytes!("../../fixtures/start_game.bin");
const LEVEL_CHUNK: &[u8] = include_bytes!("../../fixtures/level_chunk.bin");
const MOVE_PLAYER: &[u8] = include_bytes!("../../fixtures/move_player.bin");
const PLAYER_AUTH_INPUT: &[u8] = include_bytes!("../../fixtures/player_auth_input.bin");
const ADD_ACTOR: &[u8] = include_bytes!("../../fixtures/add_actor.bin");
const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_BATCH_PACKETS: usize = 1_600;

/// `TeleportCauseCommand` in gophertunnel `minecraft/protocol/packet/move_player.go`.
///
/// `MovePlayerTeleportData::teleportation_cause` is a plain `i32` on the wire, so
/// the fixture's value is checked against the pinned Go constant.
const TELEPORT_CAUSE_COMMAND: i32 = 3;

/// `MovePlayerTeleportData::source_actor_type` is likewise a bare `i32`.
const SOURCE_ACTOR_TYPE: i32 = 87;

fn decode_one(fixture: &'static [u8], id: PacketId) -> protocol::Packet {
    let packets = decode_batch(Bytes::from_static(fixture)).expect("decode fixture");
    assert_eq!(packets.len(), 1);
    let packet = packets.into_iter().next().expect("one packet");
    assert_eq!(packet.header.id, id);
    assert_eq!(packet.header.from_subclient, 1);
    assert_eq!(packet.header.to_subclient, 2);
    packet
}

fn block_pos(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos {
        x: wire::ZigZag32(x),
        y: wire::ZigZag32(y),
        z: wire::ZigZag32(z),
    }
}

fn assert_exact_round_trip(packet: &protocol::Packet, fixture: &[u8]) {
    let encoded = encode(packet).expect("encode fixture");
    assert_eq!(encoded.as_ref(), fixture);
}

#[test]
fn protocol_constants_match_the_pinned_target() {
    let target: serde_json::Value =
        serde_json::from_str(include_str!("../../../../assets/bedrock-target.json")).unwrap();
    assert_eq!(GAME_VERSION, target["game_version"].as_str().unwrap());
    assert_eq!(
        i64::from(PROTOCOL_VERSION),
        target["wire_protocol"].as_i64().unwrap()
    );
}

#[test]
fn network_settings_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(NETWORK_SETTINGS, PacketId::NetworkSettings);
    match &*packet.data {
        packets::Packet::NetworkSettings(settings) => {
            assert_eq!(settings.compression_threshold.0, 512);
            // Restated, not weakened: the fixture still carries algorithm 0.
            // gophertunnel calls 0 `CompressionAlgorithmFlate`
            // (packet/network_settings.go); the generated enum names it `Zlib`.
            assert_eq!(
                settings.compression_algorithm,
                PacketCompressionAlgorithm::Zlib
            );
            assert!(settings.client_throttle_enabled);
            assert_eq!(settings.client_throttle_threshold.0, 8);
            assert_eq!(settings.client_throttle_scalar.0, 0.5);
        }
        other => panic!("unexpected variant: {:?}", other.id()),
    }
    assert_exact_round_trip(&packet, NETWORK_SETTINGS);
}

#[test]
fn start_game_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(START_GAME, PacketId::StartGame);
    match &*packet.data {
        packets::Packet::StartGame(start) => {
            assert_eq!(start.entity_id, ActorUniqueID(1));
            assert_eq!(start.runtime_id, ActorRuntimeID(2));
            assert_eq!(start.game_type, GameType::Creative);
            assert_eq!(start.position.to_array(), [1.25, 64.0, -2.5]);
            // The rotation's second component carries the yaw.
            assert_eq!(start.rotation.x, 10.5);
            assert_eq!(start.rotation.y, 20.25);

            let settings = &start.settings;
            assert_eq!(settings.seed.0, 12_345);
            // The spawn settings carry a plain dimension id; 0 is the overworld.
            assert_eq!(settings.spawn_settings.dimension.0, 0);
            assert_eq!(settings.default_spawn_block_position, block_pos(8, 64, -8));
            assert_eq!(settings.base_game_version, "1.26.50");

            // 1.26.40 wire changes carried by this fixture.
            //
            // PlayerPermissions is a single byte:
            // `io.Uint8(&pk.PlayerPermissions)` in the same file.
            assert_eq!(settings.player_permissions, PlayerPermissionLevel::Member);
            // The GameRule list is carried by `rule_data`, and this fixture
            // sends none.
            assert!(settings.rule_data.rules_list.is_empty());
            // `IsLoggingChat` is gone from the 1.26.40 LevelSettings entirely;
            // there is no field left to assert on. The exact round-trip below
            // is what proves no byte is written for it.

            assert_eq!(start.level_id, "fixture-level");
            assert_eq!(start.level_name, "Fixture World");
            // The two movement fields live in `movement_settings`.
            assert_eq!(start.movement_settings.rewind_history_size.0, 20);
            assert!(start.movement_settings.server_authoritative_block_breaking);
            assert_eq!(start.level_current_time.0, 123_456_789);
            assert_eq!(
                start.multiplayer_correlation_id,
                "00000000-0000-0000-0000-000000000001"
            );
            assert!(start.enable_item_stack_net_manager);
            assert_eq!(start.server_version, "1.26.50");
            assert!(start.block_network_ids_are_hashes);
        }
        other => panic!("unexpected variant: {:?}", other.id()),
    }
    assert_exact_round_trip(&packet, START_GAME);
}

#[test]
fn level_chunk_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(LEVEL_CHUNK, PacketId::LevelChunk);
    match &*packet.data {
        packets::Packet::LevelChunk(chunk) => {
            assert_eq!(
                chunk.chunk_position,
                ChunkPos {
                    x: wire::ZigZag32(3),
                    z: wire::ZigZag32(-4),
                }
            );
            assert_eq!(chunk.dimension_id, DimensionType(0));
            // Restated, not weakened. gophertunnel
            // packet/level_chunk.go now writes
            // `io.Varuint32(&pk.SubChunkCount)` followed by
            // `protocol.OptionalFunc(io, &pk.SubChunkLimit, io.Varint32)`, so
            // the old `-2` request-mode sentinel packed into the count is gone.
            // The fixture's request-mode chunk is therefore count 0 plus an
            // explicit limit, and the old `highest_subchunk_count == Some(24)`
            // is now that limit.
            assert_eq!(chunk.sub_chunks_count.0, 0);
            assert_eq!(
                chunk.client_request_sub_chunk_limit,
                Some(wire::ZigZag32(24))
            );
            // `blobs: Option<..>` is gone: the same file writes
            // `protocol.FuncSlice(io, &pk.BlobHashes, io.Uint64)`
            // unconditionally, gated by nothing. The old `blobs.is_none()`
            // therefore restates as "cache disabled and no hashes present".
            assert!(!chunk.cache_enabled);
            assert!(chunk.cache_metadata.is_empty());
            assert_eq!(
                chunk.serialized_chunk_data.as_ref(),
                [0xde, 0xad, 0xbe, 0xef]
            );
        }
        other => panic!("unexpected variant: {:?}", other.id()),
    }
    assert_exact_round_trip(&packet, LEVEL_CHUNK);
}

#[test]
fn move_player_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(MOVE_PLAYER, PacketId::MovePlayer);
    match &*packet.data {
        packets::Packet::MovePlayer(movement) => {
            assert_eq!(movement.player_runtime_id, ActorRuntimeID(42));
            assert_eq!(movement.position.to_array(), [1.25, 64.0, -2.5]);
            assert_eq!(movement.rotation.to_array(), [10.5, 20.25]);
            assert_eq!(movement.y_head_rotation.0, 30.75);
            assert_eq!(
                movement.position_mode,
                PlayerPositionModeComponentPositionMode::Teleport
            );
            assert!(movement.on_ground);
            assert_eq!(movement.riding_runtime_id, ActorRuntimeID(0));
            // TeleportData is now optional on the wire:
            // `protocol.OptionalMarshaler(io, &pk.TeleportData)` in
            // gophertunnel packet/move_player.go writes a presence bool first.
            // The fixture sets it, so the payload must still be present.
            let teleport = movement.teleport_data.as_ref().expect("teleport data");
            assert_eq!(teleport.teleportation_cause.0, TELEPORT_CAUSE_COMMAND);
            assert_eq!(teleport.source_actor_type.0, SOURCE_ACTOR_TYPE);
            assert_eq!(movement.tick, PlayerInputTick(1_234));
        }
        other => panic!("unexpected variant: {:?}", other.id()),
    }
    assert_exact_round_trip(&packet, MOVE_PLAYER);
}

#[test]
fn player_auth_input_fixture_decodes_and_round_trips_exactly() {
    let fixture = decode_one(PLAYER_AUTH_INPUT, PacketId::PlayerAuthInput);
    let packets::Packet::PlayerAuthInput(input) = &*fixture.data else {
        panic!("unexpected fixture payload");
    };
    assert_eq!(input.client_tick, PlayerInputTick(1_234));
    assert_eq!(input.input_mode, InputMode::Mouse);
    // gophertunnel packet/player_auth_input.go writes
    // `io.Varint32(&pk.InteractionModel)` and the fixture carries `0x02`,
    // i.e. zigzag 1 == `InteractionModelCrosshair`.
    assert_eq!(input.new_interaction_model, NewInteractionModel::Crosshair);
    // `protocol.InputFlagList` (gophertunnel minecraft/protocol/input_flags.go)
    // writes a count and then one zigzag varint per set flag ID, in ascending
    // flag-ID order.
    assert_eq!(
        input.input_data,
        vec![
            InputData::Jumping,
            InputData::Up,
            InputData::Left,
            InputData::Sprinting,
        ]
    );
    assert_eq!(input.player_rotation.to_array(), [10.5, 20.25]);
    assert_eq!(input.position.to_array(), [1.25, 64.0, -2.5]);
    assert_eq!(input.player_head_rotation.0, 30.75);
    assert_eq!(input.pos_delta.to_array(), [0.25, 0.0, -0.5]);
    assert_eq!(input.move_vector.to_array(), [-1.0, 1.0]);
    assert_eq!(input.analog_move_vector.to_array(), [-1.0, 1.0]);
    assert_eq!(input.raw_move_vector.to_array(), [-1.0, 1.0]);
    assert_eq!(input.camera_orientation.to_array(), [0.25, -0.5, -0.75]);
    assert_eq!(input.item_use_transaction, None);
    assert_eq!(input.item_stack_request, None);
    assert_eq!(input.player_block_actions, None);
    assert_eq!(input.vehicle_rotation, None);
    assert_eq!(input.client_predicted_vehicle, None);

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
        encode(&built).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT,
        "the builder must emit the pinned gophertunnel bytes"
    );
}

#[test]
fn add_actor_fixture_maps_to_add_entity_and_round_trips_exactly() {
    let packet = decode_one(ADD_ACTOR, PacketId::AddActor);
    match &*packet.data {
        packets::Packet::AddActor(entity) => {
            assert_eq!(entity.target_actor_id, ActorUniqueID(-77));
            assert_eq!(entity.target_runtime_id, ActorRuntimeID(77));
            assert_eq!(entity.actor_type, "minecraft:pig");
            assert_eq!(entity.position.to_array(), [2.0, 65.0, -3.0]);
            assert_eq!(entity.velocity.to_array(), [0.1, 0.2, 0.3]);
            assert_eq!(entity.rotation.to_array(), [1.0, 2.0]);
            assert_eq!(entity.y_head_rotation.0, 3.0);
            assert_eq!(entity.y_body_rotation.0, 4.0);
            assert!(entity.attributes_list.is_empty());
            assert!(entity.actor_data.data.is_empty());
            assert!(entity.synched_properties.int_entries_list.is_empty());
            assert!(entity.synched_properties.float_entries_list.is_empty());
            assert!(entity.actor_links.is_empty());
        }
        other => panic!("unexpected variant: {:?}", other.id()),
    }
    assert_exact_round_trip(&packet, ADD_ACTOR);
}

#[test]
fn decode_preserves_sender_and_target_subclients() {
    let packet = decode_one(NETWORK_SETTINGS, PacketId::NetworkSettings);
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
    let err = decode_batch(Bytes::from(oversized)).expect_err("oversized batch");
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
    let packets = decode_batch(at_limit.freeze()).expect("1,600 packets");
    assert_eq!(packets.len(), MAX_BATCH_PACKETS);

    let mut over_limit = BytesMut::with_capacity(1 + inner.len() * (MAX_BATCH_PACKETS + 1));
    over_limit.put_u8(0xfe);
    for _ in 0..=MAX_BATCH_PACKETS {
        over_limit.extend_from_slice(inner);
    }
    let err = decode_batch(over_limit.freeze()).expect_err("1,601 packets");
    assert!(matches!(
        err,
        ProtocolError::TooManyPackets {
            max: MAX_BATCH_PACKETS
        }
    ));
}

#[test]
fn decode_rejects_truncated_length() {
    assert!(decode_batch(Bytes::from_static(&[0xfe, 0x80])).is_err());
}

#[test]
fn decode_rejects_truncated_header() {
    assert!(decode_batch(Bytes::from_static(&[0xfe, 0x01, 0x80])).is_err());
}

#[test]
fn decode_rejects_truncated_body() {
    let truncated = Bytes::copy_from_slice(&NETWORK_SETTINGS[..NETWORK_SETTINGS.len() - 1]);
    assert!(decode_batch(truncated).is_err());
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
    let err = decode_batch(Bytes::from(malformed)).expect_err("trailing entry byte");
    assert!(matches!(
        err,
        ProtocolError::TrailingPacketBytes { remaining: 1 }
    ));
}

#[test]
fn encode_rejects_header_data_id_mismatch() {
    let mut packet = decode_one(NETWORK_SETTINGS, PacketId::NetworkSettings);
    packet.header.id = PacketId::StartGame;
    let err = encode(&packet).expect_err("mismatched header ID");
    assert!(matches!(err, ProtocolError::HeaderIdMismatch { .. }));
}

#[test]
fn encode_rejects_out_of_range_subclient_ids() {
    let packet = decode_one(NETWORK_SETTINGS, PacketId::NetworkSettings);

    let mut invalid_sender = packet.clone();
    invalid_sender.header.from_subclient = 4;
    assert!(matches!(
        encode(&invalid_sender).expect_err("invalid sender subclient"),
        ProtocolError::InvalidSubclient {
            sender: 4,
            target: 2,
        }
    ));

    let mut invalid_target = packet;
    invalid_target.header.to_subclient = 4;
    assert!(matches!(
        encode(&invalid_target).expect_err("invalid target subclient"),
        ProtocolError::InvalidSubclient {
            sender: 1,
            target: 4,
        }
    ));
}

const PLAYER_AUTH_INPUT_BLOCK_ACTIONS: &[u8] =
    include_bytes!("../../fixtures/player_auth_input_block_actions.bin");
const PLAYER_AUTH_INPUT_BREAK_BLOCK: &[u8] =
    include_bytes!("../../fixtures/player_auth_input_break_block.bin");

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
    use bedrock_protocol_1_26_51::enums::PlayerActionType;
    use bedrock_protocol_1_26_51::types::PlayerBlockActionData;

    let fixture = decode_one(PLAYER_AUTH_INPUT_BLOCK_ACTIONS, PacketId::PlayerAuthInput);
    let packets::Packet::PlayerAuthInput(input) = &*fixture.data else {
        panic!("unexpected fixture payload");
    };
    // The flag list gains exactly PerformBlockActions (ordinal 35), after
    // Sprinting (20), in ascending order.
    assert_eq!(
        input.input_data,
        vec![
            InputData::Jumping,
            InputData::Up,
            InputData::Left,
            InputData::Sprinting,
            InputData::Performblockactions,
        ]
    );
    // Every action writes its action id, block position, and face.
    assert_eq!(
        input.player_block_actions,
        Some(vec![
            PlayerBlockActionData {
                player_action_type: PlayerActionType::Startdestroyblock,
                position: block_pos(13, 71, -29),
                facing: wire::ZigZag32(5),
            },
            PlayerBlockActionData {
                player_action_type: PlayerActionType::Predictdestroyblock,
                position: block_pos(-8, 63, 21),
                facing: wire::ZigZag32(1),
            },
        ])
    );
    assert_eq!(input.item_use_transaction, None);
    assert_eq!(input.client_tick, PlayerInputTick(1_234));
    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT_BLOCK_ACTIONS);
}

#[test]
fn player_auth_input_builder_embeds_block_actions_byte_exactly() {
    let interactions = protocol::PlayerAuthInputInteractions {
        block_actions: fixture_block_actions(),
        block_interaction: None,
    };
    let mut built =
        protocol::player_auth_input_with_interactions(fixture_movement_snapshot(), &interactions)
            .expect("valid block actions");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT_BLOCK_ACTIONS,
        "block actions must produce the pinned gophertunnel bytes: the derived \
         PerformBlockActions flag in ascending list position and one \
         action/position/face triple per entry"
    );
}

#[test]
fn player_auth_input_break_block_fixture_decodes_and_round_trips_exactly() {
    use bedrock_protocol_1_26_51::enums::{
        ItemUseInventoryTransactionActionType, ItemUseInventoryTransactionClientCooldownState,
        ItemUseInventoryTransactionPredictedResult, ItemUseInventoryTransactionTriggerType,
    };

    let fixture = decode_one(PLAYER_AUTH_INPUT_BREAK_BLOCK, PacketId::PlayerAuthInput);
    let packets::Packet::PlayerAuthInput(input) = &*fixture.data else {
        panic!("unexpected fixture payload");
    };
    assert_eq!(
        input.input_data,
        vec![
            InputData::Jumping,
            InputData::Up,
            InputData::Left,
            InputData::Sprinting,
            InputData::Performiteminteraction,
        ]
    );
    assert_eq!(input.player_block_actions, None);
    let Some(packed) = &input.item_use_transaction else {
        panic!("expected an embedded item-use transaction");
    };
    assert_eq!(packed.legacy_request_id.0, 0);
    assert!(packed.legacy_set_item_slots.is_none());
    let transaction = &packed.item_use_transaction;
    assert!(transaction.actions.actions.is_empty());
    assert_eq!(
        transaction.action_type,
        ItemUseInventoryTransactionActionType::Destroy
    );
    assert_eq!(
        transaction.trigger_type,
        ItemUseInventoryTransactionTriggerType::PlayerInput
    );
    assert_eq!(transaction.position, block_pos(24, 68, -41));
    assert_eq!(transaction.face.0, 3);
    assert_eq!(transaction.slot.0, 5);
    assert_eq!(transaction.item.id.0, 9);
    assert_eq!(transaction.item.stack_size.0, 3);
    assert_eq!(transaction.item.net_id_variant, Some(wire::ZigZag32(15)));
    assert_eq!(
        transaction.from_position.to_array(),
        [24.625, 69.5, -40.125]
    );
    assert_eq!(transaction.click_position.to_array(), [0.625, 0.375, 0.875]);
    assert_eq!(transaction.target_block_id.0, 654_321);
    assert_eq!(
        transaction.client_interact_prediction,
        ItemUseInventoryTransactionPredictedResult::Failure
    );
    assert_eq!(
        transaction.client_cooldown_state,
        ItemUseInventoryTransactionClientCooldownState::Off
    );
    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT_BREAK_BLOCK);
}

#[test]
fn player_auth_input_builder_embeds_destroy_without_placement_fields() {
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    let fixture = decode_one(PLAYER_AUTH_INPUT_BREAK_BLOCK, PacketId::PlayerAuthInput);
    let packets::Packet::PlayerAuthInput(input) = &*fixture.data else {
        panic!("unexpected fixture payload");
    };
    let item = &input
        .item_use_transaction
        .as_ref()
        .map(|packed| &packed.item_use_transaction)
        .expect("embedded transaction")
        .item;
    let digest: [u8; 32] = Sha256::digest(&item.user_data_buffer).into();
    let selected_item = protocol::VerifiedNetworkItemStack::try_new(
        protocol::NetworkItemStack {
            network_id: i32::from(item.id.0),
            metadata: item.aux_value.0,
            stack_network_id: item.net_id_variant.map_or(-1, |id| id.0),
            count: item.stack_size.0,
            nbt_digest: digest,
            block_runtime_id: i32::from_ne_bytes(item.block_runtime_id.0.to_ne_bytes()),
            extra_data: Arc::from(item.user_data_buffer.as_ref()),
        },
        digest,
    )
    .expect("fixture item is verified");
    let interactions = protocol::PlayerAuthInputInteractions {
        block_actions: protocol::BlockActions::new(),
        block_interaction: Some(protocol::BlockItemInteraction::Destroy(
            protocol::BlockUseRequest {
                hand: protocol::ItemUseHand::Mainhand,
                block_position: [24, 68, -41],
                face: 3,
                selected_slot: 5,
                selected_item,
                player_position: [24.625, 69.5, -40.125],
                relative_hit: [0.625, 0.375, 0.875],
                block_runtime_id: 654_321,
            },
            None,
        )),
    };
    let mut built =
        protocol::player_auth_input_with_interactions(fixture_movement_snapshot(), &interactions)
            .expect("valid creative break");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built).expect("encode built PlayerAuthInput"),
        encode(&expected_destroy_input(PLAYER_AUTH_INPUT_BREAK_BLOCK)).unwrap(),
        "destroy preserves the fixture's selection, pose and interaction flag"
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
    include_bytes!("../../fixtures/player_auth_input_block_actions_and_break_block.bin");

/// Keeps the fixture's movement and stack while clearing fields used only for placement.
fn expected_destroy_input(bytes: &'static [u8]) -> protocol::Packet {
    let mut packet = decode_one(bytes, PacketId::PlayerAuthInput);
    let packets::Packet::PlayerAuthInput(input) = &mut *packet.data else {
        panic!("expected movement input");
    };
    let transaction = &mut input
        .item_use_transaction
        .as_mut()
        .unwrap()
        .item_use_transaction;
    transaction.trigger_type = ItemUseInventoryTransactionTriggerType::Unknown;
    transaction.click_position = [0.0; 3].into();
    transaction.target_block_id = wire::VarUInt(0);
    transaction.item.net_id_variant = None;
    packet
}

/// The verified selected item carried by the break-block fixtures.
fn fixture_break_block_item() -> protocol::VerifiedNetworkItemStack {
    use sha2::{Digest, Sha256};
    use std::sync::Arc;

    let fixture = decode_one(PLAYER_AUTH_INPUT_BREAK_BLOCK, PacketId::PlayerAuthInput);
    let packets::Packet::PlayerAuthInput(input) = &*fixture.data else {
        panic!("unexpected fixture payload");
    };
    let item = &input
        .item_use_transaction
        .as_ref()
        .map(|packed| &packed.item_use_transaction)
        .expect("embedded transaction")
        .item;
    let digest: [u8; 32] = Sha256::digest(&item.user_data_buffer).into();
    protocol::VerifiedNetworkItemStack::try_new(
        protocol::NetworkItemStack {
            network_id: i32::from(item.id.0),
            metadata: item.aux_value.0,
            stack_network_id: item.net_id_variant.map_or(-1, |id| id.0),
            count: item.stack_size.0,
            nbt_digest: digest,
            block_runtime_id: i32::from_ne_bytes(item.block_runtime_id.0.to_ne_bytes()),
            extra_data: Arc::from(item.user_data_buffer.as_ref()),
        },
        digest,
    )
    .expect("fixture item is verified")
}

fn fixture_break_block_request() -> protocol::BlockUseRequest {
    protocol::BlockUseRequest {
        hand: protocol::ItemUseHand::Mainhand,
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
fn player_auth_input_combined_interactions_preserve_actions_and_clear_placement_fields() {
    let fixture = decode_one(
        PLAYER_AUTH_INPUT_BLOCK_ACTIONS_AND_BREAK_BLOCK,
        PacketId::PlayerAuthInput,
    );
    let packets::Packet::PlayerAuthInput(input) = &*fixture.data else {
        panic!("unexpected fixture payload");
    };
    assert_eq!(
        input.input_data,
        vec![
            InputData::Jumping,
            InputData::Up,
            InputData::Left,
            InputData::Sprinting,
            InputData::Performiteminteraction,
            InputData::Performblockactions,
        ]
    );
    assert!(input.item_use_transaction.is_some());
    assert_eq!(input.player_block_actions.as_ref().map(Vec::len), Some(2));
    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT_BLOCK_ACTIONS_AND_BREAK_BLOCK);

    let interactions = protocol::PlayerAuthInputInteractions {
        block_actions: fixture_block_actions(),
        block_interaction: Some(protocol::BlockItemInteraction::Destroy(
            fixture_break_block_request(),
            None,
        )),
    };
    let mut built =
        protocol::player_auth_input_with_interactions(fixture_movement_snapshot(), &interactions)
            .expect("valid combined interactions");
    built.header.from_subclient = 1;
    built.header.to_subclient = 2;
    assert_eq!(
        encode(&built).expect("encode built PlayerAuthInput"),
        encode(&expected_destroy_input(
            PLAYER_AUTH_INPUT_BLOCK_ACTIONS_AND_BREAK_BLOCK
        ))
        .unwrap(),
        "both carriers preserve their order while destroy clears placement fields"
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
            block_interaction: Some(protocol::BlockItemInteraction::Destroy(request, None)),
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
                block_interaction: None,
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
        encode(&built).expect("encode built PlayerAuthInput"),
        PLAYER_AUTH_INPUT
    );
}
