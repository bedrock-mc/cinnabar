use bedrock_protocol_1_26_51::wire;

use crate::raw_packet::RawPacket;

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
    let mut body = wire::Reader::new(raw.body());
    let actor_runtime_id = body.read_var_u64().map_err(|_| malformed())?;
    let network_id = i16::from_le_bytes(body.read_bytes().map_err(|_| malformed())?);
    if network_id != 0 {
        return Ok(None);
    }
    let count = u16::from_le_bytes(body.read_bytes().map_err(|_| malformed())?);
    let metadata = body.read_var_u32().map_err(|_| malformed())?;
    let mut contradictory_shape = count != 0 || metadata != 0;
    let has_stack_id = body.read_u8().map_err(|_| malformed())?;
    if has_stack_id != 0 {
        let _stack_id = body.read_var_u32().map_err(|_| malformed())?;
        contradictory_shape = true;
    }
    let block_runtime_id = body.read_var_u32().map_err(|_| malformed())?;
    let extra_len =
        usize::try_from(body.read_var_u32().map_err(|_| malformed())?).unwrap_or(usize::MAX);
    body.take(extra_len).map_err(|_| malformed())?;
    contradictory_shape |= block_runtime_id != 0 || extra_len != 0;
    let inventory_slot = body.read_u8().map_err(|_| malformed())?;
    let selected_slot = body.read_u8().map_err(|_| malformed())?;
    // The container ID is a plain byte in 1.26.40 rather than a named enum.
    let window = body.read_u8().map_err(|_| malformed())?;
    if !body.is_empty() {
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
