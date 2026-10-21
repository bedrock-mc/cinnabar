//! Validates unused store traffic without exposing a UI action.

use super::{PacketId, PlaySession, ProtocolError, RawPacket};

/// Identifies store packets whose payloads still require wire validation.
pub(super) fn is_unused(id: PacketId) -> bool {
    matches!(
        id,
        PacketId::ShowStoreOffer | PacketId::PurchaseReceipt | PacketId::ServerStoreInfo
    )
}

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Counts well-formed unused packets after decoding; malformed wire still ends the session.
    pub(super) fn discard_unused_packet(&mut self, raw: RawPacket) -> Result<(), ProtocolError> {
        let id = raw.id;
        if self.decode_world_candidate(raw)?.is_some() {
            self.unused_packets = self.unused_packets.saturating_add(1);
            tracing::debug!(?id, "skipping an unused packet");
        }
        Ok(())
    }

    /// Number of well-formed packets decoded and discarded because their UI is unavailable.
    pub fn unused_packet_count(&self) -> u64 {
        self.unused_packets
    }
}
