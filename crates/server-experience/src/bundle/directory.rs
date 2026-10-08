//! Bounded physical ZIP index; only validated local spans reach streaming decompression.

use anyhow::{Result, ensure};
use std::collections::BTreeMap;

const END_RECORD_BYTES: usize = 22;

pub(super) struct Entry<'a> {
    pub size: u64,
    pub local: &'a [u8],
}

/// Accepts one ordinary ZIP directory and checks every physical name before lossy indexing.
pub(super) fn validate(bytes: &[u8]) -> Result<BTreeMap<&str, Entry<'_>>> {
    let end = (bytes
        .len()
        .saturating_sub(END_RECORD_BYTES + u16::MAX as usize)
        ..bytes.len().saturating_sub(END_RECORD_BYTES - 1))
        .rev()
        .find(|&at| bytes.get(at..at + 4) == Some(b"PK\x05\x06") && end_record_matches(bytes, at))
        .ok_or_else(|| anyhow::anyhow!("missing ZIP directory"))?;
    ensure!(
        end + END_RECORD_BYTES + word(bytes, end + 20)? == bytes.len(),
        "invalid ZIP end"
    );
    ensure!(
        word(bytes, end + 4)? == 0 && word(bytes, end + 6)? == 0,
        "split ZIP unsupported"
    );
    let count = word(bytes, end + 10)?;
    ensure!(
        count <= crate::policy::MAX_FILES + 1,
        "too many physical archive entries"
    );
    ensure!(word(bytes, end + 8)? == count, "split ZIP directory");
    let size = dword(bytes, end + 12)?;
    let start = dword(bytes, end + 16)?;
    let mut at = start;
    ensure!(
        at.checked_add(size) == Some(end),
        "invalid ZIP directory extent"
    );
    let mut entries = BTreeMap::new();
    for _ in 0..count {
        ensure!(
            bytes.get(at..at + 4) == Some(b"PK\x01\x02"),
            "invalid ZIP entry"
        );
        let name_len = word(bytes, at + 28)?;
        let next = at + 46 + name_len + word(bytes, at + 30)? + word(bytes, at + 32)?;
        ensure!(next <= end, "truncated ZIP entry");
        let name = std::str::from_utf8(&bytes[at + 46..at + 46 + name_len])?;
        ensure!(super::safe_path(name), "unsafe archive path");
        ensure!(
            !entries.contains_key(name),
            "duplicate physical archive path"
        );
        ensure!(word(bytes, at + 34)? == 0, "split ZIP entry");
        metadata(bytes, at + 8, at + 10, word(bytes, at + 30)?)?;
        let mode = dword(bytes, at + 38)? >> 16;
        ensure!(
            mode & 0o170000 == 0 || mode & 0o170000 == 0o100000,
            "nonregular archive entry"
        );
        let offset = dword(bytes, at + 42)?;
        let local = local_entry(bytes, offset, start, at, name)?;
        entries.insert(
            name,
            Entry {
                size: dword(bytes, at + 24)? as u64,
                local,
            },
        );
        at = next;
    }
    ensure!(at == end, "unaccounted physical archive entries");
    Ok(entries)
}

/// Distinguishes a real end record from signature bytes inside its comment.
fn end_record_matches(bytes: &[u8], at: usize) -> bool {
    let fields = (|| -> Result<bool> {
        let count = word(bytes, at + 10)?;
        Ok(
            at.checked_add(END_RECORD_BYTES + word(bytes, at + 20)?) == Some(bytes.len())
                && word(bytes, at + 4)? == 0
                && word(bytes, at + 6)? == 0
                && word(bytes, at + 8)? == count
                && dword(bytes, at + 16)?.checked_add(dword(bytes, at + 12)?) == Some(at),
        )
    })();
    fields.unwrap_or(false)
}

/// Rejects features whose extra metadata or streaming sizes are outside the bundle format.
fn metadata(bytes: &[u8], flags: usize, method: usize, extra: usize) -> Result<()> {
    ensure!(
        word(bytes, flags)? & !0x0806 == 0,
        "encrypted or streaming ZIP unsupported"
    );
    ensure!(
        matches!(word(bytes, method)?, 0 | 8),
        "unsupported compression"
    );
    ensure!(extra == 0, "ZIP extra metadata unsupported");
    Ok(())
}

/// Checks a local entry against the one validated index before the streaming decoder allocates.
fn local_entry<'a>(
    bytes: &'a [u8],
    offset: usize,
    start: usize,
    central: usize,
    name: &str,
) -> Result<&'a [u8]> {
    ensure!(
        offset.checked_add(30).is_some_and(|end| end <= start),
        "invalid local entry extent"
    );
    ensure!(
        bytes.get(offset..offset + 4) == Some(b"PK\x03\x04"),
        "invalid local entry"
    );
    metadata(bytes, offset + 6, offset + 8, word(bytes, offset + 28)?)?;
    let data = offset + 30 + word(bytes, offset + 26)?;
    let end = data
        .checked_add(dword(bytes, central + 20)?)
        .ok_or_else(|| anyhow::anyhow!("local entry overflow"))?;
    ensure!(end <= start, "invalid local entry extent");
    ensure!(
        bytes.get(offset + 30..data) == Some(name.as_bytes()),
        "local name mismatch"
    );
    ensure!(
        word(bytes, offset + 6)? == word(bytes, central + 8)?
            && word(bytes, offset + 8)? == word(bytes, central + 10)?
            && dword(bytes, offset + 14)? == dword(bytes, central + 16)?
            && dword(bytes, offset + 18)? == dword(bytes, central + 20)?
            && dword(bytes, offset + 22)? == dword(bytes, central + 24)?,
        "entry size differs from signed size or local metadata"
    );
    Ok(&bytes[offset..end])
}

/// Reads a little-endian ZIP field without trusting directory offsets.
fn word(bytes: &[u8], at: usize) -> Result<usize> {
    let field = bytes
        .get(at..at + 2)
        .ok_or_else(|| anyhow::anyhow!("truncated ZIP field"))?;
    Ok(u16::from_le_bytes(field.try_into()?) as usize)
}

/// Reads a non-ZIP64 directory offset or size.
fn dword(bytes: &[u8], at: usize) -> Result<usize> {
    let field = bytes
        .get(at..at + 4)
        .ok_or_else(|| anyhow::anyhow!("truncated ZIP field"))?;
    Ok(u32::from_le_bytes(field.try_into()?) as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_a_zip_comment_can_contain_an_end_record_signature() {
        let mut bytes = b"PK\x05\x06".to_vec();
        bytes.extend([0; 16]);
        bytes.extend(22_u16.to_le_bytes());
        bytes.extend(b"PK\x05\x06");
        bytes.extend([0; 18]);
        assert!(validate(&bytes).unwrap().is_empty());
    }
}
