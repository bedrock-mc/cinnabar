//! `SetActorMotion` ingress classification.
//!
//! Well-formed server impulses normalize into bounded world events,
//! non-finite components are semantic skips rather than session failures,
//! and truncated wire stays fatal per the malformed-wire contract.

use bytes::{Buf, BufMut, BytesMut};
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_44::{
    ActorRuntimeId, McpePacketName, PlayerInputTick, SetActorMotionPacket, Vec3 as WireVec3,
};

use super::*;

fn raw_motion_packet(body: &[u8]) -> jolyne::raw::RawPacket {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::SetActorMotionPacket as u32);
    payload.put_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.put_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw packet")
}

#[test]
fn set_actor_motion_normalizes_impulses_skips_non_finite_and_keeps_truncation_fatal() {
    let session = BedrockSession { shield_item_id: 0 };
    let impulse = |x: f32| SetActorMotionPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 42,
        },
        motion: WireVec3 {
            x,
            y: 0.25,
            z: -0.75,
        },
        tick: PlayerInputTick { inputtick: 7 },
    };

    let packet: Packet = impulse(1.5).into();
    let mut batch = crate::encode(&packet, &session).expect("encode motion packet");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw motion packet");
    let event = decode_world_raw_with(raw, 0, |raw| raw.decode(&session))
        .expect("well-formed motion decodes")
        .expect("motion is allowlisted");
    let WorldEvent::ActorMotion(motion) = event else {
        panic!("unexpected event {event:?}");
    };
    assert_eq!(motion.actor_runtime_id, 42);
    assert_eq!(motion.motion, [1.5, 0.25, -0.75]);
    assert_eq!(motion.tick, 7);

    for bad in [f32::NAN, f32::INFINITY] {
        let packet: Packet = impulse(bad).into();
        let mut batch = crate::encode(&packet, &session).expect("encode non-finite motion");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw non-finite motion");
        let skipped = decode_world_raw_with(raw, 0, |raw| raw.decode(&session))
            .expect("non-finite motion is not fatal");
        assert!(skipped.is_none(), "non-finite impulse must be skipped");
    }

    let truncated = raw_motion_packet(&[1]);
    assert!(
        decode_world_raw_with(truncated, 0, |raw| raw.decode(&session)).is_err(),
        "truncated motion wire stays fatal"
    );
}
