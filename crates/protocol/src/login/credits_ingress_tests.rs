use std::cell::Cell;

use bytes::BytesMut;
use jolyne::raw::{RawPacket, decode_packet_raw};
use valentine::bedrock::{
    context::BedrockSession,
    version::v1_26_51::{ActorRuntimeId, ShowCreditsPacket},
};

fn raw_credits(runtime_id: u64, credits_state: i32) -> RawPacket {
    let packet: crate::Packet = ShowCreditsPacket {
        player_runtime_id: ActorRuntimeId {
            actor_runtime_id: runtime_id,
        },
        credits_state,
    }
    .into();
    let mut frame = BytesMut::new();
    packet
        .data
        .encode_inner_bytes_mut(&mut frame, 0, 0)
        .unwrap();
    decode_packet_raw(&mut frame.freeze()).unwrap()
}

#[test]
fn credits_start_reaches_raw_world_ingress_with_the_addressed_actor() {
    let session = BedrockSession { shield_item_id: 0 };
    for runtime_id in [0, 71, u64::MAX] {
        let event =
            super::decode_world_raw_with(raw_credits(runtime_id, 0), 2, |raw| raw.decode(&session))
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
    let session = BedrockSession { shield_item_id: 0 };
    for credits_state in [1, -1, i32::MAX] {
        let decoded = Cell::new(false);
        let event = super::decode_world_raw_with(raw_credits(71, credits_state), 2, |raw| {
            decoded.set(true);
            raw.decode(&session)
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
    let mut payload = BytesMut::new();
    valentine::protocol::wire::write_var_u32(&mut payload, packet.id as u32);
    payload.extend_from_slice(&packet.body()[..packet.body().len() - 1]);
    let mut frame = BytesMut::new();
    valentine::protocol::wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.extend_from_slice(&payload);
    let raw = decode_packet_raw(&mut frame.freeze()).unwrap();
    let session = BedrockSession { shield_item_id: 0 };
    let error = super::decode_world_raw_with(raw, 2, |raw| raw.decode(&session))
        .expect_err("a truncated credits packet must not be skipped by the raw filter");
    let mut skipped = 0;
    assert!(super::skip_semantic_world_error(error, &mut skipped).is_err());
    assert_eq!(skipped, 0);
}
