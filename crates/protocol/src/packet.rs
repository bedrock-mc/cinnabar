//! The protocol-owned packet value; its wire layout stays private to this crate.

use std::fmt;

use bedrock_protocol_1_26_51::packets::{self, PacketId};

/// One complete Bedrock 1.26.50 (protocol 2193) game packet, including its subclient header.
///
/// The generated payload is boxed, so queues of packets move a pointer, not the largest variant.
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub(crate) header: GameHeader,
    pub(crate) data: Box<packets::Packet>,
}

/// A batch entry's header varint: the packet ID and its two-bit sender and target subclients.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GameHeader {
    pub(crate) id: PacketId,
    pub(crate) from_subclient: u32,
    pub(crate) to_subclient: u32,
}

impl GameHeader {
    /// A header for `id` with both subclients zero, as the client sends every packet.
    pub(crate) const fn new(id: PacketId) -> Self {
        Self {
            id,
            from_subclient: 0,
            to_subclient: 0,
        }
    }
}

/// Which packet a [`Packet`] is; its `Debug` form is the Bedrock packet name.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PacketKind(pub(crate) PacketId);

impl PacketKind {
    /// The packet's wire ID.
    #[must_use]
    pub const fn id(self) -> u32 {
        self.0 as u32
    }
}

impl fmt::Debug for PacketKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{:?}Packet", self.0)
    }
}

impl Packet {
    /// Wraps any generated payload with default subclients, as the codec frames it.
    pub(crate) fn new<P>(payload: P) -> Self
    where
        packets::Packet: From<P>,
    {
        let data = packets::Packet::from(payload);
        Self::from_parts(GameHeader::new(data.id()), data)
    }

    pub(crate) fn from_parts(header: GameHeader, data: packets::Packet) -> Self {
        Self {
            header,
            data: Box::new(data),
        }
    }

    /// Which packet this is, as its header names it.
    #[must_use]
    pub const fn kind(&self) -> PacketKind {
        PacketKind(self.header.id)
    }
}
