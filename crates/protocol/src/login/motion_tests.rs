//! `SetActorMotion` ingress classification.
//!
//! Well-formed server impulses normalize into bounded world events,
//! non-finite components are semantic skips rather than session failures,
//! and truncated wire stays fatal per the malformed-wire contract.

use bytes::{Buf, BufMut, BytesMut};
use jolyne::raw::decode_packet_raw;
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_51::{
    ActorRuntimeId, EnumsMovementEffectType, McpePacketName, MoveActorAbsoluteData,
    MoveActorAbsolutePacket, MoveActorDeltaData, MoveActorDeltaPacket, MovementEffectPacket,
    NetworkStackLatencyPacket, PlayerInputTick, SetActorMotionPacket, Vec3 as WireVec3,
};

use super::*;

#[test]
fn server_latency_probe_remains_in_the_ordered_world_event_stream() {
    let probe = NetworkStackLatencyPacket {
        creation_time: 777,
        is_from_server: true,
    };
    assert!(into_world_event(probe.into(), 0).unwrap().is_some());
    let ignored = NetworkStackLatencyPacket {
        creation_time: 888,
        is_from_server: false,
    };
    assert!(into_world_event(ignored.into(), 0).unwrap().is_none());
}

/// A firework's glide boost reaches the ordered world stream with its stamp and kind.
#[test]
fn movement_effect_packets_normalize_through_the_world_allowlist() {
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = MovementEffectPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 42,
        },
        effect_id: EnumsMovementEffectType::GlideBoost,
        effect_duration: 20,
        tick: PlayerInputTick { inputtick: 7 },
    }
    .into();
    let mut batch = crate::encode(&packet, &session).expect("encode movement effect");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw movement effect");
    let event = decode_world_raw_with(raw, 0, |raw| raw.decode(&session))
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

#[test]
fn raw_actor_delta_keeps_server_duration_and_completion_ordering() {
    let session = BedrockSession { shield_item_id: 0 };
    for ticks in [0, 10, u64::MAX] {
        let packet: Packet = MoveActorDeltaPacket {
            move_data: MoveActorDeltaData {
                actor_runtime_id: ActorRuntimeId {
                    actor_runtime_id: 7,
                },
                new_position_x: Some(12.0),
                force_completion: true,
                ticks,
                ..Default::default()
            },
        }
        .into();
        let mut batch = crate::encode(&packet, &session).unwrap();
        batch.advance(1);
        let raw = decode_packet_raw(&mut batch).unwrap();
        let Some(WorldEvent::Actor(crate::ActorEvent::Move(movement))) =
            decode_world_raw_with(raw, 2, |raw| raw.decode(&session)).unwrap()
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
    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = MoveActorAbsolutePacket {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: 7,
            },
            header: 1 << 3,
            position: WireVec3 {
                x: 12.0,
                y: 0.0,
                z: 0.0,
            },
            rotation_x: 0,
            rotation_y: 0,
            rotation_y_head: 0,
        },
    }
    .into();
    let mut batch = crate::encode(&packet, &session).unwrap();
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
