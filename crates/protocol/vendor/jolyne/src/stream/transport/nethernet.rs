//! NetherNet (WebRTC) transport adapter.
//!
//! Wraps `tokio_nethernet::NetherNetStream` to implement the `Transport` trait.

use super::{Transport, TransportMessage, TransportRecvMessage};
use futures::{Sink, Stream};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_nethernet::{Message as NNMessage, NetherNetError, NetherNetStream};

/// NetherNet transport wrapper implementing the `Transport` trait.
///
/// This adapter converts between Jolyne's `TransportMessage` and
/// tokio-nethernet's `Message` types.
pub struct NetherNetTransport {
    stream: NetherNetStream,
    /// Placeholder address since WebRTC has no traditional peer address.
    addr: SocketAddr,
}

impl NetherNetTransport {
    /// Create a new NetherNet transport from a NetherNetStream.
    pub fn new(stream: NetherNetStream) -> Self {
        Self {
            stream,
            addr: "0.0.0.0:0".parse().unwrap(),
        }
    }

    /// Create with a custom label/addr for logging purposes.
    pub fn with_addr(stream: NetherNetStream, addr: SocketAddr) -> Self {
        Self { stream, addr }
    }

    /// Get the underlying NetherNetStream.
    pub fn into_inner(self) -> NetherNetStream {
        self.stream
    }
}

impl Transport for NetherNetTransport {
    type Error = NetherNetError;

    /// NetherNet does NOT use the 0xFE batch prefix - just compression algorithm byte directly.
    const USES_BATCH_PREFIX: bool = false;

    fn poll_send(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        msg: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        // First check if ready
        match <NetherNetStream as Sink<NNMessage>>::poll_ready(Pin::new(&mut self.stream), cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }

        // Convert and send
        let nn_msg = NNMessage::new(msg.buffer, msg.reliable);

        if let Err(e) =
            <NetherNetStream as Sink<NNMessage>>::start_send(Pin::new(&mut self.stream), nn_msg)
        {
            return Poll::Ready(Err(e));
        }

        // Flush
        <NetherNetStream as Sink<NNMessage>>::poll_flush(Pin::new(&mut self.stream), cx)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
        // NetherNetStream yields Result<Message, Error>, we need Result<Bytes, Error>
        match Pin::new(&mut self.stream).poll_next(cx) {
            Poll::Ready(Some(Ok(msg))) => {
                Poll::Ready(Some(Ok(TransportRecvMessage::Contiguous(msg.buffer))))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        self.addr
    }
}
