use super::*;

use bedrock_protocol_1_26_51::packets::SubChunkRequest;
use bedrock_protocol_1_26_51::types::{DimensionType, SubChunkPos, SubChunkPosOffset};
use bedrock_protocol_1_26_51::wire;

use render_api::sub_chunk_requests::{
    MAX_SUB_CHUNK_COLUMN_HEIGHT, MAX_SUB_CHUNK_REQUESTS, sub_chunk_request_offset,
};

/// Encodes ordered absolute positions around one dispatch-time center.
pub fn request_sub_chunks(
    dimension: i32,
    center: [i32; 3],
    positions: impl ExactSizeIterator<Item = [i32; 3]>,
) -> Result<Packet, WorldPacketError> {
    if positions.len() > MAX_SUB_CHUNK_REQUESTS {
        return Err(WorldPacketError::TooManySubChunkRequests {
            count: positions.len(),
            max: MAX_SUB_CHUNK_REQUESTS,
        });
    }
    let requests = positions
        .map(|position| {
            let [x, y, z] = sub_chunk_request_offset(center, position)
                .ok_or(WorldPacketError::SubChunkRequestOutOfRange { center, position })?;
            Ok(SubChunkPosOffset {
                subchunk_offset_x: wire::I8(x),
                subchunk_offset_y: wire::I8(y),
                subchunk_offset_z: wire::I8(z),
            })
        })
        .collect::<Result<Vec<_>, WorldPacketError>>()?;
    Ok(crate::Packet::new(SubChunkRequest {
        dimension_type: DimensionType(dimension),
        sub_chunk_position_offset_list: requests,
        center_pos: SubChunkPos {
            subchunk_position_x: wire::I32LE(center[0]),
            subchunk_position_y: wire::I32LE(center[1]),
            subchunk_position_z: wire::I32LE(center[2]),
        },
    }))
}

/// Builds a single column around its horizontal coordinates with a zero vertical origin.
pub fn request_sub_chunk_column(
    dimension: i32,
    chunk_x: i32,
    chunk_z: i32,
    base_sub_chunk_y: i32,
    count: usize,
) -> Result<Packet, WorldPacketError> {
    if count > MAX_SUB_CHUNK_COLUMN_HEIGHT {
        return Err(WorldPacketError::TooManySubChunkRequests {
            count,
            max: MAX_SUB_CHUNK_COLUMN_HEIGHT,
        });
    }
    for offset in 0..count {
        let offset_i32 = i32::try_from(offset).expect("request count is capped at 128");
        base_sub_chunk_y.checked_add(offset_i32).ok_or(
            WorldPacketError::SubChunkRequestYOverflow {
                base_y: base_sub_chunk_y,
                offset,
            },
        )?;
    }
    request_sub_chunks(
        dimension,
        [chunk_x, 0, chunk_z],
        (0..count).map(|offset| [chunk_x, base_sub_chunk_y + offset as i32, chunk_z]),
    )
}

pub(super) fn normalize_layer(layer: u32) -> Result<usize, WorldPacketError> {
    let normalized =
        usize::try_from(layer).map_err(|_| WorldPacketError::InvalidBlockLayer(layer))?;
    if normalized >= MAX_BLOCK_LAYERS {
        return Err(WorldPacketError::InvalidBlockLayer(layer));
    }
    Ok(normalized)
}

pub(super) fn checked_sub_chunk_position(
    origin: [i32; 3],
    offset: [i8; 3],
) -> Result<[i32; 3], WorldPacketError> {
    let mut position = [0; 3];
    for axis in 0..3 {
        position[axis] = origin[axis]
            .checked_add(i32::from(offset[axis]))
            .ok_or(WorldPacketError::SubChunkPositionOverflow { origin, offset })?;
    }
    Ok(position)
}
