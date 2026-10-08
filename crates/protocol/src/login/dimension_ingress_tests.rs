use bytes::Buf;
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, EnumsPlayerActionType, PlayerActionPacket,
};

use super::*;

#[test]
fn dimension_ack_ingress_keeps_session_local_ack_even_with_sentinel_actor_id() {
    let session = crate::BedrockSession { shield_item_id: 0 };
    for runtime_id in [0, 42] {
        let packet: Packet = PlayerActionPacket {
            player_runtime_id: ActorRuntimeId {
                actor_runtime_id: runtime_id,
            },
            action: EnumsPlayerActionType::Changedimensionack,
            ..Default::default()
        }
        .into();
        let mut encoded = crate::encode(&packet, &session).unwrap();
        encoded.advance(1);
        let raw = decode_packet_raw(&mut encoded).unwrap();
        let event = decode_world_raw_with(raw, 1, |raw| raw.decode(&session)).unwrap();
        assert_eq!(event, Some(WorldEvent::DimensionChangeAck { runtime_id }));
    }
}

#[test]
fn dimension_action_ingress_skips_other_actions_and_rejects_truncation() {
    let session = crate::BedrockSession { shield_item_id: 0 };
    for action in [
        EnumsPlayerActionType::Startjump,
        EnumsPlayerActionType::UnknownValue(i32::MIN),
        EnumsPlayerActionType::UnknownValue(i32::MAX),
    ] {
        let packet: Packet = PlayerActionPacket {
            action,
            ..Default::default()
        }
        .into();
        let mut encoded = crate::encode(&packet, &session).unwrap();
        encoded.advance(1);
        let raw = decode_packet_raw(&mut encoded).unwrap();
        assert!(
            decode_world_raw_with(raw, 1, |raw| raw.decode(&session))
                .unwrap()
                .is_none()
        );
    }

    let mut payload = bytes::BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::PlayerActionPacket as u32);
    let mut frame = bytes::BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    let truncated = decode_packet_raw(&mut frame.freeze()).unwrap();
    assert!(decode_world_raw_with(truncated, 1, |raw| raw.decode(&session)).is_err());
}
