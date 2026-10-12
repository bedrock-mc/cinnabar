use crate::raw_packet::decode_packet_raw;
use bedrock_protocol_1_26_51::enums::PlayerActionType;
use bedrock_protocol_1_26_51::packets::PlayerAction;
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bytes::Buf;

use super::*;

#[test]
fn dimension_ack_ingress_keeps_session_local_ack_even_with_sentinel_actor_id() {
    for runtime_id in [0, 42] {
        let packet: Packet = crate::Packet::new(PlayerAction {
            player_runtime_id: ActorRuntimeID(runtime_id),
            action: PlayerActionType::Changedimensionack,
            ..Default::default()
        });
        let mut encoded = crate::encode(&packet).unwrap();
        encoded.advance(1);
        let raw = decode_packet_raw(&mut encoded).unwrap();
        let event = decode_world_raw_with(raw, 1, super::decode_test_raw).unwrap();
        assert_eq!(event, Some(WorldEvent::DimensionChangeAck { runtime_id }));
    }
}

#[test]
fn dimension_action_ingress_skips_other_actions_and_rejects_truncation() {
    for action in [
        PlayerActionType::Startjump,
        PlayerActionType::Unknown2(i32::MIN),
        PlayerActionType::Unknown2(i32::MAX),
    ] {
        let packet: Packet = crate::Packet::new(PlayerAction {
            action,
            ..Default::default()
        });
        let mut encoded = crate::encode(&packet).unwrap();
        encoded.advance(1);
        let raw = decode_packet_raw(&mut encoded).unwrap();
        assert!(
            decode_world_raw_with(raw, 1, super::decode_test_raw)
                .unwrap()
                .is_none()
        );
    }

    let mut payload = Vec::new();
    wire::Writer::new(&mut payload)
        .write_var_u32(PacketId::PlayerAction as u32)
        .unwrap();
    let mut frame = Vec::new();
    wire::Writer::new(&mut frame)
        .write_var_u32(payload.len() as u32)
        .unwrap();
    frame.extend_from_slice(&payload);
    let truncated = decode_packet_raw(&mut bytes::Bytes::from(frame)).unwrap();
    assert!(decode_world_raw_with(truncated, 1, super::decode_test_raw).is_err());
}
