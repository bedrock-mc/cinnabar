use bedrock_protocol_1_26_51::packets::{PacketId, SetTime};
use bedrock_protocol_1_26_51::wire;
use bytes::{Buf, BufMut, Bytes};

use crate::raw_packet::{RawPacket, decode_packet_raw};

use super::{decode_world_raw_with, skip_semantic_world_error};
use crate::{Packet, ProtocolError, WorldEvent};

#[test]
fn world_clock_registry_packet_is_allowlisted_at_raw_ingress() {
    use bedrock_protocol_1_26_51::packets::SyncWorldClocks;
    use bedrock_protocol_1_26_51::types::{SyncWorldClocksData, WorldClockData};
    let packet: Packet = crate::Packet::new(SyncWorldClocks {
        data: SyncWorldClocksData::InitializeRegistryData {
            clock_data: vec![WorldClockData {
                id: wire::VarULong(31),
                name: crate::OVERWORLD_CLOCK_NAME.into(),
                time: wire::ZigZag32(18_000),
                is_paused: true,
                ..Default::default()
            }],
        },
    });
    let mut batch = crate::encode(&packet).expect("encode clock witness");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw clock witness");
    assert_eq!(
        decode_world_raw_with(raw, 0, super::decode_test_raw).expect("clock ingress"),
        Some(WorldEvent::WorldClocks(vec![
            crate::WorldClockUpdateEvent::Initialize(crate::WorldClockDefinition {
                id: 31,
                time: 18_000,
                paused: true,
            })
        ]))
    );
}

/// Builds one raw packet frame around a test-owned body.
fn raw_packet(id: PacketId, body: &[u8]) -> RawPacket {
    let mut payload = Vec::new();
    wire::Writer::new(&mut payload)
        .write_var_u32(id as u32)
        .unwrap();
    payload.put_slice(body);
    let mut frame = Vec::new();
    wire::Writer::new(&mut frame)
        .write_var_u32(payload.len() as u32)
        .unwrap();
    frame.put_slice(&payload);
    decode_packet_raw(&mut Bytes::from(frame)).expect("raw packet")
}

