use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::pin::Pin;
use std::task::{Context, Poll};

use bridge::{BridgeError, FrameQueue, FramedReader};
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
    peer_addr: SocketAddr,
}

impl SocketTransport {
    pub(crate) async fn connect(socket_dir: &Path) -> anyhow::Result<Self> {
        let (reader, frames) = bridge::connect(socket_dir).await?;
        Ok(Self {
            reader,
            frames,
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

    /// Accepts the frame on the first poll, so a cancelled send can neither lose nor repeat it.
    fn poll_send(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        message: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(self.frames.send(message.buffer))
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
