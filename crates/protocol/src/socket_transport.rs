use std::collections::VecDeque;
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

impl SocketTransport {
    pub(crate) async fn connect(socket_dir: &Path) -> anyhow::Result<Self> {
        let (reader, frames) = bridge::connect(socket_dir).await?;
        Ok(Self {
            reader,
            frames,
            sending: VecDeque::new(),
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
            let frames = this.frames.clone();
            let buffer = message.buffer.clone();
            this.sending.push_back(InFlightSend {
                buffer: message.buffer.clone(),
                write: Mutex::new(Box::pin(async move { frames.send(buffer).await })),
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use futures::task::noop_waker;
    use tokio::io::AsyncReadExt;

    /// Two sends cancelled while the peer is stalled both flush on drain, once each, in order.
    #[tokio::test]
    async fn cancelled_sends_behind_a_pending_send_flush_in_order_on_drain() {
        let socket_dir =
            std::env::temp_dir().join(format!("cinnabar-socket-transport-{}", std::process::id()));
        std::fs::create_dir_all(&socket_dir).unwrap();
        let endpoint = bridge_endpoint_path(&socket_dir);
        let _ = std::fs::remove_file(&endpoint);
        let listener = tokio::net::UnixListener::bind(&endpoint).unwrap();
        let mut transport = SocketTransport::connect(&socket_dir).await.unwrap();
        let (mut peer, _) = listener.accept().await.unwrap();

        // Too large to flush while the peer is not reading.
        let first = Bytes::from(vec![1; 16 * 1024 * 1024]);
        let second = Bytes::from_static(&[2, 2]);
        let waker = noop_waker();
        let mut cx = Context::from_waker(&waker);
        for frame in [&first, &second] {
            let poll = Pin::new(&mut transport)
                .poll_send(&mut cx, TransportMessage::reliable(frame.clone()));
            assert!(poll.is_pending(), "the stalled peer must hold the send");
        }

        let mut received = Vec::new();
        let drain = async {
            std::future::poll_fn(|cx| Pin::new(&mut transport).poll_drain_send(cx))
                .await
                .unwrap();
            drop(transport);
        };
        let (_, read) = tokio::join!(drain, peer.read_to_end(&mut received));
        read.unwrap();
        let _ = std::fs::remove_file(&endpoint);
        let _ = std::fs::remove_dir(&socket_dir);

        let mut expected = Vec::new();
        for frame in [&first, &second] {
            expected.extend_from_slice(&(frame.len() as u32).to_be_bytes());
            expected.extend_from_slice(frame);
        }
        assert_eq!(received.len(), expected.len());
        assert!(received == expected, "both frames arrive once, in order");
    }
}
