use std::collections::VecDeque;
use std::future::{Future, poll_fn};
use std::path::Path;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll, ready};

use crate::codec::SessionFailure;
use crate::raw_packet::{MAX_RAW_BATCH_PACKETS, RawPacket, decode_batch_body};
use crate::session_wire::{CoreMessage, SessionDisconnect, SessionTransfer};
use crate::{Packet, ProtocolError, SessionError};
use bridge::{BridgeError, FrameQueue, FramedReader};
use bytes::{BufMut, Bytes, BytesMut};
use futures::StreamExt;

/// A server may batch a whole pack transfer, or a large item registry, into one batch,
/// so the local leg accepts batches far larger than a typical client's 16 MiB default.
/// The core already bounds pack transfers before they reach this hop.
pub(crate) const MAX_SESSION_BATCH_BYTES: usize = 256 * 1024 * 1024;

/// Best-effort: tells the core whether this client applied (or reverted) the
/// newest attempt's handed-off packs; returns whether the core recorded it.
pub async fn report_pack_application(socket_dir: &Path, applied: bool) -> bool {
    let Ok(status) = bridge::read_status(socket_dir).await else {
        return false;
    };
    bridge::report_pack_application(socket_dir, status.pack_admission.attempt_id, applied)
        .await
        .is_ok()
}

/// What the core delivers next: a server packet, or the terminal message that ends the session.
pub(crate) enum Inbound {
    Packet(RawPacket),
    Terminal(Terminal),
}

/// The core's last message on a session, standing for the server's own Transfer or Disconnect.
pub(crate) enum Terminal {
    Transfer(SessionTransfer),
    Disconnect(SessionDisconnect),
}

/// A handed-off session's packets and its send FIFO.
///
/// The handed-off startup packets come first; each later Batch frame is split in place, without
/// copying. A terminal Transfer or Disconnect ends the stream. A send is retained from its first
/// poll until flushed, strictly in order, so cancellation neither loses nor repeats it.
pub(crate) struct SessionTransport {
    reader: FramedReader,
    frames: FrameQueue,
    startup: VecDeque<Bytes>, // startup batch bodies, read before any core frame
    received: VecDeque<RawPacket>, // packets split from a batch but not yet read
    ended: bool,              // a terminal message was delivered
    sending: VecDeque<InFlightSend>,
}

type FrameSend = Pin<Box<dyn Future<Output = Result<(), BridgeError>> + Send>>;

/// A send retained across cancellation until its frame is flushed.
struct InFlightSend {
    // Only reached through `&mut`; the mutex just keeps the transport `Sync`.
    write: Mutex<FrameSend>,
}

impl InFlightSend {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BridgeError>> {
        self.write
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .poll(cx)
    }
}

impl SessionTransport {
    /// Continues a handed-off session; the `startup` packets, each with its header, come first.
    pub(crate) fn new(reader: FramedReader, frames: FrameQueue, startup: &[Bytes]) -> Self {
        Self {
            reader,
            frames,
            startup: startup_batches(startup).into(),
            received: VecDeque::new(),
            ended: false,
            sending: VecDeque::new(),
        }
    }

    /// The FIFO this transport's own sends share with detached outbound batches.
    pub(crate) fn frame_queue(&self) -> FrameQueue {
        self.frames.clone()
    }

    /// Returns the next packet or terminal message; cancelling the wait loses nothing.
    pub(crate) async fn recv(&mut self) -> Result<Inbound, SessionError> {
        loop {
            if let Some(packet) = self.received.pop_front() {
                return Ok(Inbound::Packet(packet));
            }
            if let Some(batch) = self.startup.pop_front() {
                self.received
                    .extend(decode_batch_body(batch, MAX_SESSION_BATCH_BYTES)?);
                continue;
            }
            if self.ended {
                return Err(SessionFailure::ConnectionClosed.into());
            }
            let frame = match self.reader.next().await {
                Some(Ok(frame)) => frame,
                Some(Err(error)) => return Err(SessionFailure::Transport(error).into()),
                None => return Err(SessionFailure::ConnectionClosed.into()),
            };
            if let Some(body) = crate::session_wire::batch_frame_body(&frame) {
                self.received
                    .extend(decode_batch_body(body, MAX_SESSION_BATCH_BYTES)?);
                continue;
            }
            let terminal = match crate::session_wire::decode_core_message(frame)
                .map_err(SessionFailure::Transport)?
            {
                CoreMessage::Transfer(transfer) => Terminal::Transfer(transfer),
                CoreMessage::Disconnect(disconnect) => Terminal::Disconnect(disconnect),
                CoreMessage::Handoff(_) | CoreMessage::PackData { .. } | CoreMessage::Batch(_) => {
                    return Err(
                        SessionFailure::Transport(BridgeError::InvalidSessionMessage {
                            reason: "setup message after the handoff",
                        })
                        .into(),
                    );
                }
            };
            self.ended = true;
            return Ok(Inbound::Terminal(terminal));
        }
    }

