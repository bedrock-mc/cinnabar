//! `SetActorMotion` ingress classification.
//!
//! Well-formed server impulses normalize into bounded world events,
//! non-finite components are semantic skips rather than session failures,
//! and truncated wire stays fatal per the malformed-wire contract.

use crate::raw_packet::decode_packet_raw;
use bytes::Buf;

use bedrock_protocol_1_26_51::enums::MovementEffectType;
use bedrock_protocol_1_26_51::packets::{
    MoveActorAbsolute, MoveActorDelta, MovementEffect, NetworkStackLatency, SetActorMotion,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, MoveActorAbsoluteData, MoveActorDeltaData, PlayerInputTick,
};

use super::*;

#[test]
fn server_latency_probe_remains_in_the_ordered_world_event_stream() {
    let probe = NetworkStackLatency {
        creation_time: wire::U64LE(777),
        is_from_server: true,
    };
    assert!(
        into_world_event(crate::Packet::new(probe), 0)
            .unwrap()
            .is_some()
    );
    let ignored = NetworkStackLatency {
        creation_time: wire::U64LE(888),
        is_from_server: false,
    };
    assert!(
        into_world_event(crate::Packet::new(ignored), 0)
            .unwrap()
            .is_none()
    );
}

/// A firework's glide boost reaches the ordered world stream with its stamp and kind.
#[test]
fn movement_effect_packets_normalize_through_the_world_allowlist() {
    let packet: Packet = crate::Packet::new(MovementEffect {
        target_runtime_id: ActorRuntimeID(42),
        effect_id: MovementEffectType::GlideBoost,
        effect_duration: wire::ZigZag32(20),
        tick: PlayerInputTick(7),
    });
    let mut batch = crate::encode(&packet).expect("encode movement effect");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw movement effect");
    let event = decode_world_raw_with(raw, 0, super::decode_test_raw)
        .expect("well-formed movement effect decodes")
        .expect("movement effect is allowlisted");
    assert_eq!(
        event,
        WorldEvent::MovementEffect(crate::MovementEffectEvent {
            actor_runtime_id: 42,
            kind: crate::MovementEffectKind::GlideBoost,
            duration_ticks: 20,
            tick: 7,
        })
    );
}

fn raw_motion_packet(body: &[u8]) -> crate::raw_packet::RawPacket {
    let mut payload = Vec::new();
    wire::Writer::new(&mut payload)
        .write_var_u32(PacketId::SetActorMotion as u32)
        .unwrap();
    payload.extend_from_slice(body);
    let mut frame = Vec::new();
    wire::Writer::new(&mut frame)
        .write_var_u32(payload.len() as u32)
        .unwrap();
    frame.extend_from_slice(&payload);
    decode_packet_raw(&mut bytes::Bytes::from(frame)).expect("raw packet")
}

#[test]
fn set_actor_motion_normalizes_impulses_skips_non_finite_and_keeps_truncation_fatal() {
    let impulse = |x: f32| SetActorMotion {
        target_runtime_id: ActorRuntimeID(42),
        motion: [x, 0.25, -0.75].into(),
        tick: PlayerInputTick(7),
    };

    let packet: Packet = Packet::new(impulse(1.5));
    let mut batch = crate::encode(&packet).expect("encode motion packet");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw motion packet");
    let event = decode_world_raw_with(raw, 0, super::decode_test_raw)
        .expect("well-formed motion decodes")
        .expect("motion is allowlisted");
    let WorldEvent::ActorMotion(motion) = event else {
        panic!("unexpected event {event:?}");
    };
    assert_eq!(motion.actor_runtime_id, 42);
    assert_eq!(motion.motion, [1.5, 0.25, -0.75]);
    assert_eq!(motion.tick, 7);

    for bad in [f32::NAN, f32::INFINITY] {
        let packet: Packet = Packet::new(impulse(bad));
        let mut batch = crate::encode(&packet).expect("encode non-finite motion");
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).expect("raw non-finite motion");
        let skipped = decode_world_raw_with(raw, 0, super::decode_test_raw)
            .expect("non-finite motion is not fatal");
        assert!(skipped.is_none(), "non-finite impulse must be skipped");
    }

    let truncated = raw_motion_packet(&[1]);
    assert!(
        decode_world_raw_with(truncated, 0, super::decode_test_raw).is_err(),
        "truncated motion wire stays fatal"
    );
}

#[test]
fn raw_actor_delta_keeps_server_duration_and_completion_ordering() {
    for ticks in [0, 10, u64::MAX] {
        let packet: Packet = crate::Packet::new(MoveActorDelta {
            move_data: MoveActorDeltaData {
                actor_runtime_id: ActorRuntimeID(7),
                new_position_x: Some(wire::F32LE(12.0)),
                force_completion: true,
                ticks: wire::VarULong(ticks),
                ..Default::default()
            },
        });
        let mut batch = crate::encode(&packet).unwrap();
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).unwrap();
        let Some(WorldEvent::Actor(crate::ActorEvent::Move(movement))) =
            decode_world_raw_with(raw, 2, super::decode_test_raw).unwrap()
        else {
            panic!("raw actor movement must reach its owner")
        };
        assert_eq!(
            movement.interpolation,
            crate::ActorInterpolation {
                ticks,
                force_completion: true
            }
        );
        assert_eq!(movement.position, [Some(12.0), None, None]);
        assert!(
            !movement.teleported,
            "completion ordering does not teleport"
        );
    }
}

#[test]
fn raw_absolute_completion_keeps_the_fast_path_and_default_duration() {
    let packet: Packet = crate::Packet::new(MoveActorAbsolute {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeID(7),
            header: wire::U8(1 << 3),
            position: [12.0, 0.0, 0.0].into(),
            rotation_x: wire::U8(0),
            rotation_y: wire::U8(0),
            rotation_y_head: wire::U8(0),
        },
    });
    let mut batch = crate::encode(&packet).unwrap();
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).unwrap();
    let Some(WorldEvent::Actor(crate::ActorEvent::Move(movement))) =
        decode_world_raw_with(raw, 2, |_| {
            panic!("absolute movement must keep its direct decoder")
        })
        .unwrap()
    else {
        panic!("raw actor movement must reach its owner")
    };
    assert_eq!(
        movement.interpolation,
        crate::ActorInterpolation {
            force_completion: true,
            ..Default::default()
        }
    );
    assert!(!movement.teleported);
}
