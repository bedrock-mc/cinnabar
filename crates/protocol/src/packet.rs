//! The protocol-owned packet value; its wire layout stays private to this crate.

use std::fmt;

use valentine::bedrock::version::v1_26_51::{
    GameHeader, McpePacket, McpePacketData, McpePacketName,
};

/// One complete Bedrock 1.26.50 (protocol 2193) game packet, including its subclient header.
#[derive(Clone, Debug, PartialEq)]
pub struct Packet {
    pub(crate) header: GameHeader,
    pub(crate) data: McpePacketData,
}

/// Which packet a [`Packet`] is; its `Debug` form is the Bedrock packet name.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PacketKind(pub(crate) McpePacketName);

impl PacketKind {
    /// The packet's wire ID.
    #[must_use]
    pub const fn id(self) -> u32 {
        self.0 as u32
    }
}

impl fmt::Debug for PacketKind {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.0, formatter)
    }
}

impl Packet {
    /// Wraps any generated payload with default subclients, as the codec frames it.
    pub(crate) fn new<P>(payload: P) -> Self
    where
        McpePacket: From<P>,
    {
        let packet = McpePacket::from(payload);
        Self::from_parts(packet.header, packet.data)
    }

    pub(crate) const fn from_parts(header: GameHeader, data: McpePacketData) -> Self {
        Self { header, data }
    }

    /// Which packet this is, as its header names it.
    #[must_use]
    pub const fn kind(&self) -> PacketKind {
        PacketKind(self.header.id)
    }

    /// The generated value the vendored session layer sends.
    pub(crate) fn into_wire(self) -> McpePacket {
        McpePacket::new(self.header, self.data)
    }
}