/// Builds complete non-air MobEquipment wire with controlled retained user data.
fn raw_mob_equipment(count: u16, extra: &[u8], container_id: i8) -> RawPacket {
    let mut body = Vec::new();
    wire::Writer::new(&mut body).write_var_u64(42).unwrap();
    body.put_i16_le(5);
    body.put_u16_le(count);
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    body.put_u8(0);
    wire::Writer::new(&mut body).write_var_u32(0).unwrap();
    wire::Writer::new(&mut body)
        .write_var_u32(extra.len() as u32)
        .unwrap();
    body.put_slice(extra);
    body.put_u8(0);
    body.put_u8(0);
    body.put_i8(container_id);
    raw_packet(PacketId::MobEquipment, &body)
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
            PacketId::InventoryContent,
            &[0x00, 0x00][..],
            "InventoryContent",
        ),
        (PacketId::InventorySlot, &[0x00][..], "InventorySlot"),
        (
            PacketId::ItemStackResponse,
            &[0x01][..],
            "ItemStackResponse",
        ),
        (PacketId::MobEquipment, &[0x2a][..], "MobEquipment"),
    ];

    for (id, body, label) in cases {
        let error = decode_world_raw_with(raw_packet(id, body), 0, super::decode_test_raw)
            .expect_err(label);
        assert!(
            matches!(
                error,
                ProtocolError::World(crate::WorldPacketError::Wire(_))
            ),
            "unexpected {label} classification: {error:?}"
        );
    }

    let mut absolute = Vec::new();
    wire::Writer::new(&mut absolute).write_var_u64(42).unwrap();
    absolute.extend_from_slice(&[0; 16]);
    let overlong = [&absolute[..], &[0xff][..]].concat();
    for body in [&absolute[..15], overlong.as_slice()] {
        let error = decode_world_raw_with(raw_packet(PacketId::MoveActorAbsolute, body), 0, |_| {
            unreachable!("absolute movement uses the raw decoder")
        })
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
        super::decode_test_raw(raw)
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
    let mut missing_stack_id = Vec::new();
    wire::Writer::new(&mut missing_stack_id)
        .write_var_u64(42)
        .unwrap();
    missing_stack_id.put_i16_le(0);
    missing_stack_id.put_u16_le(0);
    wire::Writer::new(&mut missing_stack_id)
        .write_var_u32(0)
        .unwrap();
    missing_stack_id.put_u8(1);

    let mut missing_extra = Vec::new();
    wire::Writer::new(&mut missing_extra)
        .write_var_u64(42)
        .unwrap();
    missing_extra.put_i16_le(0);
    missing_extra.put_u16_le(0);
    wire::Writer::new(&mut missing_extra)
        .write_var_u32(0)
        .unwrap();
    missing_extra.put_u8(0);
    wire::Writer::new(&mut missing_extra)
        .write_var_u32(0)
        .unwrap();
    wire::Writer::new(&mut missing_extra)
        .write_var_u32(2)
        .unwrap();

    for body in [missing_stack_id, missing_extra] {
        let error = decode_world_raw_with(raw_packet(PacketId::MobEquipment, &body), 0, |_| {
            unreachable!("empty equipment uses the raw decoder")
        })
        .expect_err("truncated declared empty-equipment field");
        assert!(matches!(
            error,
            ProtocolError::World(crate::WorldPacketError::Wire(crate::WorldWireError::Item(
                crate::ItemPacketError::MalformedWire
            )))
        ));
    }

    let mut complete_contradiction = Vec::new();
    wire::Writer::new(&mut complete_contradiction)
        .write_var_u64(42)
        .unwrap();
    complete_contradiction.put_i16_le(0);
    complete_contradiction.put_u16_le(0);
    wire::Writer::new(&mut complete_contradiction)
        .write_var_u32(0)
        .unwrap();
    complete_contradiction.put_u8(1);
    wire::Writer::new(&mut complete_contradiction)
        .write_var_u32(7)
        .unwrap();
    wire::Writer::new(&mut complete_contradiction)
        .write_var_u32(0)
        .unwrap();
    wire::Writer::new(&mut complete_contradiction)
        .write_var_u32(2)
        .unwrap();
    complete_contradiction.extend_from_slice(&[0xaa, 0xbb, 0, 0, 0]);
    let semantic = decode_world_raw_with(
        raw_packet(PacketId::MobEquipment, &complete_contradiction),
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
        super::decode_test_raw(raw)
    })
    .expect_err("unknown container is semantically unusable");
    let mut non_finite_move = Vec::new();
    wire::Writer::new(&mut non_finite_move)
        .write_var_u64(42)
        .unwrap();
    non_finite_move.put_u8(0);
    non_finite_move.put_f32_le(f32::NAN);
    non_finite_move.put_f32_le(64.0);
    non_finite_move.put_f32_le(0.0);
    non_finite_move.extend_from_slice(&[0; 3]);
    let non_finite_move = decode_world_raw_with(
        raw_packet(PacketId::MoveActorAbsolute, &non_finite_move),
        0,
        |_| unreachable!("absolute movement uses the raw decoder"),
    )
    .expect_err("non-finite movement is semantically unusable");

    for error in [unknown_container, non_finite_move] {
        skip_semantic_world_error(error, &mut world_skips)
            .expect("well-formed semantic rejection remains survivable");
    }
    assert_eq!(world_skips, 2);

    let packet: Packet = crate::Packet::new(SetTime {
        time: wire::ZigZag32(7),
    });
    let mut batch = crate::encode(&packet).expect("encode valid witness");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw valid witness");
    assert!(matches!(
        decode_world_raw_with(raw, 0, super::decode_test_raw),
        Ok(Some(WorldEvent::SetTime(crate::SetTimeEvent { time: 7 })))
    ));
}
