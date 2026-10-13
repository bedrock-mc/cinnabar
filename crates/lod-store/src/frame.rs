//! Frames: the checksummed units appended to a region file.
//!
//! Header, 24 bytes, little-endian:
//!
//! | Bytes | Field |
//! | --- | --- |
//! | 0..4 | [`FRAME_MAGIC`] |
//! | 4 | [`FrameKind`] |
//! | 5..8 | reserved, zero |
//! | 8..16 | tag: the section key bits of a record or tombstone, the commit sequence of an index |
//! | 16..20 | payload length |
//! | 20..24 | CRC32 of bytes 0..20 and the payload |
//!
//! A record's payload is one [`lod::codec`] section record, a tombstone's is empty and an
//! index's is a [`crate::region_index`] table. Every length is bounded by its kind, so a
//! malformed header never causes a large allocation.

use lod::codec::{HEADER_LEN as RECORD_HEADER_LEN, MAX_COMPRESSED_LEN};

use crate::region_index::MAX_INDEX_LEN;

/// Leading bytes of every frame.
pub const FRAME_MAGIC: [u8; 4] = *b"CLDF";
/// Bytes before a frame's payload.
pub const FRAME_HEADER_LEN: usize = 24;
const CHECKED_LEN: usize = FRAME_HEADER_LEN - 4;
/// Largest record payload: the largest section record the codec can emit.
pub const MAX_RECORD_LEN: usize = RECORD_HEADER_LEN + MAX_COMPRESSED_LEN;

const _: () = assert!(MAX_RECORD_LEN <= u32::MAX as usize);
const _: () = assert!(MAX_INDEX_LEN <= u32::MAX as usize);

/// What a frame holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum FrameKind {
    /// A section record for the key in the tag.
    Record = 1,
    /// The key in the tag was removed; replayed by a rebuilding scan.
    Tombstone = 2,
    /// A full or delta index committed with the sequence in the tag.
    Index = 3,
}

impl FrameKind {
    /// Decodes the kind byte.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            1 => Some(Self::Record),
            2 => Some(Self::Tombstone),
            3 => Some(Self::Index),
            _ => None,
        }
    }

    /// The largest payload a frame of this kind may declare.
    #[must_use]
    pub const fn max_payload(self) -> usize {
        match self {
            Self::Record => MAX_RECORD_LEN,
            Self::Tombstone => 0,
            Self::Index => MAX_INDEX_LEN,
        }
    }
}

/// Why a frame was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    #[error("frame needs {needed} bytes but {available} remain")]
    Truncated { needed: usize, available: usize },
    #[error("frame starts with {0:02x?}, not the frame magic")]
    Magic([u8; 4]),
    #[error("frame kind {0} is unknown")]
    Kind(u8),
    #[error("frame reserved bytes are {0:02x?}, not zero")]
    Reserved([u8; 3]),
    #[error("{kind:?} frame declares a {declared}-byte payload; at most {max} are allowed")]
    Length {
        kind: FrameKind,
        declared: u32,
        max: usize,
    },
    #[error("frame checksum {stored:#010x} does not match {computed:#010x}")]
    Checksum { stored: u32, computed: u32 },
    #[error(
        "expected a {expected:?} frame tagged {expected_tag:#018x}, found {kind:?} {tag:#018x}"
    )]
    Unexpected {
        expected: FrameKind,
        expected_tag: u64,
        kind: FrameKind,
        tag: u64,
    },
}

/// The validated header fields of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameHeader {
    /// What the payload holds.
    pub kind: FrameKind,
    /// The section key bits of a record or tombstone, or an index's commit sequence.
    pub tag: u64,
    /// Payload bytes after the header, within the bound of [`Self::kind`].
    pub payload_len: u32,
    crc: u32,
}

impl FrameHeader {
    /// Validates every header field that does not need the payload.
    pub fn read(bytes: &[u8; FRAME_HEADER_LEN]) -> Result<Self, FrameError> {
        let magic: [u8; 4] = bytes[0..4].try_into().expect("four bytes");
        if magic != FRAME_MAGIC {
            return Err(FrameError::Magic(magic));
        }
        let kind = FrameKind::from_byte(bytes[4]).ok_or(FrameError::Kind(bytes[4]))?;
        let reserved: [u8; 3] = bytes[5..8].try_into().expect("three bytes");
        if reserved != [0; 3] {
            return Err(FrameError::Reserved(reserved));
        }
        let tag = u64::from_le_bytes(bytes[8..16].try_into().expect("eight bytes"));
        let payload_len = u32::from_le_bytes(bytes[16..20].try_into().expect("four bytes"));
        if payload_len as usize > kind.max_payload() {
            return Err(FrameError::Length {
                kind,
                declared: payload_len,
                max: kind.max_payload(),
            });
        }
        let crc = u32::from_le_bytes(bytes[20..24].try_into().expect("four bytes"));
        Ok(Self {
            kind,
            tag,
            payload_len,
            crc,
        })
    }

    /// Bytes of the whole frame.
    #[must_use]
    pub const fn frame_len(self) -> u64 {
        FRAME_HEADER_LEN as u64 + self.payload_len as u64
    }

