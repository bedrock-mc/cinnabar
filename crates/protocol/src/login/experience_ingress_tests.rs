//! Optional experience envelopes must pass through encoded world ingress.
use super::*;
use bytes::Buf;
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::{context::BedrockSession, version::v1_26_51::ScriptMessagePacket};

/// Encodes a packet and traverses the same raw admission path as the socket reader.
fn ingress(packet: Packet) -> Option<WorldEvent> {
    let session = BedrockSession { shield_item_id: 0 };
    let mut batch = crate::encode(&packet, &session).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    decode_world_raw_with(raw, 0, |raw| raw.decode(&session)).unwrap()
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
            ingress(
                ScriptMessagePacket {
                    message_id: channel.into(),
                    message_value: bytes,
                }
                .into()
            )
            .is_none()
        );
    }
    let mut packet = crate::experience_packet(vec![1]).unwrap();
    packet.header.to_subclient = 1;
    assert!(ingress(packet).is_none());
}
