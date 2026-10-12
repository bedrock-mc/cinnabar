use bedrock_protocol_1_26_51::enums::SubChunkRequestResult;
use bedrock_protocol_1_26_51::packets::{LevelChunk, SubChunk};

use super::requests::checked_sub_chunk_position;
use super::{
    LevelChunkEvent, LevelChunkMode, SubChunkBatchEvent, SubChunkDiagnostic, SubChunkEntryEvent,
    SubChunkResult, SubChunkUnavailable, WorldPacketError,
};

fn level_chunk_mode(
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

/// Normalizes an uncached LevelChunk, returning its payload separately so it stays a slice of
/// the received batch rather than a copy.
pub(crate) fn normalize_level_chunk_payload(
    packet: LevelChunk,
) -> Result<(LevelChunkEvent, bytes::Bytes), WorldPacketError> {
    if packet.cache_enabled {
        return Err(WorldPacketError::CachedChunksUnsupported);
    }
    let mode = level_chunk_mode(
        packet.client_request_sub_chunk_limit.map(|limit| limit.0),
        packet.sub_chunks_count.0,
    )?;
    Ok((
        LevelChunkEvent {
            dimension: packet.dimension_id.0,
            x: packet.chunk_position.x.0,
            z: packet.chunk_position.z.0,
            mode,
            payload: Vec::new(),
        },
        packet.serialized_chunk_data,
    ))
}

/// Normalizes an uncached LevelChunk; `CacheEnabled` is the authoritative cache gate.
pub(super) fn normalize_level_chunk(
    packet: LevelChunk,
) -> Result<LevelChunkEvent, WorldPacketError> {
    let (mut event, payload) = normalize_level_chunk_payload(packet)?;
    event.payload = payload.to_vec();
    Ok(event)
}

/// Normalizes an uncached SubChunk reply, resolving each entry's absolute position.
pub(super) fn normalize_sub_chunks(
    packet: SubChunk,
) -> Result<SubChunkBatchEvent, WorldPacketError> {
    if packet.cache_enabled {
        return Err(WorldPacketError::CachedChunksUnsupported);
    }
    let center = &packet.center_pos;
    let origin = [
        center.subchunk_position_x.0,
        center.subchunk_position_y.0,
        center.subchunk_position_z.0,
    ];
    let mut entries = Vec::with_capacity(packet.sub_chunk_data.len());
    for entry in packet.sub_chunk_data {
        let diagnostics = Some(SubChunkDiagnostic::from_entry(&entry));
        let offset = &entry.sub_chunk_pos_offset;
        let offset = [
            offset.subchunk_offset_x.0,
            offset.subchunk_offset_y.0,
            offset.subchunk_offset_z.0,
        ];
        let position = checked_sub_chunk_position(origin, offset)?;
        // Values follow vanilla's SubChunkResult: Undefined=0, Success=1, ChunkNotFound=2,
        // InvalidDimension=3, PlayerNotFound=4, IndexOutOfBounds=5, SuccessAllAir=6.
        let result = match entry.sub_chunk_request_result {
            // A Success entry without its optional payload carries no sub-chunk bytes.
            SubChunkRequestResult::Success => SubChunkResult::Success {
                payload: entry
                    .serialized_sub_chunk
                    .map(|payload| payload.to_vec())
                    .unwrap_or_default(),
            },
            SubChunkRequestResult::Successallair => SubChunkResult::AllAir,
            SubChunkRequestResult::Unknown(0) => {
                SubChunkResult::Unavailable(SubChunkUnavailable::Undefined)
            }
            SubChunkRequestResult::Levelchunkdoesntexist => {
                SubChunkResult::Unavailable(SubChunkUnavailable::ChunkNotFound)
            }
            SubChunkRequestResult::Wrongdimension => {
                SubChunkResult::Unavailable(SubChunkUnavailable::InvalidDimension)
            }
            SubChunkRequestResult::Playerdoesntexist => {
                SubChunkResult::Unavailable(SubChunkUnavailable::PlayerNotFound)
            }
            SubChunkRequestResult::Indexoutofbounds => {
                SubChunkResult::Unavailable(SubChunkUnavailable::YIndexOutOfBounds)
            }
            SubChunkRequestResult::Unknown(value) => {
                SubChunkResult::Unavailable(SubChunkUnavailable::Unknown(value))
            }
        };
        entries.push(SubChunkEntryEvent {
            position,
            result,
            diagnostics,
        });
    }
    Ok(SubChunkBatchEvent {
        dimension: packet.dimension_type.0,
        entries,
    })
}
