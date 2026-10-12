use super::*;
use bedrock_protocol_1_26_51::enums::PlayerPositionModeComponentPositionMode as MovePlayerPacketPositionMode;
use bedrock_protocol_1_26_51::packets::MovePlayer;
use bedrock_protocol_1_26_51::types::{MovePlayerTeleportData, PlayerInputTick};
use bedrock_protocol_1_26_51::wire::{Decode, Encode};
use protocol::MovePlayerEvent;

#[test]
fn normalizes_move_player_to_the_bounded_world_surface() {
    let packet = MovePlayer {
        player_runtime_id: ActorRuntimeID(73),
        position: [-12.25, 65.5, 4096.75].into(),
        // Vec2 is (pitch, yaw); its second component is `y`, not `z`.
        rotation: [-34.5, 271.25].into(),
        y_head_rotation: wire::F32LE(99.0),
        ..Default::default()
    };

    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 2).unwrap(),
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
            riding_runtime_id: 0,
            teleport_cause: None,
        }))
    );
}

#[test]
fn move_player_rejects_every_non_finite_pose_field_and_recovers_for_valid_input() {
    for (field_index, field) in [
        "position x",
        "position y",
        "position z",
        "pitch",
        "yaw",
        "head yaw",
    ]
    .into_iter()
    .enumerate()
    {
        for non_finite in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut packet = MovePlayer {
                position: [1.0, 2.0, 3.0].into(),
                rotation: [4.0, 5.0].into(),
                y_head_rotation: wire::F32LE(6.0),
                ..Default::default()
            };
            match field_index {
                0 => packet.position.x = non_finite,
                1 => packet.position.y = non_finite,
                2 => packet.position.z = non_finite,
                3 => packet.rotation.x = non_finite,
                4 => packet.rotation.y = non_finite,
                5 => packet.y_head_rotation = wire::F32LE(non_finite),
                _ => unreachable!("the field table has six entries"),
            }

            let error = into_world_event(protocol::Packet::new(packet), 0)
                .expect_err("non-finite move player pose must be rejected");
            assert!(matches!(
                error,
                WorldPacketError::NonFiniteMovePlayerField { field: actual }
                    if actual == field
            ));
        }
    }

    let valid = MovePlayer {
        position: [f32::MAX, f32::MIN, -0.0].into(),
        rotation: [f32::MAX, f32::MIN].into(),
        y_head_rotation: wire::F32LE(f32::MAX),
        ..Default::default()
    };
    assert!(matches!(
        into_world_event(protocol::Packet::new(valid), 0),
        Ok(Some(WorldEvent::MovePlayer(_)))
    ));
}

#[test]
fn move_player_normalization_preserves_mode_tick_head_yaw_ground_ride_and_cause() {
    let packet = MovePlayer {
        player_runtime_id: ActorRuntimeID(73),
        position: [-12.25, 65.5, 4096.75].into(),
        rotation: [-34.5, 271.25].into(),
        y_head_rotation: wire::F32LE(99.0),
        position_mode: MovePlayerPacketPositionMode::Teleport,
        on_ground: true,
        riding_runtime_id: ActorRuntimeID(91),
        teleport_data: Some(MovePlayerTeleportData {
            teleportation_cause: wire::I32LE(3),
            source_actor_type: wire::I32LE(87),
        }),
        tick: PlayerInputTick(12),
    };

    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 2).unwrap(),
        Some(WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: 73,
            position: [-12.25, 65.5, 4096.75],
            pitch: -34.5,
            yaw: 271.25,
            head_yaw: 99.0,
            mode: protocol::MovePlayerMode::Teleport,
            on_ground: true,
            teleported: true,
            source_tick: 12,
            riding_runtime_id: 91,
            teleport_cause: Some(3),
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
            MovePlayerPacketPositionMode::Onlyheadrot,
            protocol::MovePlayerMode::Rotation,
        ),
        (
            MovePlayerPacketPositionMode::Unknown(9),
            protocol::MovePlayerMode::Unknown(9),
        ),
    ] {
        let packet = MovePlayer {
            position_mode: wire,
            ..Default::default()
        };
        let Some(WorldEvent::MovePlayer(event)) =
            into_world_event(protocol::Packet::new(packet), 0).unwrap()
        else {
            panic!("expected a move player event")
        };
        assert_eq!(event.mode, expected);
    }
}

