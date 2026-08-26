use jolyne::{raw::RawPacket, stream::transport::Transport};
use valentine::bedrock::version::v1_26_44::McpePacketName;

use super::{PlaySession, reset_cache_for_immediate_boundary};
use crate::{Packet, ProtocolError, ServerDisconnectEvent, ServerTransferEvent};

impl<T: Transport> PlaySession<T> {
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
    /// usable terminal event. Malformed wire stays fatal; unusable transfers
    /// remain counted semantic skips and do not reset cache transactions.
    pub(super) async fn absorb_boundary_packet(
        &mut self,
        raw: RawPacket,
        name: McpePacketName,
    ) -> Result<bool, ProtocolError> {
        let packet = match self.stream.decode_raw_packet(raw) {
            Ok(packet) => packet,
            Err(error) => return Err(self.fail_session(error)),
        };
        let retained =
            self.retain_server_disconnect(&packet) || self.retain_server_transfer(&packet);
        if retained && let Some(resolver) = self.blob_cache.as_mut() {
            reset_cache_for_immediate_boundary(resolver, name)?;
        }
        Ok(retained)
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
