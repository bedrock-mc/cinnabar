// Adapted from Jolyne (Axolotl Stack, https://github.com/axolotl-stack/axolotl-stack),
// Copyright (c) 2025 Jeremy Ianne, MIT License; see crates/protocol/vendor/LICENSE.

//! Packet entries with only their header parsed, so play can route by ID and decode bodies lazily.

use bytes::{Buf, Bytes};
use valentine::bedrock::codec::BedrockCodec;
use valentine::bedrock::version::v1_26_51::{
    BorrowedMcpePacket, GameHeader, McpePacketArgs, McpePacketData, McpePacketName,
};
use valentine::protocol::wire;

use crate::codec::SessionFailure;
use crate::{Packet, SessionError};

/// Most packets one batch may hold.
pub(crate) const MAX_RAW_BATCH_PACKETS: usize = 1_600;

/// One batch entry, `[length][header][body]`, whose body stays a slice of the received batch.
#[derive(Debug, Clone)]
pub(crate) struct RawPacket {
    pub(crate) header: GameHeader,
    pub(crate) id: McpePacketName,
    body: Bytes,
    inner_frame: Bytes,
    backing_len: usize, // bytes of the batch allocation this entry shares
}

impl RawPacket {
    /// Decodes the whole entry, rejecting a body the packet does not consume.
    pub(crate) fn decode(self) -> Result<Packet, SessionError> {
        let (packet_id, body_len, body_preview) = self.identity();
        let mut buf = self.inner_frame;
        let (header, data) =
            McpePacketData::decode_inner(&mut buf, McpePacketArgs).map_err(|source| {
                SessionFailure::PacketDecode {
                    packet_id,
                    body_len,
                    body_preview,
                    source,
                }
            })?;
        if buf.has_remaining() {
            return Err(SessionFailure::PacketTrailingBytes {
                packet_id,
                body_len,
                remaining: buf.remaining(),
            }
            .into());
        }
        Ok(Packet::from_parts(header, data))
    }

    /// Decodes the entry into a view sharing its bytes, with the same trailing-byte rule.
    pub(crate) fn decode_borrowed(self) -> Result<BorrowedMcpePacket, SessionError> {
        let (packet_id, body_len, body_preview) = self.identity();
        let mut buf = self.inner_frame;
        let (packet, payload_remaining) = BorrowedMcpePacket::decode_inner_with_remaining(&mut buf)
            .map_err(|source| SessionFailure::PacketDecode {
                packet_id,
                body_len,
                body_preview,
                source,
            })?;
        let remaining = buf.remaining() + payload_remaining;
        if remaining != 0 {
            return Err(SessionFailure::PacketTrailingBytes {
                packet_id,
                body_len,
                remaining,
            }
            .into());
        }
        Ok(packet)
    }

    /// The packet ID, body length and a bounded body preview that decode failures report.
    fn identity(&self) -> (McpePacketName, usize, Vec<u8>) {
        let preview = self.body.iter().take(32).copied().collect();
        (self.id, self.body.len(), preview)
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
    mut body: Bytes,
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
    let mut packets = Vec::new();
    while body.has_remaining() {
        if packets.len() == MAX_RAW_BATCH_PACKETS {
            return Err(SessionFailure::TooManyPackets {
                max: MAX_RAW_BATCH_PACKETS,
            }
            .into());
        }
        packets.push(decode_entry(&mut body, backing_len)?);
    }
    Ok(packets)
}

/// Decodes the first packet entry in `cursor` and advances past it.
#[cfg(test)]
pub(crate) fn decode_packet_raw(cursor: &mut Bytes) -> Result<RawPacket, SessionError> {
    let backing_len = cursor.len();
    decode_entry(cursor, backing_len)
}

/// Splits a `0xfe`-marked uncompressed batch, as fixtures capture one, into raw packets.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn decode_batch(
    mut batch: Bytes,
    max_bytes: usize,
) -> Result<Vec<RawPacket>, crate::ProtocolError> {
    if batch.first() != Some(&crate::codec::BATCH_HEADER) {
        return Err(crate::ProtocolError::InvalidBatchHeader {
            actual: batch.first().copied(),
        });
    }
    batch.advance(1);
    Ok(decode_batch_body(batch, max_bytes)?)
}

fn decode_entry(cursor: &mut Bytes, backing_len: usize) -> Result<RawPacket, SessionError> {
    let entry_start = cursor.clone();
    let declared = wire::read_var_u32(cursor).map_err(SessionFailure::MalformedEntry)? as usize;
    if cursor.remaining() < declared {
        return Err(SessionFailure::TruncatedEntry {
            declared,
            available: cursor.remaining(),
        }
        .into());
    }
    let prefix_len = entry_start.len() - cursor.len();
    let inner_frame = entry_start.slice(..prefix_len + declared);
    let mut payload = cursor.slice(..declared);
    cursor.advance(declared);

    let header_raw = wire::read_var_u32(&mut payload).map_err(SessionFailure::MalformedEntry)?;
    let id_raw = header_raw & 0x3ff;
    // Packet IDs are varints on the wire; the ten-bit ID fits two bytes, encoded on the stack.
    let mut id_bytes = [0_u8; 2];
    let mut id_writer = &mut id_bytes[..];
    wire::write_var_u32(&mut id_writer, id_raw);
    let id_len = 2 - id_writer.len();
    let id = McpePacketName::decode(&mut &id_bytes[..id_len], ())
        .map_err(|_| SessionFailure::UnknownPacketId(id_raw))?;
    Ok(RawPacket {
        header: GameHeader {
            id,
            from_subclient: (header_raw >> 10) & 0x3,
            to_subclient: (header_raw >> 12) & 0x3,
        },
        id,
        body: payload,
        inner_frame,
        backing_len,
    })
}

#[cfg(test)]
mod tests;
