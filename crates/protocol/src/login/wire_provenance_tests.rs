use bytes::{Buf, BufMut, BytesMut};
use jolyne::raw::{RawPacket, decode_packet_raw};
use valentine::bedrock::context::BedrockSession;
use valentine::bedrock::version::v1_26_44::{McpePacketName, SetTimePacket};
use valentine::protocol::wire;

use super::{decode_world_raw_with, skip_semantic_world_error};
use crate::{Packet, ProtocolError, WorldEvent};

/// Builds one raw packet frame around a test-owned body.
fn raw_packet(id: McpePacketName, body: &[u8]) -> RawPacket {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, id as u32);
    payload.put_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.put_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw packet")
}

/// Builds complete non-air MobEquipment wire with controlled retained user data.
fn raw_mob_equipment(count: u16, extra: &[u8], container_id: i8) -> RawPacket {
    let mut body = BytesMut::new();
    wire::write_var_u64(&mut body, 42);
    body.put_i16_le(5);
    body.put_u16_le(count);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    wire::write_var_u32(&mut body, extra.len() as u32);
    body.put_slice(extra);
    body.put_u8(0);
    body.put_u8(0);
    body.put_i8(container_id);
    raw_packet(McpePacketName::MobEquipmentPacket, &body)
}

#[test]
fn malformed_inner_world_wire_is_never_counted_as_a_semantic_skip() {
    let mut world_skips = 0;
    let error = ProtocolError::World(crate::WorldPacketError::Wire(
        crate::WorldWireError::Inventory(crate::InventoryPacketError::MalformedWire),
    ));

    let returned = skip_semantic_world_error(error, &mut world_skips)
        .expect_err("malformed inner wire must remain session-fatal");

    assert_eq!(world_skips, 0);
    assert!(matches!(
        returned,
        ProtocolError::World(crate::WorldPacketError::Wire(
            crate::WorldWireError::Inventory(crate::InventoryPacketError::MalformedWire)
        ))
    ));
}

#[test]
fn truncated_inventory_equipment_and_actor_bodies_are_inner_wire_fatal() {
    let cases = [
        (
            McpePacketName::InventoryContentPacket,
            &[0x00, 0x00][..],
            "InventoryContent",
        ),
        (
            McpePacketName::InventorySlotPacket,
            &[0x00][..],
            "InventorySlot",
        ),
        (
            McpePacketName::ItemStackResponsePacket,
            &[0x01][..],
            "ItemStackResponse",
        ),
        (
            McpePacketName::MobEquipmentPacket,
            &[0x2a][..],
            "MobEquipment",
        ),
    ];

    for (id, body, label) in cases {
        let error = decode_world_raw_with(raw_packet(id, body), 0, |raw| {
            raw.decode(&BedrockSession { shield_item_id: 0 })
        })
        .expect_err(label);
        assert!(
            matches!(
                error,
                ProtocolError::World(crate::WorldPacketError::Wire(_))
            ),
            "unexpected {label} classification: {error:?}"
        );
    }

    let mut absolute = BytesMut::new();
    wire::write_var_u64(&mut absolute, 42);
    absolute.extend_from_slice(&[0; 16]);
    let overlong = [&absolute[..], &[0xff]].concat();
    for body in [&absolute[..15], overlong.as_slice()] {
        let error = decode_world_raw_with(
            raw_packet(McpePacketName::MoveActorAbsolutePacket, body),
            0,
            |_| unreachable!("absolute movement uses the raw decoder"),
        )
        .expect_err("invalid absolute movement length");
        assert!(matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Wire(crate::WorldWireError::Actor(
                _
            )))
        ));
    }
}

#[test]
fn complete_item_descriptor_with_truncated_nbt_is_inner_wire_fatal() {
    let malformed_nbt = [0xff, 0xff, 1, 10];
    let error = decode_world_raw_with(raw_mob_equipment(1, &malformed_nbt, 0), 0, |raw| {
        raw.decode(&BedrockSession { shield_item_id: 0 })
    })
    .expect_err("truncated item NBT");

    assert!(matches!(
        error,
        ProtocolError::World(crate::WorldPacketError::Wire(crate::WorldWireError::Item(
            crate::ItemPacketError::InvalidItemNbt
        )))
    ));
}

