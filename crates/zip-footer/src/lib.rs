//! ZIP end-record admission for in-memory archives, run before a ZIP parser sizes its entry table.

use thiserror::Error;

/// Length of an end record without its comment.
pub const EOCD_MIN_BYTES: usize = 22;
const EOCD_MAX_SEARCH_BYTES: usize = EOCD_MIN_BYTES + u16::MAX as usize;

/// Why an archive's end record was rejected.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum FooterError {
    #[error("ZIP footer is invalid or unsupported")]
    Invalid,
    #[error("ZIP64 is unsupported")]
    Zip64,
    #[error("ZIP entry count exceeds its limit")]
    TooManyEntries,
}

/// Strips the comment after the last valid end record so a ZIP parser cannot select a
/// signature inside it, and returns that record's entry count.
pub fn prepare_zip_bytes(bytes: &mut Vec<u8>, max_entries: usize) -> Result<usize, FooterError> {
    let (footer, entries) = find_eocd(bytes, max_entries)?;
    bytes[footer + 20..footer + 22].fill(0);
    bytes.truncate(footer + EOCD_MIN_BYTES);
    Ok(entries)
}

/// Returns the offset and entry count of the last structurally valid end record, rather than an
/// arbitrary signature in its comment.
pub fn find_eocd(bytes: &[u8], max_entries: usize) -> Result<(usize, usize), FooterError> {
    if bytes.len() < EOCD_MIN_BYTES {
        return Err(FooterError::Invalid);
    }
    let start = bytes.len().saturating_sub(EOCD_MAX_SEARCH_BYTES);
    let signature = b"PK\x05\x06";
    let mut failure = FooterError::Invalid;
    for eocd in (start..=bytes.len() - EOCD_MIN_BYTES)
        .rev()
        .filter(|offset| bytes[*offset..].starts_with(signature))
    {
        match validate_eocd_at(bytes, eocd, max_entries) {
            Ok(entries) => return Ok((eocd, entries)),
            Err(error) => failure = error,
        }
    }
    Err(failure)
}

/// Validates one end-record candidate, including its central directory and exact comment extent.
fn validate_eocd_at(bytes: &[u8], eocd: usize, max_entries: usize) -> Result<usize, FooterError> {
    let tail = &bytes[eocd..];
    let disk = le_u16(tail, 4)?;
    let central_disk = le_u16(tail, 6)?;
    let disk_entries = le_u16(tail, 8)?;
    let total_entries = le_u16(tail, 10)?;
    let central_size = le_u32(tail, 12)?;
    let central_offset = le_u32(tail, 16)?;
    let comment_len = usize::from(le_u16(tail, 20)?);
    if eocd + EOCD_MIN_BYTES + comment_len != bytes.len() || disk != 0 || central_disk != 0 {
        return Err(FooterError::Invalid);
    }
    if disk_entries == u16::MAX
        || total_entries == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX
        || eocd >= 20 && bytes[eocd - 20..eocd].starts_with(b"PK\x06\x07")
    {
        return Err(FooterError::Zip64);
    }
    if disk_entries != total_entries || usize::from(total_entries) > max_entries {
        return Err(FooterError::TooManyEntries);
    }
    let central_end = usize::try_from(central_offset)
        .ok()
        .and_then(|offset| {
            usize::try_from(central_size)
                .ok()
                .and_then(|size| offset.checked_add(size))
        })
        .ok_or(FooterError::Invalid)?;
    if central_end > eocd {
        return Err(FooterError::Invalid);
    }
    if central_end != eocd {
        let gap = &bytes[central_end..eocd];
        if gap
            .windows(4)
            .any(|window| window == b"PK\x06\x06" || window == b"PK\x06\x07")
        {
            return Err(FooterError::Zip64);
        }
        return Err(FooterError::Invalid);
    }
    Ok(usize::from(total_entries))
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, FooterError> {
    let value = bytes.get(offset..offset + 2).ok_or(FooterError::Invalid)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, FooterError> {
    let value = bytes.get(offset..offset + 4).ok_or(FooterError::Invalid)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

#[cfg(test)]
mod tests;
