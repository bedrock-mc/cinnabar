use bytes::Buf;
use jolyne::raw::RawPacket;
use valentine::protocol::wire;

use crate::ProtocolError;

pub(super) fn decode_empty_mob_equipment(
    raw: &RawPacket,
) -> Result<Option<crate::EquipmentEvent>, ProtocolError> {
    let malformed = || {
        ProtocolError::World(crate::world::WorldPacketError::from(
            crate::ItemPacketError::MalformedWire,
        ))
    };
    let contradictory = || {
        ProtocolError::World(crate::world::WorldPacketError::Item(
            crate::ItemPacketError::ContradictoryStackId,
        ))
    };
    let mut body = raw.body().clone();
    let actor_runtime_id = wire::read_var_u64(&mut body).map_err(|_| malformed())?;
    if body.remaining() < 2 {
        return Err(malformed());
    }
    let network_id = body.get_i16_le();
    if network_id != 0 {
        return Ok(None);
    }
    if body.remaining() < 3 {
        return Err(malformed());
    }
    let count = body.get_u16_le();
    let metadata = wire::read_var_u32(&mut body).map_err(|_| malformed())?;
    let mut contradictory_shape = count != 0 || metadata != 0;
    if !body.has_remaining() {
        return Err(malformed());
    }
    let has_stack_id = body.get_u8();
    if has_stack_id != 0 {
        let _stack_id = wire::read_var_u32(&mut body).map_err(|_| malformed())?;
        contradictory_shape = true;
    }
    let block_runtime_id = wire::read_var_u32(&mut body).map_err(|_| malformed())?;
    let extra_len = usize::try_from(wire::read_var_u32(&mut body).map_err(|_| malformed())?)
        .unwrap_or(usize::MAX);
    if body.remaining() < extra_len {
        return Err(malformed());
    }
    body.advance(extra_len);
    contradictory_shape |= block_runtime_id != 0 || extra_len != 0;
    if body.remaining() < 3 {
        return Err(malformed());
    }
    let inventory_slot = body.get_u8();
    let selected_slot = body.get_u8();
    // The container ID is a plain byte in 1.26.40 rather than a named enum.
    let window = body.get_u8();
    if body.has_remaining() {
        return Err(ProtocolError::TrailingPacketBytes {
            remaining: body.remaining(),
        });
    }
    if contradictory_shape {
        return Err(contradictory());
    }
    Ok(Some(
        crate::item::normalize_empty_equipment(
            actor_runtime_id,
            inventory_slot,
            selected_slot,
            window,
        )
        .map_err(|error| ProtocolError::World(crate::world::WorldPacketError::Item(error)))?,
    ))
}
