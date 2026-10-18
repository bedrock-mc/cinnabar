//! Shared terrain demand membership and bounds for request publication.

/// Maximum positions in one subchunk request.
pub const MAX_SUB_CHUNK_REQUESTS: usize = 8192;
/// Maximum absolute displacement admitted on each request axis.
pub const MAX_SUB_CHUNK_REQUEST_OFFSET: i32 = i8::MAX as i32;
/// Local height bound for one queued column intent.
pub const MAX_SUB_CHUNK_COLUMN_HEIGHT: usize = MAX_SUB_CHUNK_REQUEST_OFFSET as usize + 1;

/// One absolute requested slot and the demand that still owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubChunkRequestMember {
    pub position: [i32; 3],
    pub demand: u64,
}

/// Immutable membership echoed only after one request packet is written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubChunkRequestReceipt {
    pub owner: (u64, u64),
    pub id: u64,
    pub dimension: i32,
    pub members: std::sync::Arc<[SubChunkRequestMember]>,
}

/// Returns the admitted signed offsets, using widened subtraction to reject overflow.
pub fn sub_chunk_request_offset(center: [i32; 3], position: [i32; 3]) -> Option<[i8; 3]> {
    let mut offsets = [0; 3];
    for axis in 0..3 {
        let delta = i64::from(position[axis]) - i64::from(center[axis]);
        if delta.abs() > i64::from(MAX_SUB_CHUNK_REQUEST_OFFSET) {
            return None;
        }
        offsets[axis] = delta as i8;
    }
    Some(offsets)
}
