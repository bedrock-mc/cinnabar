use std::sync::Arc;

use bridge::FrameQueue;

use super::PlaySession;
use crate::packet_tap::PacketTap;
use crate::{Packet, ProtocolError};

/// The write half of a play session: frames whole batches without borrowing the session.
pub struct PlayOutbound {
    frames: FrameQueue,
    finish_loading: Vec<Packet>,
    tap: Option<Arc<dyn PacketTap>>,
}

impl<R: bridge::FrameRead> PlaySession<R> {
    /// Detaches the write half; batches it sends share the session's own send FIFO and tap.
    pub fn outbound(&mut self) -> Result<PlayOutbound, ProtocolError> {
        Ok(PlayOutbound {
            frames: self.transport.frame_queue(),
            finish_loading: self.take_finish_loading_packets(),
            tap: self.transport.tap(),
        })
    }
}

impl PlayOutbound {
    /// Writes `packets` as one batch behind every frame already accepted; resolves once the
    /// batch is flushed, waiting for queue capacity first.
    pub async fn send_batch(&mut self, packets: Vec<Packet>) -> Result<(), ProtocolError> {
        if packets.is_empty() {
            return Ok(());
        }
        for packet in &packets {
            crate::codec::validate_packet(packet)?;
        }
        let Some(frame) = crate::session_transport::batch_frame(&packets) else {
            return Ok(());
        };
        if let Some(tap) = &self.tap {
            crate::packet_tap::record_serverbound(tap.as_ref(), &frame);
        }
        self.frames
            .send(frame)
            .await
            .map_err(|error| ProtocolError::Bridge(error.into()))
    }

    /// Takes the one-shot loading-end and initialization packets the session would send.
    pub fn take_finish_loading(&mut self) -> Vec<Packet> {
        std::mem::take(&mut self.finish_loading)
    }
}
