use bridge::FrameQueue;

use super::PlaySession;
use crate::{Packet, ProtocolError};

/// The write half of a play session: frames whole batches without borrowing the session.
pub struct PlayOutbound {
    frames: FrameQueue,
    finish_loading: Vec<Packet>,
}

impl PlaySession {
    /// Detaches the write half; batches it sends share the session's own send FIFO.
    pub fn outbound(&mut self) -> Result<PlayOutbound, ProtocolError> {
        Ok(PlayOutbound {
            frames: self.transport.frame_queue(),
            finish_loading: self.take_finish_loading_packets(),
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
        let frame = crate::session_transport::batch_frame(&packets)?;
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
