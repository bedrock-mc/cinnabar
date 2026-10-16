// Adapted from Jolyne (Axolotl Stack, https://github.com/axolotl-stack/axolotl-stack),
// Copyright (c) 2025 Jeremy Ianne, MIT License; see crates/protocol/vendor/LICENSE.

//! Packet entries with only their header parsed, so play can route by ID and decode bodies lazily.

use bedrock_protocol_1_26_51::packets::{self, PacketId};
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;

use crate::codec::SessionFailure;
use crate::packet::GameHeader;
use crate::packet_tap::PacketRecordId;
use crate::{Packet, SessionError};

/// Most packets one batch may hold.
pub(crate) const MAX_RAW_BATCH_PACKETS: usize = 1_600;

/// One batch entry, `[length][header][body]`, whose body stays a slice of the received batch.
#[derive(Debug, Clone)]
pub(crate) struct RawPacket {
    pub(crate) header: GameHeader,
    pub(crate) id: PacketId,
    body: Bytes,
    inner_frame: Bytes,
    backing_len: usize, // bytes of the batch allocation this entry shares
    /// The packet tap's handle, set when the transport splits the entry from its batch.
    pub(crate) record: Option<PacketRecordId>,
    /// The core batch frame and slot it was split from, for the latency trace; startup is frame 0.
    pub(crate) trace_position: (u64, u32),
}

impl RawPacket {
    /// Decodes the whole entry under the server limits, rejecting a body the packet does not
    /// consume.
    pub(crate) fn decode(self) -> Result<Packet, SessionError> {
        self.decode_parts()
            .map(|(header, data)| Packet::from_parts(header, data))
    }

    /// Decodes like [`Self::decode`] but returns the header and payload by value, so a caller
    /// that consumes the payload at once skips the [`Packet`] box.
    pub(crate) fn decode_parts(self) -> Result<(GameHeader, packets::Packet), SessionError> {
        match crate::codec::decode_body(self.id, &self.body) {
            Ok(data) => Ok((self.header, data)),
            Err(wire::DecodeError::TrailingBytes(remaining)) => {
                Err(SessionFailure::PacketTrailingBytes {
                    packet_id: self.id,
                    body_len: self.body.len(),
                    remaining,
                }
                .into())
            }
            Err(source) => Err(SessionFailure::PacketDecode {
                packet_id: self.id,
                body_len: self.body.len(),
                body_preview: self.body.iter().take(32).copied().collect(),
                source,
            }
            .into()),
        }
    }

    /// The payload after the header varint.
    pub(crate) fn body(&self) -> &Bytes {
        &self.body
    }

    /// The complete entry, length prefix included.
    pub(crate) fn inner_frame(&self) -> &Bytes {
        &self.inner_frame
    }

    /// Copies the entry into an allocation of its own, so a queued packet cannot keep its
    /// whole batch resident.
    pub(crate) fn into_compact(self) -> Self {
        let body_start = self.inner_frame.len() - self.body.len();
        let inner_frame = Bytes::copy_from_slice(&self.inner_frame);
        Self {
            header: self.header,
            id: self.id,
            body: inner_frame.slice(body_start..),
            backing_len: inner_frame.len(),
            inner_frame,
            record: self.record,
            trace_position: self.trace_position,
        }
    }

    /// Detaches the entry from unrelated batch siblings, so retained slices are charged only for
    /// the bytes they keep alive.
    pub(crate) fn into_retention_bounded(self) -> Self {
        if self.backing_len > self.inner_frame.len() {
            self.into_compact()
        } else {
            self
        }
    }
}

/// Splits a batch body, its length-prefixed entries without the `0xfe` marker, into raw packets
/// sharing its bytes. Bodies over `max_bytes` or holding more than [`MAX_RAW_BATCH_PACKETS`]
/// entries are refused.
pub(crate) fn decode_batch_body(
    body: Bytes,
    max_bytes: usize,
) -> Result<Vec<RawPacket>, SessionError> {
    if body.len() > max_bytes {
        return Err(SessionFailure::BatchTooLarge {
            actual: body.len(),
            max: max_bytes,
        }
        .into());
    }
    let backing_len = body.len();
    let mut offset = 0;
    let mut packets = Vec::new();
    for frame in wire::Frames::new(body.clone(), MAX_RAW_BATCH_PACKETS) {
        let frame = frame.map_err(|error| framing_failure(&body[offset..], error))?;
        offset += frame.frame().len();
        packets.push(raw_packet(frame, backing_len)?);
    }
    Ok(packets)
}

/// Decodes the first packet entry in `cursor` and advances past it.
#[cfg(test)]
pub(crate) fn decode_packet_raw(cursor: &mut Bytes) -> Result<RawPacket, SessionError> {
    let backing_len = cursor.len();
    let frame = wire::Frames::new(cursor.clone(), 1)
        .next()
        .unwrap_or(Err(wire::DecodeError::UnexpectedEof {
            needed: 1,
            remaining: 0,
        }))
        .map_err(|error| framing_failure(&cursor[..], error))?;
    let _ = cursor.split_to(frame.frame().len());
    raw_packet(frame, backing_len)
}

/// Splits a `0xfe`-marked uncompressed batch, as fixtures capture one, into raw packets.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn decode_batch(
    batch: Bytes,
    max_bytes: usize,
) -> Result<Vec<RawPacket>, crate::ProtocolError> {
    if batch.first() != Some(&wire::BATCH_HEADER) {
        return Err(crate::ProtocolError::InvalidBatchHeader {
            actual: batch.first().copied(),
        });
    }
    Ok(decode_batch_body(batch.slice(1..), max_bytes)?)
}

/// Maps a framing failure of the entry starting at `rest`: a declared length past the input is
/// a truncated entry, while a cut-short or overlong length or header varint is malformed.
fn framing_failure(rest: &[u8], error: wire::DecodeError) -> SessionError {
    if let wire::DecodeError::TooManyFrames { limit } = error {
        return SessionFailure::TooManyPackets { max: limit }.into();
    }
    let mut reader = wire::Reader::new(rest);
    match reader.read_var_u32() {
        Ok(declared) if declared as usize > reader.remaining() => SessionFailure::TruncatedEntry {
            declared: declared as usize,
            available: reader.remaining(),
        },
        _ => SessionFailure::MalformedEntry(error),
    }
    .into()
}

/// Names a framed entry's packet; an ID the protocol does not define ends the session.
fn raw_packet(frame: wire::RawFrame, backing_len: usize) -> Result<RawPacket, SessionError> {
    let header = frame.header();
    let id = PacketId::from_raw(header.id).ok_or(SessionFailure::UnknownPacketId(header.id))?;
    Ok(RawPacket {
        header: GameHeader {
            id,
            from_subclient: u32::from(header.sender_subclient),
            to_subclient: u32::from(header.target_subclient),
        },
        id,
        body: frame.body(),
        inner_frame: frame.frame().clone(),
        backing_len,
        record: None,
        trace_position: (0, 0),
    })
}

#[cfg(test)]
mod tests;
