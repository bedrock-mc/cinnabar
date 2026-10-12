//! Pre-decode wire scan of inventory packets: framing errors are fatal, semantic ones deferred.

use bytes::{Buf, Bytes};
use valentine::bedrock::version::v1_26_51::McpePacketName;

use super::*;

pub(crate) fn validate_raw_inventory_packet(
    raw: &crate::raw_packet::RawPacket,
) -> Result<(), InventoryPacketError> {
    let mut body = raw.body().clone();
    let mut semantic_error = None;
    let scanned = match raw.id {
        McpePacketName::InventoryContentPacket => {
            read_var_i32(&mut body)?;
            let count = read_count(&mut body)?;
            if count > MAX_CONTAINER_SLOTS {
                defer_inventory_error(
                    &mut semantic_error,
                    InventoryPacketError::TooManySlots {
                        count,
                        max: MAX_CONTAINER_SLOTS,
                    },
                );
            }
            for _ in 0..count {
                scan_item_descriptor(&mut body, &mut semantic_error)?;
            }
            scan_full_container(&mut body)?;
            scan_item_descriptor(&mut body, &mut semantic_error)?;
            true
        }
        McpePacketName::InventorySlotPacket => {
            // The container ID is a plain byte, not a varint.
            take_u8(&mut body)?;
            read_var_i32(&mut body)?;
            if read_presence(&mut body)? {
                scan_full_container(&mut body)?;
            }
            if read_presence(&mut body)? {
                scan_item_descriptor(&mut body, &mut semantic_error)?;
            }
            scan_item_descriptor(&mut body, &mut semantic_error)?;
            true
        }
        McpePacketName::ItemStackResponsePacket => {
            scan_stack_responses(&mut body, &mut semantic_error)?;
            true
        }
        _ => false,
    };
    if scanned && body.has_remaining() {
        return Err(InventoryPacketError::MalformedWire);
    }
    if let Some(error) = semantic_error {
        return Err(error);
    }
    Ok(())
}

/// Retains the first semantic or policy error while structural scanning continues.
fn defer_inventory_error(
    semantic_error: &mut Option<InventoryPacketError>,
    error: InventoryPacketError,
) {
    if semantic_error.is_none() {
        *semantic_error = Some(error);
    }
}

fn scan_stack_responses(
    body: &mut Bytes,
    semantic_error: &mut Option<InventoryPacketError>,
) -> Result<(), InventoryPacketError> {
    let response_count = read_count(body)?;
    if response_count > MAX_STACK_RESPONSES {
        defer_inventory_error(
            semantic_error,
            InventoryPacketError::TooManyResponses {
                count: response_count,
                max: MAX_STACK_RESPONSES,
            },
        );
    }
    for _ in 0..response_count {
        take_u8(body)?;
        read_var_i32(body)?;
        if !read_presence(body)? {
            continue;
        }
        let container_count = read_count(body)?;
        if container_count > MAX_RESPONSE_CONTAINERS {
            defer_inventory_error(
                semantic_error,
                InventoryPacketError::TooManyResponseContainers {
                    count: container_count,
                    max: MAX_RESPONSE_CONTAINERS,
                },
            );
        }
        for _ in 0..container_count {
            scan_full_container(body)?;
            let slot_count = read_count(body)?;
            if slot_count > MAX_CONTAINER_SLOTS {
                defer_inventory_error(
                    semantic_error,
                    InventoryPacketError::TooManyResponseSlots {
                        count: slot_count,
                        max: MAX_CONTAINER_SLOTS,
                    },
                );
            }
            for _ in 0..slot_count {
                // requested_slot, slot, amount
                take_bytes(body, 3)?;
                if read_presence(body)? {
                    read_var_i32(body)?;
                }
                scan_response_name(body, semantic_error)?;
                if read_presence(body)? {
                    scan_response_name(body, semantic_error)?;
                }
                read_var_i32(body)?;
            }
        }
    }
    Ok(())
}

fn scan_response_name(
    body: &mut Bytes,
    semantic_error: &mut Option<InventoryPacketError>,
) -> Result<(), InventoryPacketError> {
    let length = read_count(body)?;
    if length > MAX_RESPONSE_NAME_BYTES {
        defer_inventory_error(
            semantic_error,
            InventoryPacketError::ResponseNameTooLong {
                bytes: length,
                max: MAX_RESPONSE_NAME_BYTES,
            },
        );
    }
    take_bytes(body, length)
}

/// Walks one item descriptor without materialising it.
///
/// Protocol 1001 needed a scanner per item encoding; 1.26.40 has one shape. The
/// layout is `id: i16 LE`, `stacksize: u16 LE`, `auxvalue` varint, an optional
/// net ID (presence byte then one zigzag varint -- the old model wrote two
/// varints here for its `empty`/`id` pair), `block_runtime_id` varint, and the
/// length-prefixed user-data buffer.
fn scan_item_descriptor(
    body: &mut Bytes,
    semantic_error: &mut Option<InventoryPacketError>,
) -> Result<(), InventoryPacketError> {
    take_bytes(body, 4)?;
    read_var_i32(body)?;
    if read_presence(body)? {
        read_var_i32(body)?;
    }
    read_var_i32(body)?;
    scan_item_extra(body, semantic_error)
}

fn scan_item_extra(
    body: &mut Bytes,
    semantic_error: &mut Option<InventoryPacketError>,
) -> Result<(), InventoryPacketError> {
    let bytes = read_count(body)?;
    if bytes > MAX_ITEM_EXTRA_BYTES {
        defer_inventory_error(
            semantic_error,
            InventoryPacketError::ItemExtraTooLarge {
                bytes,
                max: MAX_ITEM_EXTRA_BYTES,
            },
        );
    }
    take_bytes(body, bytes)
}

fn scan_full_container(body: &mut Bytes) -> Result<(), InventoryPacketError> {
    take_u8(body)?;
    if read_presence(body)? {
        take_bytes(body, 4)?;
    }
    Ok(())
}

fn read_presence(body: &mut Bytes) -> Result<bool, InventoryPacketError> {
    Ok(take_u8(body)? != 0)
}

fn take_u8(body: &mut Bytes) -> Result<u8, InventoryPacketError> {
    if !body.has_remaining() {
        return Err(InventoryPacketError::MalformedWire);
    }
    Ok(body.get_u8())
}

fn take_bytes(body: &mut Bytes, bytes: usize) -> Result<(), InventoryPacketError> {
    if body.remaining() < bytes {
        return Err(InventoryPacketError::MalformedWire);
    }
    body.advance(bytes);
    Ok(())
}

fn read_count(body: &mut Bytes) -> Result<usize, InventoryPacketError> {
    let value = read_var_i32(body)?;
    usize::try_from(value).map_err(|_| InventoryPacketError::MalformedWire)
}

fn read_var_i32(body: &mut Bytes) -> Result<i32, InventoryPacketError> {
    wire::read_var_u32(body)
        .map(|value| i32::from_ne_bytes(value.to_ne_bytes()))
        .map_err(|_| InventoryPacketError::MalformedWire)
}
