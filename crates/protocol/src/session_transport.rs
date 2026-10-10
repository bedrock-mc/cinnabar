use std::collections::VecDeque;
use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll, ready};

use bridge::{BridgeError, CoreMessage, FrameQueue, FramedReader};
use bytes::Bytes;
use futures::Stream;
use jolyne::stream::transport::{Transport, TransportMessage, TransportRecvMessage};

/// Returns the endpoint the client joins through, whose publication marks the core ready.
#[must_use]
pub fn bridge_endpoint_path(socket_dir: &Path) -> PathBuf {
    bridge::session_endpoint_path(socket_dir)
}

/// Returns every endpoint the core publishes in `socket_dir`, for cleanup after a lost core.
#[must_use]
pub fn core_endpoint_paths(socket_dir: &Path) -> [PathBuf; 3] {
    [
        bridge::session_endpoint_path(socket_dir),
        bridge::endpoint_path(socket_dir),
        bridge::control_endpoint_path(socket_dir),
    ]
}

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

/// Jolyne transport over a handed-off session's raw batches.
///
/// Batch frames reach Jolyne as uncompressed Bedrock batches without copying. The core's terminal
/// Transfer and Disconnect messages arrive as the server's own packets, then the stream ends, so play
/// handles them exactly as it handles a server's packet. A spawned writer owns the write half, so
/// sends never wait on a receive in progress.
pub struct SessionTransport {
    reader: FramedReader,
    frames: FrameQueue,
    pending: VecDeque<Bytes>, // Bedrock batches delivered before the next core frame
    ended: bool,              // a terminal message was delivered
    sending: VecDeque<InFlightSend>, // accepted sends, flushed strictly in order
    peer_addr: SocketAddr,
}

type FrameSend = Pin<Box<dyn Future<Output = Result<(), BridgeError>> + Send>>;

/// A send retained across cancellation until its frame is flushed.
struct InFlightSend {
    buffer: Bytes,
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

fn same_buffer(left: &Bytes, right: &Bytes) -> bool {
    left.len() == right.len() && left.as_ptr() == right.as_ptr()
}

impl SessionTransport {
    /// Continues a handed-off session; `startup` is delivered first as one Bedrock batch.
    pub(crate) fn new(reader: FramedReader, frames: FrameQueue, startup: Bytes) -> Self {
        Self {
            reader,
            frames,
            pending: VecDeque::from([startup]),
            ended: false,
            sending: VecDeque::new(),
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        }
    }

    /// The FIFO this transport's own sends share with detached outbound batches.
    pub(crate) fn frame_queue(&self) -> FrameQueue {
        self.frames.clone()
    }

    /// Maps one core frame onto what Jolyne reads: a batch, or the server packet a terminal message stands for.
    fn receive(&mut self, frame: Bytes) -> Result<TransportRecvMessage, BridgeError> {
        if let Some(body) = bridge::batch_frame_body(&frame) {
            return Ok(TransportRecvMessage::SplitFirst {
                first: crate::codec::BATCH_HEADER,
                rest: body,
            });
        }
        let packet = match bridge::decode_core_message(frame)? {
            CoreMessage::Transfer(transfer) => {
                crate::login::session_join::transfer_packet(&transfer)
            }
            CoreMessage::Disconnect(disconnect) => {
                crate::login::session_join::disconnect_packet(&disconnect)
            }
            CoreMessage::Handoff(_) | CoreMessage::PackData { .. } | CoreMessage::Batch(_) => {
                return Err(BridgeError::InvalidSessionMessage {
                    reason: "setup message after the handoff",
                });
            }
        };
        self.ended = true;
        crate::codec::encode(&packet, &crate::BedrockSession { shield_item_id: 0 })
            .map(TransportRecvMessage::Contiguous)
            .map_err(|_| BridgeError::InvalidSessionMessage {
                reason: "terminal message does not encode",
            })
    }
}

impl Transport for SessionTransport {
    type Error = BridgeError;

    const USES_BATCH_PREFIX: bool = true;

    /// Retains the frame from the first poll until it is flushed, so a cancelled send can
    /// neither lose nor repeat it; frames retained earlier finish first.
    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        message: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        if !this
            .sending
            .iter()
            .any(|sending| same_buffer(&sending.buffer, &message.buffer))
        {
            let frame = match bridge::batch_frame_from_bedrock(&message.buffer) {
                Ok(frame) => frame,
                Err(error) => return Poll::Ready(Err(error)),
            };
            let frames = this.frames.clone();
            this.sending.push_back(InFlightSend {
                buffer: message.buffer.clone(),
                write: Mutex::new(Box::pin(async move { frames.send(frame).await })),
            });
        }
        while let Some(front) = this.sending.front_mut() {
            let result = ready!(front.poll(cx));
            let finished = this
                .sending
                .pop_front()
                .expect("the polled send is retained");
            if result.is_err() {
                this.sending.clear();
                return Poll::Ready(result);
            }
            if same_buffer(&finished.buffer, &message.buffer) {
                return Poll::Ready(Ok(()));
            }
        }
        unreachable!("the current frame is retained until it finishes")
    }

    fn poll_drain_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        while let Some(front) = this.sending.front_mut() {
            let result = ready!(front.poll(cx));
            this.sending.pop_front();
            if result.is_err() {
                this.sending.clear();
                return Poll::Ready(result);
            }
        }
        Poll::Ready(Ok(()))
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
        if let Some(batch) = self.pending.pop_front() {
            return Poll::Ready(Some(Ok(TransportRecvMessage::Contiguous(batch))));
        }
        if self.ended {
            return Poll::Ready(None);
        }
        match ready!(Pin::new(&mut self.reader).poll_next(cx)) {
            Some(Ok(frame)) => Poll::Ready(Some(self.receive(frame))),
            Some(Err(error)) => Poll::Ready(Some(Err(error))),
            None => Poll::Ready(None),
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}

#[cfg(test)]
mod tests;
