//! Capture v2: a recording of one session's packets, which `bedrock-replay` reads.
//!
//! Every field is little-endian. The header is the magic `CNBRCAP2`, a `u16` format version,
//! the `i32` protocol version and the `u64` start time in Unix milliseconds. Each record is a
//! `u8` kind, `u32` packet ID, `u64` microseconds since the start, `u32` body length and the
//! body. Kinds 0 and 1 are packets in their [`PacketDirection`]; kind 2 is a gap whose 8-byte
//! body counts the packets dropped at that point; kind 3, with an empty body, ends a capture
//! its size limit cut short. Markers carry packet ID 0.
//!
//! A capture whose writer stopped mid-write, as when the client crashed, ends inside a record;
//! [`read_capture`] keeps the complete records before it and reports the capture cut short.

use std::io::{self, Write};

use bytes::Bytes;
use thiserror::Error;

use crate::packet_tap::PacketDirection;

/// The first bytes of every v2 capture.
pub const CAPTURE_MAGIC: [u8; 8] = *b"CNBRCAP2";
/// The format version following the magic.
pub const CAPTURE_VERSION: u16 = 2;
/// Bytes before the first record.
pub const CAPTURE_HEADER_BYTES: u64 = 8 + 2 + 4 + 8;
/// Bytes of a record before its body.
pub const RECORD_HEADER_BYTES: u64 = 1 + 4 + 8 + 4;
/// The largest capture written or read, header and final marker included.
pub const MAX_CAPTURE_BYTES: u64 = 512 << 20;

const KIND_GAP: u8 = 2;
const KIND_TRUNCATED: u8 = 3;
const GAP_BODY_BYTES: u32 = 8;

/// What a capture starts with.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CaptureHeader {
    pub protocol_version: i32,
    pub started_unix_ms: u64,
}

/// One capture record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CaptureRecord {
    /// A packet body, after its header varint.
    Packet {
        direction: PacketDirection,
        packet_id: u32,
        elapsed_us: u64,
        body: Bytes,
    },
    /// `dropped` packets were not recorded at this point.
    Gap { elapsed_us: u64, dropped: u64 },
    /// The size limit ended the capture; no record follows.
    Truncated { elapsed_us: u64 },
}

impl CaptureRecord {
    /// Encoded size in bytes.
    #[must_use]
    pub fn encoded_len(&self) -> u64 {
        RECORD_HEADER_BYTES
            + match self {
                Self::Packet { body, .. } => body.len() as u64,
                Self::Gap { .. } => u64::from(GAP_BODY_BYTES),
                Self::Truncated { .. } => 0,
            }
    }

    fn write_to(&self, out: &mut impl Write) -> io::Result<()> {
        let (kind, packet_id, elapsed_us, body): (u8, u32, u64, &[u8]) = match self {
            Self::Packet {
                direction,
                packet_id,
                elapsed_us,
                body,
            } => (*direction as u8, *packet_id, *elapsed_us, body.as_ref()),
            Self::Gap {
                elapsed_us,
                dropped,
            } => {
                return write_record_header(out, KIND_GAP, 0, *elapsed_us, GAP_BODY_BYTES)
                    .and_then(|()| out.write_all(&dropped.to_le_bytes()));
            }
            Self::Truncated { elapsed_us } => (KIND_TRUNCATED, 0, *elapsed_us, &[][..]),
        };
        let length = u32::try_from(body.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "packet body over 4 GiB"))?;
        write_record_header(out, kind, packet_id, elapsed_us, length)?;
        out.write_all(body)
    }
}

fn write_record_header(
    out: &mut impl Write,
    kind: u8,
    packet_id: u32,
    elapsed_us: u64,
    length: u32,
) -> io::Result<()> {
    let mut header = [0_u8; RECORD_HEADER_BYTES as usize];
    header[0] = kind;
    header[1..5].copy_from_slice(&packet_id.to_le_bytes());
    header[5..13].copy_from_slice(&elapsed_us.to_le_bytes());
    header[13..17].copy_from_slice(&length.to_le_bytes());
    out.write_all(&header)
}

/// Writes a capture that never exceeds its size limit: the record that would leave no room
/// for the final marker is replaced by that marker, and nothing is written after it.
pub struct CaptureWriter<W: Write> {
    out: W,
    written: u64,
    limit: u64,
    ended: bool,
}

impl<W: Write> CaptureWriter<W> {
    /// Writes the header of a capture limited to [`MAX_CAPTURE_BYTES`].
    pub fn new(out: W, header: CaptureHeader) -> io::Result<Self> {
        Self::with_limit(out, header, MAX_CAPTURE_BYTES)
    }