#[test]
fn contradictory_empty_equipment_still_scans_declared_wire_before_semantic_skip() {
    let mut missing_stack_id = BytesMut::new();
    wire::write_var_u64(&mut missing_stack_id, 42);
    missing_stack_id.put_i16_le(0);
    missing_stack_id.put_u16_le(0);
    wire::write_var_u32(&mut missing_stack_id, 0);
    missing_stack_id.put_u8(1);

    let mut missing_extra = BytesMut::new();
    wire::write_var_u64(&mut missing_extra, 42);
    missing_extra.put_i16_le(0);
    missing_extra.put_u16_le(0);
    wire::write_var_u32(&mut missing_extra, 0);
    missing_extra.put_u8(0);
    wire::write_var_u32(&mut missing_extra, 0);
    wire::write_var_u32(&mut missing_extra, 2);

    for body in [missing_stack_id, missing_extra] {
        let error = decode_world_raw_with(
            raw_packet(McpePacketName::MobEquipmentPacket, &body),
            0,
            |_| unreachable!("empty equipment uses the raw decoder"),
        )
        .expect_err("truncated declared empty-equipment field");
        assert!(matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Wire(crate::WorldWireError::Item(
                crate::ItemPacketError::MalformedWire
            )))
        ));
    }

    let mut complete_contradiction = BytesMut::new();
    wire::write_var_u64(&mut complete_contradiction, 42);
    complete_contradiction.put_i16_le(0);
    complete_contradiction.put_u16_le(0);
    wire::write_var_u32(&mut complete_contradiction, 0);
    complete_contradiction.put_u8(1);
    wire::write_var_u32(&mut complete_contradiction, 7);
    wire::write_var_u32(&mut complete_contradiction, 0);
    wire::write_var_u32(&mut complete_contradiction, 2);
    complete_contradiction.extend_from_slice(&[0xaa, 0xbb, 0, 0, 0]);
    let semantic = decode_world_raw_with(
        raw_packet(McpePacketName::MobEquipmentPacket, &complete_contradiction),
        0,
        |_| unreachable!("empty equipment uses the raw decoder"),
    )
    .expect_err("complete contradictory descriptor is semantic");
    assert!(matches!(
        semantic,
        ProtocolError::World(crate::WorldPacketError::Item(
            crate::ItemPacketError::ContradictoryStackId
        ))
    ));
}

#[test]
fn semantic_world_rejections_remain_skippable_before_the_next_valid_packet() {
    let mut world_skips = 0;
    let unknown_container = decode_world_raw_with(raw_mob_equipment(1, &[0; 10], -1), 0, |raw| {
        raw.decode(&BedrockSession { shield_item_id: 0 })
    })
    .expect_err("unknown container is semantically unusable");
    let mut non_finite_move = BytesMut::new();
    wire::write_var_u64(&mut non_finite_move, 42);
    non_finite_move.put_u8(0);
    non_finite_move.put_f32_le(f32::NAN);
    non_finite_move.put_f32_le(64.0);
    non_finite_move.put_f32_le(0.0);
    non_finite_move.extend_from_slice(&[0; 3]);
    let non_finite_move = decode_world_raw_with(
        raw_packet(McpePacketName::MoveActorAbsolutePacket, &non_finite_move),
        0,
        |_| unreachable!("absolute movement uses the raw decoder"),
    )
    .expect_err("non-finite movement is semantically unusable");

    for error in [unknown_container, non_finite_move] {
        skip_semantic_world_error(error, &mut world_skips)
            .expect("well-formed semantic rejection remains survivable");
    }
    assert_eq!(world_skips, 2);

    let session = BedrockSession { shield_item_id: 0 };
    let packet: Packet = SetTimePacket { time: 7 }.into();
    let mut batch = crate::encode(&packet, &session).expect("encode valid witness");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw valid witness");
    assert!(matches!(
        decode_world_raw_with(raw, 0, |raw| raw.decode(&session)),
        Ok(Some(WorldEvent::SetTime(crate::SetTimeEvent { time: 7 })))
    ));
}
