use std::cell::Cell;

use bytes::{BufMut, BytesMut};
use jolyne::raw::{RawPacket, decode_packet_raw};
use valentine::bedrock::version::v1_26_44::McpePacketName;
use valentine::protocol::wire;

use super::decode_world_raw_with;
use crate::{InventoryPacketError, ProtocolError, WorldPacketError};

/// Builds one raw packet frame around a test-owned inventory body.
fn raw_packet(id: McpePacketName, body: &[u8]) -> RawPacket {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, id as u32);
    payload.put_slice(body);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.put_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw inventory packet")
}

/// Appends one complete minimal item descriptor without allocating retained data.
fn append_minimal_item(body: &mut BytesMut) {
    body.put_i16_le(0);
    body.put_u16_le(0);
    wire::write_var_u32(body, 0);
    body.put_u8(0);
    wire::write_var_u32(body, 0);
    wire::write_var_u32(body, 0);
}

/// Appends one complete empty stack-response slot.
fn append_minimal_response_slot(body: &mut BytesMut) {
    body.extend_from_slice(&[0, 0, 0]);
    body.put_u8(0);
    body.put_u8(0);
    wire::write_var_u32(body, 0);
    wire::write_var_u32(body, 0);
    wire::write_var_u32(body, 0);
}

/// Builds one accepted-response prefix with a present container list.
fn response_container_prefix(container_count: usize) -> BytesMut {
    let mut body = BytesMut::new();
    wire::write_var_u32(&mut body, 1);
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(1);
    body.put_u8(1);
    wire::write_var_u32(&mut body, container_count as u32);
    body
}

#[test]
fn complete_oversized_inventory_content_remains_semantic_without_owned_decode() {
    let count = crate::MAX_CONTAINER_SLOTS + 1;
    let mut body = BytesMut::new();
    wire::write_var_u32(&mut body, 0);
    wire::write_var_u32(&mut body, count as u32);
    for _ in 0..count {
        append_minimal_item(&mut body);
    }
    body.extend_from_slice(&[0, 0]);
    append_minimal_item(&mut body);
    let decoder_called = Cell::new(false);

    let error = decode_world_raw_with(
        raw_packet(McpePacketName::InventoryContentPacket, &body),
        0,
        |_| {
            decoder_called.set(true);
            unreachable!("semantic count rejection precedes owned decode")
        },
    )
    .expect_err("complete oversized InventoryContent");

    assert!(!decoder_called.get());
    assert!(matches!(
        error,
        ProtocolError::World(WorldPacketError::Inventory(
            InventoryPacketError::TooManySlots { count: actual, .. }
        )) if actual == count
    ));
}

#[test]
fn complete_oversized_item_extra_remains_semantic_without_owned_decode() {
    let extra_len = crate::MAX_ITEM_EXTRA_BYTES + 1;
    let mut body = BytesMut::new();
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(0);
    body.put_u8(0);
    body.put_i16_le(1);
    body.put_u16_le(1);
    wire::write_var_u32(&mut body, 0);
    body.put_u8(0);
    wire::write_var_u32(&mut body, 0);
    wire::write_var_u32(&mut body, extra_len as u32);
    body.resize(body.len() + extra_len, 0);
    let decoder_called = Cell::new(false);

    let error = decode_world_raw_with(
        raw_packet(McpePacketName::InventorySlotPacket, &body),
        0,
        |_| {
            decoder_called.set(true);
            unreachable!("semantic extra-size rejection precedes owned decode")
        },
    )
    .expect_err("complete oversized item extra");

    assert!(!decoder_called.get());
    assert!(matches!(
        error,
        ProtocolError::World(WorldPacketError::Inventory(
            InventoryPacketError::ItemExtraTooLarge { bytes, .. }
        )) if bytes == extra_len
    ));
}

#[test]
fn complete_oversized_stack_response_levels_remain_semantic_without_owned_decode() {
    let response_count = crate::MAX_STACK_RESPONSES + 1;
    let mut responses = BytesMut::new();
    wire::write_var_u32(&mut responses, response_count as u32);
    for _ in 0..response_count {
        responses.extend_from_slice(&[0, 0, 0, 0]);
    }

    let container_count = crate::MAX_RESPONSE_CONTAINERS + 1;
    let mut containers = response_container_prefix(container_count);
    for _ in 0..container_count {
        containers.extend_from_slice(&[0, 0, 0]);
    }

    let slot_count = crate::MAX_CONTAINER_SLOTS + 1;
    let mut slots = response_container_prefix(1);
    slots.extend_from_slice(&[0, 0]);
    wire::write_var_u32(&mut slots, slot_count as u32);
    for _ in 0..slot_count {
        append_minimal_response_slot(&mut slots);
    }

    for (body, expected) in [
        (responses, "responses"),
        (containers, "containers"),
        (slots, "slots"),
    ] {
        let decoder_called = Cell::new(false);
        let error = decode_world_raw_with(
            raw_packet(McpePacketName::ItemStackResponsePacket, &body),
            0,
            |_| {
                decoder_called.set(true);
                unreachable!("semantic nested count rejection precedes owned decode")
            },
        )
        .expect_err(expected);
        assert!(!decoder_called.get());
        assert!(
            matches!(
                (expected, error),
                (
                    "responses",
                    ProtocolError::World(WorldPacketError::Inventory(
                        InventoryPacketError::TooManyResponses { .. }
                    ))
                ) | (
                    "containers",
                    ProtocolError::World(WorldPacketError::Inventory(
                        InventoryPacketError::TooManyResponseContainers { .. }
                    ))
                ) | (
                    "slots",
                    ProtocolError::World(WorldPacketError::Inventory(
                        InventoryPacketError::TooManyResponseSlots { .. }
                    ))
                )
            ),
            "unexpected complete {expected} classification"
        );
    }
}

#[test]
fn complete_oversized_response_names_remain_semantic_without_owned_decode() {
    for redacted in [false, true] {
        let name_len = crate::MAX_RESPONSE_NAME_BYTES + 1;
        let mut body = response_container_prefix(1);
        body.extend_from_slice(&[0, 0]);
        wire::write_var_u32(&mut body, 1);
        body.extend_from_slice(&[0, 0, 0, 0, 0]);
        if redacted {
            wire::write_var_u32(&mut body, 0);
        }
        wire::write_var_u32(&mut body, name_len as u32);
        body.resize(body.len() + name_len, b'x');
        if !redacted {
            wire::write_var_u32(&mut body, 0);
        }
        wire::write_var_u32(&mut body, 0);
        let decoder_called = Cell::new(false);

        let error = decode_world_raw_with(
            raw_packet(McpePacketName::ItemStackResponsePacket, &body),
            0,
            |_| {
                decoder_called.set(true);
                unreachable!("semantic response-name rejection precedes owned decode")
            },
        )
        .expect_err("complete oversized response name");

        assert!(!decoder_called.get());
        assert!(matches!(
            error,
            ProtocolError::World(WorldPacketError::Inventory(
                InventoryPacketError::ResponseNameTooLong { bytes, .. }
            )) if bytes == name_len
        ));
    }
}
