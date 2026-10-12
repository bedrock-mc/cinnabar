use std::cell::Cell;

use crate::raw_packet::{RawPacket, decode_packet_raw};
use bedrock_protocol_1_26_51::packets::ShowCredits;
use bedrock_protocol_1_26_51::types::ActorRuntimeID;
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;

fn raw_credits(runtime_id: u64, credits_state: i32) -> RawPacket {
    let packet: crate::Packet = crate::Packet::new(ShowCredits {
        player_runtime_id: ActorRuntimeID(runtime_id),
        credits_state: wire::ZigZag32(credits_state),
    });
    let mut frame = Vec::new();
    crate::codec::encode_entry(&packet, &mut frame).unwrap();
    decode_packet_raw(&mut Bytes::from(frame)).unwrap()
}

#[test]
fn credits_start_reaches_raw_world_ingress_with_the_addressed_actor() {
    for runtime_id in [0, 71, u64::MAX] {
        let event =
            super::decode_world_raw_with(raw_credits(runtime_id, 0), 2, super::decode_test_raw)
                .unwrap();
        assert_eq!(
            event,
            Some(crate::WorldEvent::Ui(crate::UiEvent::ShowCredits(
                crate::ShowCreditsEvent { runtime_id }
            )))
        );
    }
}

#[test]
fn non_start_credits_states_are_decoded_then_ignored_without_session_failure() {
    for credits_state in [1, -1, i32::MAX] {
        let decoded = Cell::new(false);
        let event = super::decode_world_raw_with(raw_credits(71, credits_state), 2, |raw| {
            decoded.set(true);
            super::decode_test_raw(raw)
        })
        .unwrap();
        assert!(
            decoded.get(),
            "credits framing must be validated before skipping its state"
        );
        assert!(event.is_none());
    }
}

#[test]
fn truncated_credits_are_a_fatal_wire_fault() {
    let packet = raw_credits(71, 0);
    let mut payload = Vec::new();
    wire::Writer::new(&mut payload)
        .write_var_u32(packet.id as u32)
        .unwrap();
    payload.extend_from_slice(&packet.body()[..packet.body().len() - 1]);
    let mut frame = Vec::new();
    wire::Writer::new(&mut frame)
        .write_var_u32(payload.len() as u32)
        .unwrap();
    frame.extend_from_slice(&payload);
    let raw = decode_packet_raw(&mut Bytes::from(frame)).unwrap();
    let error = super::decode_world_raw_with(raw, 2, super::decode_test_raw)
        .expect_err("a truncated credits packet must not be skipped by the raw filter");
    let mut skipped = 0;
    assert!(super::skip_semantic_world_error(error, &mut skipped).is_err());
    assert_eq!(skipped, 0);
}
