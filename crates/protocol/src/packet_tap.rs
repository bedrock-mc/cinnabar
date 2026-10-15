//! Observation of every packet crossing the session endpoint, for diagnostics.
//!
//! The session transport reports each clientbound packet once, in arrival order and before any
//! decoding, and each serverbound packet once after it is encoded. An observer must not block:
//! it runs on the network tasks.
//!
//! Clientbound packets are reported when the transport reads their batch from the core's
//! socket, which it does only when ingress asks for a packet and has none split; the handed-off
//! startup packets are reported together as the first is asked for. An observer that times
//! them therefore measures when the client read each batch: under ingress backpressure (a full
//! world-event channel, slow decoding) that trails the batch reaching the socket, and the true
//! socket arrival is not observable without reading ahead of ingress, which would change the
//! session's flow control.

use std::fmt;

use bedrock_protocol_1_26_51::packets::PacketId;
use bytes::Bytes;

/// Which way a packet travelled relative to this client. The discriminant is its wire value
/// in header records and capture files.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PacketDirection {
    /// Server to client, as the core relayed it.
    Clientbound = 0,
    /// Client to server, as this client encoded it.
    Serverbound = 1,
}

impl PacketDirection {
    /// Every direction, in wire-value order.
    pub const ALL: [Self; 2] = [Self::Clientbound, Self::Serverbound];

    /// The direction whose wire value is `value`.
    #[must_use]
    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Clientbound),
            1 => Some(Self::Serverbound),
            _ => None,
        }
    }

    /// The direction as reports write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Clientbound => "clientbound",
            Self::Serverbound => "serverbound",
        }
    }
}

/// What ingress made of a recorded packet. The discriminant is its wire value in header records.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum PacketOutcome {
    /// Encoded for sending, or received without being rejected.
    Accepted = 0,
    /// Well-formed but semantically unusable: counted and skipped without ending the session.
    Skipped = 1,
    /// Malformed, or otherwise the packet whose processing ended the session.
    Failed = 2,
}

impl PacketOutcome {
    /// Every outcome, in wire-value order.
    pub const ALL: [Self; 3] = [Self::Accepted, Self::Skipped, Self::Failed];

    /// The outcome whose wire value is `value`.
    #[must_use]
    pub const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Accepted),
            1 => Some(Self::Skipped),
            2 => Some(Self::Failed),
            _ => None,
        }
    }

    /// The outcome as reports write it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::Skipped => "skipped",
            Self::Failed => "failed",
        }
    }
}

/// The observer's handle for a recorded packet, naming it in a later outcome report.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketRecordId(pub u64);

/// A packet body after its header varint, borrowed from the batch that carried it.
#[derive(Clone, Copy)]
pub struct PacketBody<'a> {
    batch: &'a Bytes,
    start: usize,
    end: usize,
}

impl<'a> PacketBody<'a> {
    /// The whole of `body`.
    #[must_use]
    pub fn whole(body: &'a Bytes) -> Self {
        Self {
            batch: body,
            start: 0,
            end: body.len(),
        }
    }

    /// `batch[start..end]`; panics when the range is out of bounds.
    pub(crate) fn range(batch: &'a Bytes, start: usize, end: usize) -> Self {
        assert!(
            start <= end && end <= batch.len(),
            "packet body out of bounds"
        );
        Self { batch, start, end }
    }

    /// Body length in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.end - self.start
    }

    /// Whether the body is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// The body bytes.
    #[must_use]
    pub fn as_slice(&self) -> &'a [u8] {
        &self.batch[self.start..self.end]
    }

    /// The body as a view sharing the batch allocation; never copies the bytes.
    #[must_use]
    pub fn to_shared(&self) -> Bytes {
        self.batch.slice(self.start..self.end)
    }
}

/// Receives the packet stream of one session.
pub trait PacketTap: Send + Sync {
    /// Records one packet and returns the handle a later [`PacketTap::reject`] names it by.
    /// Clientbound packets are reported once each, in arrival order, before decoding, as their
    /// batch is read; serverbound packets once each, after encoding.
    fn record(
        &self,
        direction: PacketDirection,
        packet_id: u32,
        body: PacketBody<'_>,
    ) -> PacketRecordId;

    /// Reports that ingress skipped or failed the clientbound packet `record`; `body` is the
    /// same bytes [`PacketTap::record`] saw. At most one report follows each record.
    fn reject(
        &self,
        record: PacketRecordId,
        packet_id: u32,
        outcome: PacketOutcome,
        body: PacketBody<'_>,
    );

    /// Records a clientbound batch of `len` bytes that could not be split into packets. It
    /// ends the session.
    fn reject_batch(&self, len: usize);
}

/// The packet ID header records use for a batch that could not be split into packets. No
/// Bedrock packet has this ID.
pub const UNFRAMED_BATCH_ID: u32 = 0;

/// Packets whose bodies can carry login credentials, session-key material or pack content keys.
const SECRET_PACKETS: [PacketId; 4] = [
    PacketId::Login,
    PacketId::ServerToClientHandshake,
    PacketId::ResourcePacksInfo,
    PacketId::SubClientLogin,
];

/// Whether the body of `packet_id` can carry login credentials, session-key material or pack
/// content keys. Observers may record such a packet's header, never its body.
#[must_use]
pub fn carries_secrets(packet_id: u32) -> bool {
    SECRET_PACKETS.iter().any(|id| *id as u32 == packet_id)
}

/// Displays a packet ID by its pinned-protocol name.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketName(pub u32);

impl fmt::Display for PacketName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0 == UNFRAMED_BATCH_ID {
            return formatter.write_str("UnframedBatch");
        }
        match PacketId::from_raw(self.0) {
            Some(id) => write!(formatter, "{id:?}"),
            None => write!(formatter, "Unknown({})", self.0),
        }
    }
}

/// Reports each packet of an encoded Batch frame, `[kind][length][header][body]...`, to `tap`
/// as serverbound. Reads only lengths and headers; bodies stay views of `frame`.
pub(crate) fn record_serverbound(tap: &dyn PacketTap, frame: &Bytes) {
    let mut offset = 1;
    while offset < frame.len() {
        let Some((length, length_bytes)) = read_varuint32(&frame[offset..]) else {
            return;
        };
        let start = offset + length_bytes;
        let Some(end) = start
            .checked_add(length as usize)
            .filter(|end| *end <= frame.len())
        else {
            return;
        };
        let Some((header, header_bytes)) = read_varuint32(&frame[start..end]) else {
            return;
        };
        let _ = tap.record(
            PacketDirection::Serverbound,
            header & 0x3ff,
            PacketBody::range(frame, start + header_bytes, end),
        );
        offset = end;
    }
}

/// Reads a varuint32, returning it and the bytes it used.
fn read_varuint32(bytes: &[u8]) -> Option<(u32, usize)> {
    let mut value = 0_u32;
    for (index, byte) in bytes.iter().take(5).enumerate() {
        value |= u32::from(byte & 0x7f) << (7 * index);
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
    }
    None
}

#[cfg(test)]
mod tests;