#[test]
fn move_player_uses_varuint64_for_runtime_and_ridden_ids_above_u32() {
    const RUNTIME_ID: u64 = 0x1_0000_0001;
    const RIDDEN_RUNTIME_ID: u64 = 0x2_0000_0002;
    let packet = MovePlayer {
        player_runtime_id: ActorRuntimeID(RUNTIME_ID),
        position: [1.0, 2.0, 3.0].into(),
        riding_runtime_id: ActorRuntimeID(RIDDEN_RUNTIME_ID),
        ..Default::default()
    };
    let encoded = packet.encode_to_vec().unwrap();

    assert_eq!(&encoded[..5], &[0x81, 0x80, 0x80, 0x80, 0x10]);
    // 5 (runtime id) + 12 (position) + 8 (rotation) + 4 (head rotation)
    // + 1 (position mode) + 1 (on ground) == 31.
    assert_eq!(&encoded[31..36], &[0x82, 0x80, 0x80, 0x80, 0x20]);

    let decoded = MovePlayer::decode_exact(&encoded).unwrap();
    assert_eq!(decoded.player_runtime_id.0, RUNTIME_ID);
    assert_eq!(decoded.riding_runtime_id.0, RIDDEN_RUNTIME_ID);

    assert_eq!(
        into_world_event(protocol::Packet::new(decoded), 0).unwrap(),
        Some(WorldEvent::MovePlayer(MovePlayerEvent {
            runtime_id: RUNTIME_ID,
            position: [1.0, 2.0, 3.0],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            mode: protocol::MovePlayerMode::Normal,
            on_ground: false,
            teleported: false,
            source_tick: 0,
            riding_runtime_id: RIDDEN_RUNTIME_ID,
            teleport_cause: None,
        }))
    );
}

#[test]
fn move_player_rejects_overlong_runtime_and_ridden_varint_ids() {
    let packet = MovePlayer::default();
    let valid = packet.encode_to_vec().unwrap();
    let ridden_offset = 1 + 12 + 8 + 4 + 1 + 1;
    for malformed_id in [
        [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
        [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
    ] {
        let mut malformed_runtime = malformed_id.to_vec();
        malformed_runtime.extend_from_slice(&valid[1..]);
        assert!(MovePlayer::decode_exact(&malformed_runtime).is_err());

        let mut malformed_ridden = valid[..ridden_offset].to_vec();
        malformed_ridden.extend_from_slice(&malformed_id);
        malformed_ridden.extend_from_slice(&valid[ridden_offset + 1..]);
        assert!(MovePlayer::decode_exact(&malformed_ridden).is_err());
    }
}

#[test]
fn move_player_accepts_canonical_u64_max_runtime_and_ridden_ids_exactly() {
    let packet = MovePlayer {
        player_runtime_id: ActorRuntimeID(u64::MAX),
        riding_runtime_id: ActorRuntimeID(u64::MAX),
        ..Default::default()
    };
    let encoded = packet.encode_to_vec().unwrap();
    let canonical_max = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
    assert_eq!(&encoded[..10], &canonical_max);
    let ridden_offset = 10 + 12 + 8 + 4 + 1 + 1;
    assert_eq!(&encoded[ridden_offset..ridden_offset + 10], &canonical_max);

    let decoded = MovePlayer::decode_exact(&encoded).unwrap();
    assert_eq!(decoded, packet);
    assert_eq!(decoded.encode_to_vec().unwrap(), encoded);
}
