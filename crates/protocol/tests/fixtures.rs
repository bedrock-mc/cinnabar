use bytes::{BufMut, Bytes, BytesMut};
use protocol::{
    BedrockSession, GAME_VERSION, PROTOCOL_VERSION, PlayerAuthInputSnapshot, PlayerInputFlags,
    PlayerInputMode, ProtocolError, decode_batch, encode, player_auth_input,
};
use valentine::bedrock::version::v1_26_30::{
    GameMode, InputFlag, LegacyEntityType, McpePacketData, McpePacketName, MovePlayerPacketMode,
    MovePlayerPacketTeleportCause, NetworkSettingsPacketCompressionAlgorithm,
    PlayerAuthInputPacketInputMode, PlayerAuthInputPacketInteractionModel,
    StartGamePacketDimension, Vec3F,
};

const NETWORK_SETTINGS: &[u8] = include_bytes!("../fixtures/network_settings.bin");
const START_GAME: &[u8] = include_bytes!("../fixtures/start_game.bin");
const LEVEL_CHUNK: &[u8] = include_bytes!("../fixtures/level_chunk.bin");
const MOVE_PLAYER: &[u8] = include_bytes!("../fixtures/move_player.bin");
const PLAYER_AUTH_INPUT: &[u8] = include_bytes!("../fixtures/player_auth_input.bin");
const ADD_ACTOR: &[u8] = include_bytes!("../fixtures/add_actor.bin");
const MAX_BATCH_BYTES: usize = 16 * 1024 * 1024;
const MAX_BATCH_PACKETS: usize = 1_600;

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
fn protocol_constants_are_pinned_to_1_26_30() {
    assert_eq!(GAME_VERSION, "1.26.30");
    assert_eq!(PROTOCOL_VERSION, 1001);
}

#[test]
fn network_settings_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(NETWORK_SETTINGS, McpePacketName::PacketNetworkSettings);
    match &packet.data {
        McpePacketData::PacketNetworkSettings(settings) => {
            assert_eq!(settings.compression_threshold, 512);
            assert_eq!(
                settings.compression_algorithm,
                NetworkSettingsPacketCompressionAlgorithm::Deflate
            );
            assert!(settings.client_throttle);
            assert_eq!(settings.client_throttle_threshold, 8);
            assert_eq!(settings.client_throttle_scalar, 0.5);
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, NETWORK_SETTINGS);
}

#[test]
fn start_game_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(START_GAME, McpePacketName::PacketStartGame);
    match &packet.data {
        McpePacketData::PacketStartGame(start) => {
            assert_eq!(start.entity_id, 1);
            assert_eq!(start.runtime_entity_id, 2);
            assert_eq!(start.player_gamemode, GameMode::Creative);
            assert_eq!(
                start.player_position,
                Vec3F {
                    x: 1.25,
                    y: 64.0,
                    z: -2.5,
                }
            );
            assert_eq!(start.rotation.x, 10.5);
            assert_eq!(start.rotation.z, 20.25);
            assert_eq!(start.seed, 12_345);
            assert_eq!(start.dimension, StartGamePacketDimension::Overworld);
            assert_eq!(start.spawn_position.x, 8);
            assert_eq!(start.spawn_position.y, 64);
            assert_eq!(start.spawn_position.z, -8);
            assert_eq!(start.game_version, "1.26.30");
            assert_eq!(start.level_id, "fixture-level");
            assert_eq!(start.world_name, "Fixture World");
            assert_eq!(start.rewind_history_size, 20);
            assert!(start.server_authoritative_block_breaking);
            assert_eq!(start.current_tick, 123_456_789);
            assert_eq!(
                start.multiplayer_correlation_id,
                "00000000-0000-0000-0000-000000000001"
            );
            assert!(start.server_authoritative_inventory);
            assert_eq!(start.engine, "1.26.30");
            assert!(start.block_network_ids_are_hashes);
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, START_GAME);
}

#[test]
fn level_chunk_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(LEVEL_CHUNK, McpePacketName::PacketLevelChunk);
    match &packet.data {
        McpePacketData::PacketLevelChunk(chunk) => {
            assert_eq!(chunk.x, 3);
            assert_eq!(chunk.z, -4);
            assert_eq!(chunk.dimension, 0);
            assert_eq!(chunk.sub_chunk_count, -2);
            assert_eq!(chunk.highest_subchunk_count, Some(24));
            assert!(chunk.blobs.is_none());
            assert_eq!(chunk.payload, [0xde, 0xad, 0xbe, 0xef]);
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, LEVEL_CHUNK);
}

