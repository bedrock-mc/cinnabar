use bedrock_protocol_1_26_51::packets::PacketId;

use super::{PlaySession, reset_cache_for_immediate_boundary};
use crate::raw_packet::RawPacket;
use crate::session_transport::{Inbound, Terminal};
use crate::{Packet, ProtocolError, ServerDisconnectEvent, ServerTransferEvent};

impl<R: bridge::FrameRead> PlaySession<R> {
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
                self.transport
                    .reject_current(crate::packet_tap::PacketOutcome::Skipped);
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
            self.cache_status.lock().expect("cache status lock").reset();
        }
        Ok(retained)
    }

    /// After a write fails, reads what the core sent before it closed, decoding only the
    /// Transfer or Disconnect that ended the session, so it is retained rather than lost behind
    /// unread packets. The caller bounds the wait with [`super::WRITE_FAILURE_DRAIN`].
    pub async fn drain_to_boundary(&mut self) {
        loop {
            let ended = match self.transport.recv().await {
                Ok(Inbound::Terminal(terminal)) => {
                    self.reset_blob_cache_pending();
                    self.absorb_terminal(terminal)
                }
                Ok(Inbound::Packet(raw))
                    if matches!(raw.id, PacketId::Transfer | PacketId::Disconnect) =>
                {
                    let name = raw.id;
                    self.absorb_boundary_packet(raw, name).await
                }
                Ok(Inbound::Packet(_)) => Ok(false),
                Err(_) => return,
            };
            if ended.unwrap_or(true) {
                return;
            }
        }
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
            self.cache_status.lock().expect("cache status lock").reset();
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

    /// A Transfer the core sent behind unread packets before closing is still retained once the
    /// session drains after its write failed.
    #[cfg(unix)]
    #[tokio::test]
    async fn draining_after_a_failed_write_retains_the_transfer_behind_unread_packets() {
        use bedrock_protocol_1_26_51::packets::SetTime;
        use bedrock_protocol_1_26_51::wire;

        use crate::session_transport::SessionTransport;
        use crate::session_transport::tests::unix::write_frame;

        let dir = tempfile::tempdir().unwrap();
        let listener =
            tokio::net::UnixListener::bind(bridge::session_endpoint_path(dir.path())).unwrap();
        let (reader, frames) = bridge::connect_session(dir.path()).await.unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();
        let backlog = crate::session_transport::batch_frame(&[Packet::new(SetTime {
            time: wire::ZigZag32(1),
        })])
        .unwrap();
        for _ in 0..4 {
            write_frame(&mut peer, &backlog).await;
        }
        let mut transfer = vec![bridge::contract::KIND_TRANSFER];
        transfer.extend_from_slice(
            br#"{"address":"game.example.test","port":19133,"reload_world":false}"#,
        );
        write_frame(&mut peer, &transfer).await;
        drop(peer);
        let transport = SessionTransport::new(reader, frames, &[]);
        let mut session = PlaySession::new(transport, None, 1, false);

        session.drain_to_boundary().await;

        let transfer = session
            .take_server_transfer()
            .expect("the transfer is retained");
        assert_eq!(
            (transfer.host.as_str(), transfer.port),
            ("game.example.test", 19133)
        );
    }

    #[test]
    fn decoded_session_boundary_wakes_the_world_receiver_immediately() {
        assert!(matches!(
            boundary_wakeup::<crate::WorldEvent>(),
            Err(ProtocolError::SessionBoundary)
        ));
    }
}
