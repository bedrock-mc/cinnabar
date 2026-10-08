use valentine::bedrock::version::v1_26_51::LevelChunkPacketView;

use super::{LevelChunkEvent, LevelChunkMode, WorldPacketError};

pub(super) fn level_chunk_mode(
    request_limit: Option<i32>,
    subchunks_count: u32,
) -> Result<LevelChunkMode, WorldPacketError> {
    match request_limit {
        Some(-1) => Ok(LevelChunkMode::LimitlessRequests),
        Some(limit) => Ok(LevelChunkMode::LimitedRequests {
            highest: u16::try_from(limit)
                .map_err(|_| WorldPacketError::InvalidSubChunkCount(limit))?,
        }),
        None => {
            let count = usize::try_from(subchunks_count)
                .map_err(|_| WorldPacketError::InvalidSubChunkCount(i32::MAX))?;
            // Vanilla bounds the inline count only while decoding the payload.
            Ok(LevelChunkMode::Inline { count })
        }
    }
}

pub(crate) fn normalize_borrowed_level_chunk(
    packet: LevelChunkPacketView,
) -> Result<(LevelChunkEvent, bytes::Bytes), WorldPacketError> {
    if packet.cache_enabled {
        return Err(WorldPacketError::CachedChunksUnsupported);
    }
    let mode = level_chunk_mode(
        packet.client_request_sub_chunk_limit,
        packet.subchunks_count,
    )?;
    let payload = packet.serialized_chunk_data;
    Ok((
        LevelChunkEvent {
            dimension: packet.dimension_id.value,
            x: packet.chunk_position.x,
            z: packet.chunk_position.z,
            mode,
            payload: Vec::new(),
        },
        payload,
    ))
}
