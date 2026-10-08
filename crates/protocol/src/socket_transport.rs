use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll, ready};

use bridge::{BridgeError, FrameQueue, FramedReader};
use bytes::Bytes;
use futures::Stream;
use jolyne::stream::transport::{Transport, TransportMessage, TransportRecvMessage};

/// Returns the local transport endpoint for a logical socket directory.
#[must_use]
pub fn bridge_endpoint_path(socket_dir: &Path) -> std::path::PathBuf {
    bridge::endpoint_path(socket_dir)
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

/// Jolyne transport over the local length-framed bridge.
///
/// A spawned writer owns the socket's write half, so sends never wait on a receive in progress.
pub struct SocketTransport {
    reader: FramedReader,
    frames: FrameQueue,
    sending: Option<InFlightSend>,
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

impl SocketTransport {
    pub(crate) async fn connect(socket_dir: &Path) -> anyhow::Result<Self> {
        let (reader, frames) = bridge::connect(socket_dir).await?;
        Ok(Self {
            reader,
            frames,
            sending: None,
            peer_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        })
    }

    /// The FIFO this transport's own sends share with detached outbound batches.
    pub(crate) fn frame_queue(&self) -> FrameQueue {
        self.frames.clone()
    }
}

impl Transport for SocketTransport {
    type Error = BridgeError;

    const USES_BATCH_PREFIX: bool = true;

    /// Retains the frame from the first poll until it is flushed, so a cancelled send can
    /// neither lose nor repeat it; an earlier cancelled send finishes first.
    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        message: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        loop {
            let Some(sending) = this.sending.as_mut() else {
                let frames = this.frames.clone();
                let buffer = message.buffer.clone();
                this.sending = Some(InFlightSend {
                    buffer: message.buffer.clone(),
                    write: Mutex::new(Box::pin(async move { frames.send(buffer).await })),
                });
                continue;
            };
            let current = same_buffer(&sending.buffer, &message.buffer);
            let result = ready!(sending.poll(cx));
            this.sending = None;
            if current || result.is_err() {
                return Poll::Ready(result);
            }
        }
    }

    fn poll_drain_send(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let this = self.get_mut();
        let Some(sending) = this.sending.as_mut() else {
            return Poll::Ready(Ok(()));
        };
        let result = ready!(sending.poll(cx));
        this.sending = None;
        Poll::Ready(result)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
        match Pin::new(&mut self.reader).poll_next(cx) {
            Poll::Ready(Some(Ok(bytes))) => {
                Poll::Ready(Some(Ok(TransportRecvMessage::Contiguous(bytes))))
            }
            Poll::Ready(Some(Err(error))) => Poll::Ready(Some(Err(error))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        self.peer_addr
    }
}