#[test]
fn move_player_fixture_decodes_and_round_trips_exactly() {
    let packet = decode_one(MOVE_PLAYER, McpePacketName::PacketMovePlayer);
    match &packet.data {
        McpePacketData::PacketMovePlayer(movement) => {
            assert_eq!(movement.runtime_id, 42);
            assert_eq!(
                movement.position,
                Vec3F {
                    x: 1.25,
                    y: 64.0,
                    z: -2.5,
                }
            );
            assert_eq!(movement.pitch, 10.5);
            assert_eq!(movement.yaw, 20.25);
            assert_eq!(movement.head_yaw, 30.75);
            assert_eq!(movement.mode, MovePlayerPacketMode::Teleport);
            assert!(movement.on_ground);
            let teleport = movement.teleport.as_ref().expect("teleport fields");
            assert_eq!(teleport.cause, MovePlayerPacketTeleportCause::Command);
            assert_eq!(teleport.source_entity_type, LegacyEntityType::EnderPearl);
            assert_eq!(movement.tick, 1_234);
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, MOVE_PLAYER);
}

#[test]
fn player_auth_input_builder_matches_gophertunnel_bytes_exactly() {
    let fixture = decode_one(PLAYER_AUTH_INPUT, McpePacketName::PacketPlayerAuthInput);
    let McpePacketData::PacketPlayerAuthInput(input) = &fixture.data else {
        panic!("unexpected fixture payload");
    };
    assert_eq!(input.tick, 1_234);
    assert_eq!(input.input_mode, PlayerAuthInputPacketInputMode::Mouse);
    assert_eq!(
        input.interaction_model,
        PlayerAuthInputPacketInteractionModel::Unknown(-1)
    );
    assert_eq!(
        input.input_data,
        InputFlag::UP | InputFlag::LEFT | InputFlag::JUMPING | InputFlag::SPRINTING
    );
    assert_exact_round_trip(&fixture, PLAYER_AUTH_INPUT);

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
        PLAYER_AUTH_INPUT
    );
}

#[test]
fn add_actor_fixture_maps_to_add_entity_and_round_trips_exactly() {
    let packet = decode_one(ADD_ACTOR, McpePacketName::PacketAddEntity);
    match &packet.data {
        McpePacketData::PacketAddEntity(entity) => {
            assert_eq!(entity.unique_id, -77);
            assert_eq!(entity.runtime_id, 77);
            assert_eq!(entity.entity_type, "minecraft:pig");
            assert_eq!(
                entity.position,
                Vec3F {
                    x: 2.0,
                    y: 65.0,
                    z: -3.0,
                }
            );
            assert_eq!(entity.pitch, 1.0);
            assert_eq!(entity.yaw, 2.0);
            assert_eq!(entity.head_yaw, 3.0);
            assert_eq!(entity.body_yaw, 4.0);
            assert!(entity.attributes.is_empty());
            assert!(entity.metadata.is_empty());
            assert!(entity.properties.ints.is_empty());
            assert!(entity.properties.floats.is_empty());
            assert!(entity.links.is_empty());
        }
        other => panic!("unexpected variant: {:?}", other.packet_id()),
    }
    assert_exact_round_trip(&packet, ADD_ACTOR);
}

#[test]
fn decode_preserves_sender_and_target_subclients() {
    let packet = decode_one(NETWORK_SETTINGS, McpePacketName::PacketNetworkSettings);
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
    let mut packet = decode_one(NETWORK_SETTINGS, McpePacketName::PacketNetworkSettings);
    packet.header.id = McpePacketName::PacketStartGame;
    let err = encode(&packet, &session()).expect_err("mismatched header ID");
    assert!(matches!(err, ProtocolError::HeaderIdMismatch { .. }));
}

#[test]
fn encode_rejects_out_of_range_subclient_ids() {
    let packet = decode_one(NETWORK_SETTINGS, McpePacketName::PacketNetworkSettings);

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
