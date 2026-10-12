use bedrock_protocol_1_26_51::packets::PacketId;

use super::{PlaySession, reset_cache_for_immediate_boundary};
use crate::raw_packet::RawPacket;
use crate::session_transport::Terminal;
use crate::{Packet, ProtocolError, ServerDisconnectEvent, ServerTransferEvent};

impl PlaySession {
    /// Retains a decoded disconnect and reports whether it ended the session.
    fn retain_server_disconnect(&mut self, packet: &Packet) -> bool {
        if let Some(event) = ServerDisconnectEvent::from_packet_data(&packet.data) {
            self.server_disconnect = Some(event);
            return true;
        }
        false
    }

    /// Retains a usable transfer or counts an unusable target as a semantic skip.
    fn retain_server_transfer(&mut self, packet: &Packet) -> bool {
        match ServerTransferEvent::from_packet_data(&packet.data) {
            Ok(Some(event)) => {
                self.server_transfer = Some(event);
                true
            }
            Ok(None) => false,
            Err(_) => {
                self.transfer_skips = self.transfer_skips.saturating_add(1);
                false
            }
        }
    }

    /// Decodes and retains one candidate boundary, returning whether it is a
    /// usable terminal event. Malformed wire stays fatal; a body the codec rejects
    /// with intact framing (such as an unknown Disconnect selector) and unusable
    /// transfers are counted semantic skips that do not reset cache transactions.
    pub(super) async fn absorb_boundary_packet(
        &mut self,
        raw: RawPacket,
        name: PacketId,
    ) -> Result<bool, ProtocolError> {
        let Some(packet) = self.decode_world_candidate(raw)? else {
            return Ok(false);
        };
        let retained =
            self.retain_server_disconnect(&packet) || self.retain_server_transfer(&packet);
        if retained && let Some(resolver) = self.blob_cache.as_mut() {
            reset_cache_for_immediate_boundary(resolver, name)?;
        }
        Ok(retained)
    }

    /// Retains the core's terminal message as the server packet it stands for, returning whether
    /// it ended the session; an unusable transfer target is a counted skip instead.
    pub(super) fn absorb_terminal(&mut self, terminal: Terminal) -> Result<bool, ProtocolError> {
        let name = match terminal {
            Terminal::Transfer(transfer) => {
                self.packet_id_trace.observe(PacketId::Transfer);
                match ServerTransferEvent::from_target(
                    &transfer.address,
                    transfer.port,
                    transfer.reload_world,
                ) {
                    Ok(event) => self.server_transfer = Some(event),
                    Err(_) => {
                        self.transfer_skips = self.transfer_skips.saturating_add(1);
                        return Ok(false);
                    }
                }
                PacketId::Transfer
            }
            Terminal::Disconnect(disconnect) => {
                self.packet_id_trace.observe(PacketId::Disconnect);
                self.server_disconnect = Some(ServerDisconnectEvent::from_session(&disconnect));
                PacketId::Disconnect
            }
        };
        if let Some(resolver) = self.blob_cache.as_mut() {
            reset_cache_for_immediate_boundary(resolver, name)?;
        }
        Ok(true)
    }
}

/// Wakes the play pump after a decoded server boundary has been retained.
///
/// The pump consumes the retained transfer or disconnect before it considers
/// this vendor-neutral sentinel an ordinary receive failure.
pub(super) fn boundary_wakeup<T>() -> Result<T, ProtocolError> {
    Err(ProtocolError::SessionBoundary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoded_session_boundary_wakes_the_world_receiver_immediately() {
        assert!(matches!(
            boundary_wakeup::<crate::WorldEvent>(),
            Err(ProtocolError::SessionBoundary)
        ));
    }
}
