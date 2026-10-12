use super::*;
use bytes::Bytes;
use protocol::MovePlayerEvent;
use valentine::bedrock::version::v1_26_51::{
    EnumsPlayerPositionModeComponentPositionMode as MovePlayerPacketPositionMode, MovePlayerPacket,
    MovePlayerPacketView, PlayerInputTick, Vec2,
};

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
            let mut packet = MovePlayerPacket {
                position: Vec3 {
                    x: 1.0,
                    y: 2.0,
                    z: 3.0,
                },
                rotation: Vec2 { x: 4.0, y: 5.0 },
                y_head_rotation: 6.0,
                ..Default::default()
            };
            match field_index {
                0 => packet.position.x = non_finite,
                1 => packet.position.y = non_finite,
                2 => packet.position.z = non_finite,
                3 => packet.rotation.x = non_finite,
                4 => packet.rotation.y = non_finite,
                5 => packet.y_head_rotation = non_finite,
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

    let valid = MovePlayerPacket {
        position: Vec3 {
            x: f32::MAX,
            y: f32::MIN,
            z: -0.0,
        },
        rotation: Vec2 {
            x: f32::MAX,
            y: f32::MIN,
        },
        y_head_rotation: f32::MAX,
        ..Default::default()
    };
    assert!(matches!(
        into_world_event(protocol::Packet::new(valid), 0),
        Ok(Some(WorldEvent::MovePlayer(_)))
    ));
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
        tick: PlayerInputTick { inputtick: 12 },
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
            mode: protocol::MovePlayerMode::Teleport,
            on_ground: true,
            teleported: true,
            source_tick: 12,
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
        let packet = MovePlayerPacket {
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
        }))
    );
}

#[test]
fn move_player_rejects_overlong_runtime_and_ridden_varint_ids() {
    let packet = MovePlayerPacket::default();
    let mut valid = BytesMut::new();
    packet.encode(&mut valid).unwrap();
    let ridden_offset = 1 + 12 + 8 + 4 + 1 + 1;
    for malformed_id in [
        [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02],
        [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x00],
    ] {
        let mut malformed_runtime = BytesMut::new();
        malformed_runtime.extend_from_slice(&malformed_id);
        malformed_runtime.extend_from_slice(&valid[1..]);
        let malformed_runtime = malformed_runtime.freeze();
        assert!(MovePlayerPacket::decode(&mut malformed_runtime.clone(), ()).is_err());
        assert!(MovePlayerPacketView::decode(&mut malformed_runtime.clone()).is_err());

        let mut malformed_ridden = BytesMut::new();
        malformed_ridden.extend_from_slice(&valid[..ridden_offset]);
        malformed_ridden.extend_from_slice(&malformed_id);
        malformed_ridden.extend_from_slice(&valid[ridden_offset + 1..]);
        let malformed_ridden = malformed_ridden.freeze();
        assert!(MovePlayerPacket::decode(&mut malformed_ridden.clone(), ()).is_err());
        assert!(MovePlayerPacketView::decode(&mut malformed_ridden.clone()).is_err());
    }
}

#[test]
fn move_player_accepts_canonical_u64_max_runtime_and_ridden_ids_exactly() {
    let packet = MovePlayerPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: u64::MAX,
        },
        riding_runtime_id: ActorRuntimeId {
            actor_runtime_id: u64::MAX,
        },
        ..Default::default()
    };
    let mut encoded = BytesMut::new();
    packet.encode(&mut encoded).unwrap();
    let canonical_max = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01];
    assert_eq!(&encoded[..10], &canonical_max);
    let ridden_offset = 10 + 12 + 8 + 4 + 1 + 1;
    assert_eq!(&encoded[ridden_offset..ridden_offset + 10], &canonical_max);

    let wire = encoded.freeze();
    let mut owned_body = wire.clone();
    let decoded = MovePlayerPacket::decode(&mut owned_body, ()).unwrap();
    assert_eq!(decoded, packet);
    assert!(!owned_body.has_remaining());
    let mut reencoded = BytesMut::new();
    decoded.encode(&mut reencoded).unwrap();
    assert_eq!(reencoded.as_ref(), wire.as_ref());

    let mut borrowed_body = Bytes::copy_from_slice(&wire);
    let borrowed = MovePlayerPacketView::decode(&mut borrowed_body).unwrap();
    assert_eq!(borrowed.player_runtime_id.actor_runtime_id, u64::MAX);
    assert_eq!(borrowed.riding_runtime_id.actor_runtime_id, u64::MAX);
    assert!(!borrowed_body.has_remaining());
}