    /// Puts `packets` back ahead of every unread packet, in their order.
    pub(crate) fn prepend(&mut self, packets: Vec<RawPacket>) {
        let mut restored = VecDeque::from(packets);
        restored.append(&mut self.received);
        self.received = restored;
    }

    /// Writes `packets` as one Batch frame after every send retained before it, resolving once
    /// it is flushed. An empty list sends nothing.
    pub(crate) async fn send(&mut self, packets: &[Packet]) -> Result<(), ProtocolError> {
        if packets.is_empty() {
            return Ok(());
        }
        let Some(frame) = batch_frame(packets) else {
            return Ok(());
        };
        let frames = self.frames.clone();
        self.sending.push_back(InFlightSend {
            write: Mutex::new(Box::pin(async move { frames.send(frame).await })),
        });
        self.drain_send().await
    }

    /// Completes every send retained across cancellation, in order.
    pub(crate) async fn drain_send(&mut self) -> Result<(), ProtocolError> {
        poll_fn(|cx| self.poll_drain_send(cx))
            .await
            .map_err(|error| SessionFailure::Transport(error).into())
    }

    fn poll_drain_send(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), BridgeError>> {
        while let Some(front) = self.sending.front_mut() {
            let result = ready!(front.poll(cx));
            self.sending.pop_front();
            if result.is_err() {
                self.sending.clear();
                return Poll::Ready(result);
            }
        }
        Poll::Ready(Ok(()))
    }
}

/// One Batch frame holding `packets`, each length-prefixed as a Bedrock batch carries it. A
/// packet the codec refuses is logged and left out so the rest still go in order; `None` when
/// none encodes.
pub(crate) fn batch_frame(packets: &[Packet]) -> Option<Bytes> {
    let mut frame = vec![bridge::contract::KIND_BATCH];
    for packet in packets {
        if let Err(error) = crate::codec::encode_entry(packet, &mut frame) {
            tracing::warn!(packet = ?packet.kind(), %error, "skipping an outbound packet that failed to encode");
        }
    }
    (frame.len() > 1).then(|| Bytes::from(frame))
}

/// Groups the startup packets, which may span many upstream batches, into batch bodies that each
/// stay within the per-batch packet and byte bounds; a packet larger than the byte bound keeps a
/// body of its own, which is refused as it would be from the server.
fn startup_batches(packets: &[Bytes]) -> Vec<Bytes> {
    let mut batches = Vec::new();
    let mut batch = BytesMut::new();
    let mut count = 0;
    for packet in packets {
        let length = u32::try_from(packet.len()).expect("a frame bounds its packets");
        let entry = varuint32_len(length) + packet.len();
        if count != 0
            && (count == MAX_RAW_BATCH_PACKETS || batch.len() + entry > MAX_SESSION_BATCH_BYTES)
        {
            batches.push(std::mem::take(&mut batch).freeze());
            count = 0;
        }
        let mut length = length;
        while length >= 0x80 {
            batch.put_u8(length as u8 | 0x80);
            length >>= 7;
        }
        batch.put_u8(length as u8);
        batch.extend_from_slice(packet);
        count += 1;
    }
    if count != 0 {
        batches.push(batch.freeze());
    }
    batches
}

fn varuint32_len(value: u32) -> usize {
    (32 - value.max(1).leading_zeros() as usize).div_ceil(7)
}

#[cfg(test)]
pub(crate) mod tests;