    /// Checks the CRC over this header's bytes and `payload`, which must have the declared
    /// length.
    pub fn verify(self, header: &[u8; FRAME_HEADER_LEN], payload: &[u8]) -> Result<(), FrameError> {
        if payload.len() != self.payload_len as usize {
            return Err(FrameError::Truncated {
                needed: self.payload_len as usize,
                available: payload.len(),
            });
        }
        let computed = frame_crc(&header[..CHECKED_LEN], payload);
        if computed != self.crc {
            return Err(FrameError::Checksum {
                stored: self.crc,
                computed,
            });
        }
        Ok(())
    }

    /// Requires this frame to be of `kind` with `tag`.
    pub fn expect(self, kind: FrameKind, tag: u64) -> Result<(), FrameError> {
        if self.kind != kind || self.tag != tag {
            return Err(FrameError::Unexpected {
                expected: kind,
                expected_tag: tag,
                kind: self.kind,
                tag: self.tag,
            });
        }
        Ok(())
    }
}

/// Parses and verifies one whole frame at the start of `bytes`, returning its header and
/// payload.
pub fn read_frame(bytes: &[u8]) -> Result<(FrameHeader, &[u8]), FrameError> {
    let header_bytes: &[u8; FRAME_HEADER_LEN] = bytes
        .get(..FRAME_HEADER_LEN)
        .and_then(|slice| slice.try_into().ok())
        .ok_or(FrameError::Truncated {
            needed: FRAME_HEADER_LEN,
            available: bytes.len(),
        })?;
    let header = FrameHeader::read(header_bytes)?;
    let end = FRAME_HEADER_LEN + header.payload_len as usize;
    let payload = bytes
        .get(FRAME_HEADER_LEN..end)
        .ok_or(FrameError::Truncated {
            needed: end,
            available: bytes.len(),
        })?;
    header.verify(header_bytes, payload)?;
    Ok((header, payload))
}

/// Appends a frame of `kind` and `tag` holding `payload` to `out`. The payload must fit
/// the kind's bound.
pub fn append_frame(out: &mut Vec<u8>, kind: FrameKind, tag: u64, payload: &[u8]) {
    assert!(
        payload.len() <= kind.max_payload(),
        "{kind:?} payload of {} bytes exceeds its bound",
        payload.len()
    );
    let start = out.len();
    out.extend_from_slice(&FRAME_MAGIC);
    out.push(kind as u8);
    out.extend_from_slice(&[0; 3]);
    out.extend_from_slice(&tag.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    let crc = frame_crc(&out[start..start + CHECKED_LEN], payload);
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(payload);
}

/// CRC32 of a frame's checked header bytes followed by its payload.
fn frame_crc(header: &[u8], payload: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(header);
    crc.update(payload);
    crc.sum()
}

/// CRC32 of `bytes`, the checksum of the fixed region-file header and commit slots.
pub(crate) fn crc32(bytes: &[u8]) -> u32 {
    frame_crc(bytes, &[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(kind: FrameKind, tag: u64, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        append_frame(&mut out, kind, tag, payload);
        out
    }

    #[test]
    fn frames_round_trip() {
        let bytes = frame(FrameKind::Index, 7, b"payload");
        let (header, payload) = read_frame(&bytes).expect("valid frame");
        assert_eq!(
            (header.kind, header.tag, payload),
            (FrameKind::Index, 7, &b"payload"[..])
        );
        assert_eq!(header.frame_len(), bytes.len() as u64);
        header
            .expect(FrameKind::Index, 7)
            .expect("same kind and tag");
        assert!(header.expect(FrameKind::Record, 7).is_err());
    }

    #[test]
    fn malformed_headers_are_rejected_before_reading_payloads() {
        let good = frame(FrameKind::Tombstone, 1, &[]);
        let header = |edit: fn(&mut Vec<u8>)| {
            let mut bytes = good.clone();
            edit(&mut bytes);
            FrameHeader::read(bytes[..FRAME_HEADER_LEN].try_into().expect("header"))
        };
        assert!(matches!(header(|b| b[0] = b'X'), Err(FrameError::Magic(_))));
        assert_eq!(header(|b| b[4] = 9), Err(FrameError::Kind(9)));
        assert_eq!(header(|b| b[6] = 1), Err(FrameError::Reserved([0, 1, 0])));
        assert!(matches!(
            header(|b| b[16..20].copy_from_slice(&1u32.to_le_bytes())),
            Err(FrameError::Length {
                kind: FrameKind::Tombstone,
                declared: 1,
                ..
            })
        ));
        let mut huge = frame(FrameKind::Record, 1, &[1, 2, 3]);
        huge[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(read_frame(&huge), Err(FrameError::Length { .. })));
    }

    #[test]
    fn corrupt_or_short_frames_are_rejected() {
        let good = frame(FrameKind::Record, 5, &[1, 2, 3, 4]);
        for at in [8, 21, good.len() - 1] {
            let mut bad = good.clone();
            bad[at] ^= 0x40;
            assert!(
                matches!(read_frame(&bad), Err(FrameError::Checksum { .. })),
                "{at}"
            );
        }
        assert!(matches!(
            read_frame(&good[..good.len() - 1]),
            Err(FrameError::Truncated { .. })
        ));
        assert!(matches!(
            read_frame(&good[..10]),
            Err(FrameError::Truncated { .. })
        ));
    }
}
