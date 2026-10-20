//! Server-side session messages used by offline replay and protocol fixtures.

use super::*;

/// Decodes the first client frame without requiring claims supplied only by upstream login.
pub fn decode_connect(frame: &[u8]) -> Result<ConnectRequest, BridgeError> {
    if frame.first() != Some(&KIND_CONNECT) {
        return Err(invalid("expected connect"));
    }
    let request: ConnectRequest = json(&frame[1..])?;
    if !request.client_data.is_object() {
        return Err(invalid("connect has no client data"));
    }
    Ok(request)
}

/// Encodes a core message using the same schema and packet boundaries as the client decoder.
pub fn encode_core_message(message: &CoreMessage) -> Result<Bytes, BridgeError> {
    let mut frame = BytesMut::new();
    match message {
        CoreMessage::Handoff(handoff) => {
            let metadata = serde_json::to_vec(handoff).map_err(BridgeError::SessionJson)?;
            let length =
                u32::try_from(metadata.len()).map_err(|_| invalid("handoff metadata length"))?;
            frame.put_u8(KIND_HANDOFF);
            frame.put_u32(length);
            frame.extend_from_slice(&metadata);
            let batch = encode_batch(&handoff.startup)?;
            frame.extend_from_slice(&batch[1..]);
        }
        CoreMessage::PackData { index, data } => {
            if data.is_empty() {
                return Err(invalid("pack data without bytes"));
            }
            frame.put_u8(KIND_PACK_DATA);
            frame.put_u32(*index);
            frame.extend_from_slice(data);
        }
        CoreMessage::Batch(packets) => frame.extend_from_slice(&encode_batch(packets)?),
        CoreMessage::Transfer(value) => {
            frame.put_u8(KIND_TRANSFER);
            frame.extend_from_slice(&serde_json::to_vec(value).map_err(BridgeError::SessionJson)?);
        }
        CoreMessage::Disconnect(value) => {
            frame.put_u8(KIND_DISCONNECT);
            frame.extend_from_slice(&serde_json::to_vec(value).map_err(BridgeError::SessionJson)?);
        }
    }
    if frame.len() > MAX_FRAME_LEN {
        return Err(invalid("core message exceeds frame limit"));
    }
    let frame = frame.freeze();
    // Validate startup termination and the batch boundaries before publishing a handoff.
    if matches!(message, CoreMessage::Handoff(_)) {
        decode_core_message(frame.clone())?;
    }
    Ok(frame)
}

/// The startup role of a captured packet, independent of its opaque body.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CapturedPacketKind {
    StartGame,
    Transfer,
    Login,
    Other,
}

/// Classifies the pinned protocol's packets replaced by a session handoff.
pub fn captured_packet_kind(id: u32) -> CapturedPacketKind {
    if id == PacketId::StartGame as u32 {
        CapturedPacketKind::StartGame
    } else if id == PacketId::Transfer as u32 {
        CapturedPacketKind::Transfer
    } else if [
        PacketId::NetworkSettings,
        PacketId::ServerToClientHandshake,
        PacketId::PlayStatus,
        PacketId::ResourcePacksInfo,
        PacketId::ResourcePackStack,
    ]
    .iter()
    .any(|packet| *packet as u32 == id)
    {
        CapturedPacketKind::Login
    } else {
        CapturedPacketKind::Other
    }
}

/// Rejects terrain that offline replay cannot deliver without the original client's blob store.
/// Malformed terrain is also rejected when its cache mode cannot be validated.
pub fn validate_replay_cache_mode(id: u32, body: &Bytes) -> Result<(), BridgeError> {
    use bedrock_protocol_1_26_51::packets::Packet;
    let Some(id @ (PacketId::LevelChunk | PacketId::SubChunk)) = PacketId::from_raw(id) else {
        return Ok(());
    };
    let packet = crate::codec::decode_body(id, body)
        .map_err(|_| invalid("cannot validate terrain blob-cache mode for offline replay"))?;
    let cached = match packet {
        Packet::LevelChunk(packet) => packet.cache_enabled,
        Packet::SubChunk(packet) => packet.cache_enabled,
        _ => unreachable!("the terrain ID selects a terrain payload"),
    };
    if cached {
        return Err(invalid(
            "blob-cache terrain is unsupported in offline replay",
        ));
    }
    Ok(())
}

/// Rebuilds a captured packet header with zero subclient IDs while retaining its opaque body.
pub fn packet_from_body(id: u32, body: &[u8]) -> Result<Bytes, BridgeError> {
    if id > 0x3ff {
        return Err(invalid("packet ID exceeds the Bedrock header"));
    }
    let mut wire = BytesMut::new();
    put_varuint32(&mut wire, id);
    wire.extend_from_slice(body);
    Ok(wire.freeze())
}
