//! Optional experience envelopes must pass through encoded world ingress.
use super::*;
use crate::raw_packet::decode_packet_raw;
use bedrock_protocol_1_26_51::packets::ScriptMessage;
use bytes::Buf;

/// Encodes a packet and traverses the same raw admission path as the socket reader.
fn ingress(packet: Packet) -> Option<WorldEvent> {
    let mut batch = crate::encode(&packet).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    decode_world_raw_with(raw, 0, super::decode_test_raw).unwrap()
}

#[test]
fn experience_replies_cross_raw_ingress_with_existing_bounds_and_routes() {
    let bytes = b"accept fixture".to_vec();
    assert_eq!(
        ingress(crate::experience_packet(bytes.clone()).unwrap()),
        Some(WorldEvent::Experience(crate::ExperienceMessage { bytes }))
    );
    for (channel, bytes) in [
        ("unrelated", vec![1]),
        (
            crate::EXPERIENCE_CHANNEL,
            vec![0; crate::MAX_EXPERIENCE_ENVELOPE_BYTES + 1],
        ),
    ] {
        assert!(
            ingress(crate::Packet::new(ScriptMessage {
                message_id: channel.into(),
                message_value: bytes.into(),
            }))
            .is_none()
        );
    }
    let mut packet = crate::experience_packet(vec![1]).unwrap();
    packet.header.to_subclient = 1;
    assert!(ingress(packet).is_none());
}
