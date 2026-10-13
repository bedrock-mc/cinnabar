//! The fixed header and the two commit slots at the start of every region file.

use super::{
    COMMIT_SLOT_LEN, COMMIT_SLOT_OFFSETS, DATA_START, HEADER_CHECKED_LEN, REGION_FORMAT_VERSION,
    REGION_MAGIC,
};
use crate::{error::HeaderError, frame::crc32, region_id::RegionId};

/// The fixed header of an empty region: identity, checksum and two invalid commit slots.
pub(super) fn header_bytes(id: RegionId) -> [u8; DATA_START as usize] {
    let mut bytes = [0; DATA_START as usize];
    bytes[0..4].copy_from_slice(&REGION_MAGIC);
    bytes[4..6].copy_from_slice(&REGION_FORMAT_VERSION.to_le_bytes());
    bytes[6] = id.level();
    bytes[8..12].copy_from_slice(&id.x().to_le_bytes());
    bytes[12..16].copy_from_slice(&id.z().to_le_bytes());
    let crc = crc32(&bytes[..HEADER_CHECKED_LEN]);
    bytes[16..20].copy_from_slice(&crc.to_le_bytes());
    bytes
}

/// Validates the fixed header against the region the caller expects.
pub(super) fn check_header(
    bytes: &[u8; DATA_START as usize],
    id: RegionId,
) -> Result<(), HeaderError> {
    let magic: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
    if magic != REGION_MAGIC {
        return Err(HeaderError::Magic(magic));
    }
    let stored = u32::from_le_bytes(bytes[16..20].try_into().expect("four bytes"));
    let computed = crc32(&bytes[..HEADER_CHECKED_LEN]);
    if stored != computed {
        return Err(HeaderError::Checksum { stored, computed });
    }
    let version = u16::from_le_bytes(bytes[4..6].try_into().expect("two bytes"));
    if version != REGION_FORMAT_VERSION {
        return Err(HeaderError::Version(version));
    }
    let level = bytes[6];
    let x = i32::from_le_bytes(bytes[8..12].try_into().expect("four bytes"));
    let z = i32::from_le_bytes(bytes[12..16].try_into().expect("four bytes"));
    if bytes[7] != 0 || RegionId::new(level, x, z) != Some(id) {
        return Err(HeaderError::Identity {
            expected: id,
            level,
            x,
            z,
        });
    }
    Ok(())
}

/// The slot a commit with `seq` overwrites: never the one holding `seq - 1`.
pub(super) fn commit_slot_offset(seq: u64) -> u64 {
    COMMIT_SLOT_OFFSETS[(seq % 2) as usize] as u64
}

/// A commit slot naming the index frame at `offset` as commit `seq`, with its CRC32.
pub(super) fn commit_slot_bytes(seq: u64, offset: u64) -> [u8; COMMIT_SLOT_LEN] {
    let mut bytes = [0; COMMIT_SLOT_LEN];
    bytes[0..8].copy_from_slice(&seq.to_le_bytes());
    bytes[8..16].copy_from_slice(&offset.to_le_bytes());
    let crc = crc32(&bytes[..16]);
    bytes[16..20].copy_from_slice(&crc.to_le_bytes());
    bytes
}

/// The sequence and index offset of a valid commit slot.
pub(super) fn read_commit_slot(bytes: &[u8]) -> Option<(u64, u64)> {
    let stored = u32::from_le_bytes(bytes[16..20].try_into().ok()?);
    if crc32(&bytes[..16]) != stored {
        return None;
    }
    let seq = u64::from_le_bytes(bytes[0..8].try_into().ok()?);
    let offset = u64::from_le_bytes(bytes[8..16].try_into().ok()?);
    (seq > 0 && offset >= DATA_START).then_some((seq, offset))
}