    /// Writes the header of a capture limited to `limit` bytes, which must hold the header and
    /// the final marker.
    pub fn with_limit(mut out: W, header: CaptureHeader, limit: u64) -> io::Result<Self> {
        if limit < CAPTURE_HEADER_BYTES + RECORD_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "capture limit cannot hold the header and final marker",
            ));
        }
        out.write_all(&CAPTURE_MAGIC)?;
        out.write_all(&CAPTURE_VERSION.to_le_bytes())?;
        out.write_all(&header.protocol_version.to_le_bytes())?;
        out.write_all(&header.started_unix_ms.to_le_bytes())?;
        Ok(Self {
            out,
            written: CAPTURE_HEADER_BYTES,
            limit,
            ended: false,
        })
    }

    /// Appends `record`, returning `false` once the limit has ended the capture: the record
    /// that reaches it is replaced by the final marker, and later records are not written. A
    /// gap of zero packets is refused.
    pub fn write(&mut self, record: &CaptureRecord) -> io::Result<bool> {
        if matches!(record, CaptureRecord::Gap { dropped: 0, .. }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a gap record counts at least one dropped packet",
            ));
        }
        if self.ended {
            return Ok(false);
        }
        let length = record.encoded_len();
        if self.written + length + RECORD_HEADER_BYTES > self.limit {
            let elapsed_us = match record {
                CaptureRecord::Packet { elapsed_us, .. }
                | CaptureRecord::Gap { elapsed_us, .. }
                | CaptureRecord::Truncated { elapsed_us } => *elapsed_us,
            };
            self.ended = true;
            CaptureRecord::Truncated { elapsed_us }.write_to(&mut self.out)?;
            self.written += RECORD_HEADER_BYTES;
            return Ok(false);
        }
        record.write_to(&mut self.out)?;
        self.written += length;
        Ok(true)
    }

    /// Bytes written so far, header included.
    #[must_use]
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Flushes the underlying writer.
    pub fn flush(&mut self) -> io::Result<()> {
        self.out.flush()
    }

    /// Returns the underlying writer.
    pub fn into_inner(self) -> W {
        self.out
    }
}

/// Why a capture could not be read.
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CaptureError {
    #[error("capture does not start with the v2 magic")]
    Magic,
    #[error("capture header is truncated")]
    TruncatedHeader,
    #[error("capture of {0} bytes exceeds the capture size limit")]
    TooLarge(u64),
    #[error("unsupported capture format version {0}")]
    Version(u16),
    #[error("record {record}: unknown kind {kind}")]
    Kind { record: usize, kind: u8 },
    #[error("record {record}: malformed {kind} marker")]
    Marker { record: usize, kind: &'static str },
    #[error("record {record}: follows the final marker")]
    AfterEnd { record: usize },
}

/// A v2 capture as read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaptureFile {
    pub header: CaptureHeader,
    pub records: Vec<CaptureRecord>,
    /// The file ends inside a record, which was left out: its writer stopped mid-write.
    pub cut_short: bool,
}

/// Whether `data` starts like a v2 capture.
#[must_use]
pub fn is_capture_v2(data: &[u8]) -> bool {
    data.starts_with(&CAPTURE_MAGIC)
}

/// Reads a whole v2 capture of at most [`MAX_CAPTURE_BYTES`]; packet bodies are views of
/// `data`. A final record the file ends inside is dropped and reported as
/// [`CaptureFile::cut_short`]; any other malformed record rejects the capture.
pub fn read_capture(data: Bytes) -> Result<CaptureFile, CaptureError> {
    read_capture_within(data, MAX_CAPTURE_BYTES)
}

fn read_capture_within(data: Bytes, max_bytes: u64) -> Result<CaptureFile, CaptureError> {
    if !is_capture_v2(&data) {
        return Err(CaptureError::Magic);
    }
    if data.len() as u64 > max_bytes {
        return Err(CaptureError::TooLarge(data.len() as u64));
    }
    if (data.len() as u64) < CAPTURE_HEADER_BYTES {
        return Err(CaptureError::TruncatedHeader);
    }
    let version = u16::from_le_bytes(data[8..10].try_into().expect("two bytes"));
    if version != CAPTURE_VERSION {
        return Err(CaptureError::Version(version));
    }
    let header = CaptureHeader {
        protocol_version: i32::from_le_bytes(data[10..14].try_into().expect("four bytes")),
        started_unix_ms: u64::from_le_bytes(data[14..22].try_into().expect("eight bytes")),
    };
    let mut records = Vec::new();
    let mut offset = CAPTURE_HEADER_BYTES as usize;
    let mut ended = false;
    let mut cut_short = false;
    while offset < data.len() {
        let record = records.len();
        if ended {
            return Err(CaptureError::AfterEnd { record });
        }
        let rest = &data[offset..];
        if (rest.len() as u64) < RECORD_HEADER_BYTES {
            cut_short = true;
            break;
        }
        let kind = rest[0];
        let packet_id = u32::from_le_bytes(rest[1..5].try_into().expect("four bytes"));
        let elapsed_us = u64::from_le_bytes(rest[5..13].try_into().expect("eight bytes"));
        let length = u32::from_le_bytes(rest[13..17].try_into().expect("four bytes")) as usize;
        let start = offset + RECORD_HEADER_BYTES as usize;
        let Some(end) = start.checked_add(length).filter(|end| *end <= data.len()) else {
            cut_short = true;
            break;
        };
        let marker = |name| CaptureError::Marker { record, kind: name };
        records.push(match (PacketDirection::from_wire(kind), kind) {
            (Some(direction), _) => CaptureRecord::Packet {
                direction,
                packet_id,
                elapsed_us,
                body: data.slice(start..end),
            },
            (None, KIND_GAP) => {
                if packet_id != 0 || length != GAP_BODY_BYTES as usize {
                    return Err(marker("gap"));
                }
                let dropped = u64::from_le_bytes(data[start..end].try_into().expect("eight bytes"));
                if dropped == 0 {
                    return Err(marker("gap"));
                }
                CaptureRecord::Gap {
                    elapsed_us,
                    dropped,
                }
            }
            (None, KIND_TRUNCATED) => {
                if packet_id != 0 || length != 0 {
                    return Err(marker("final"));
                }
                ended = true;
                CaptureRecord::Truncated { elapsed_us }
            }
            (None, kind) => return Err(CaptureError::Kind { record, kind }),
        });
        offset = end;
    }
    Ok(CaptureFile {
        header,
        records,
        cut_short,
    })
}

#[cfg(test)]
mod tests;
