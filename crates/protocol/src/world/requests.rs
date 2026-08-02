use super::*;

use valentine::bedrock::version::v1_26_30::{SubchunkRequestPacket, Vec3I8, Vec3Li};

/// Builds one bounded vertical-column SubChunkRequest.
pub fn request_sub_chunk_column(
    dimension: i32,
    chunk_x: i32,
    chunk_z: i32,
    base_sub_chunk_y: i32,
    count: usize,
) -> Result<Packet, WorldPacketError> {
    if count > MAX_SUB_CHUNK_REQUESTS {
        return Err(WorldPacketError::TooManySubChunkRequests {
            count,
            max: MAX_SUB_CHUNK_REQUESTS,
        });
    }
    let mut requests = Vec::with_capacity(count);
    for offset in 0..count {
        let offset_i32 = i32::try_from(offset).expect("request count is capped at 128");
        base_sub_chunk_y.checked_add(offset_i32).ok_or(
            WorldPacketError::SubChunkRequestYOverflow {
                base_y: base_sub_chunk_y,
                offset,
            },
        )?;
        requests.push(Vec3I8 {
            x: 0,
            y: offset as i8,
            z: 0,
        });
    }
    Ok(SubchunkRequestPacket {
        dimension,
        requests,
        origin: Vec3Li {
            x: chunk_x,
            y: base_sub_chunk_y,
            z: chunk_z,
        },
    }
    .into())
}

pub(super) fn normalize_layer(layer: i32) -> Result<usize, WorldPacketError> {
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
