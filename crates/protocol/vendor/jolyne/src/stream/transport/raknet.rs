//! RakNet transport adapter.
//!
//! Wraps `tokio_raknet::RaknetStream` to implement the `Transport` trait.

use super::{Transport, TransportMessage, TransportRecvMessage};
use futures::Sink;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_raknet::protocol::reliability::Reliability;
use tokio_raknet::transport::{Message as RakMessage, RaknetStream};

/// RakNet transport wrapper implementing the `Transport` trait.
///
/// This adapter converts between Jolyne's `TransportMessage` and
/// tokio-raknet's `Message` types.
pub struct RakNetTransport {
    inner: RaknetStream,
}

impl RakNetTransport {
    /// Create a new RakNet transport from a RaknetStream.
    pub fn new(stream: RaknetStream) -> Self {
        Self { inner: stream }
    }

    /// Get the underlying RaknetStream.
    pub fn into_inner(self) -> RaknetStream {
        self.inner
    }
}

impl Transport for RakNetTransport {
    type Error = tokio_raknet::RaknetError;

    /// RakNet uses the 0xFE batch prefix.
    const USES_BATCH_PREFIX: bool = true;

    fn poll_send(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        msg: TransportMessage,
    ) -> Poll<Result<(), Self::Error>> {
        // First check if ready
        match <RaknetStream as Sink<RakMessage>>::poll_ready(Pin::new(&mut self.inner), cx) {
            Poll::Ready(Ok(())) => {}
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        }

        // Convert and send
        let reliability = if msg.reliable {
            Reliability::ReliableOrdered
        } else {
            Reliability::Unreliable
        };
        let rak_msg = RakMessage::new(msg.buffer).reliability(reliability);

        if let Err(e) =
            <RaknetStream as Sink<RakMessage>>::start_send(Pin::new(&mut self.inner), rak_msg)
        {
            return Poll::Ready(Err(e));
        }

        // Flush
        <RaknetStream as Sink<RakMessage>>::poll_flush(Pin::new(&mut self.inner), cx)
    }

    fn poll_recv(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>> {
        match Pin::new(&mut self.inner).poll_recv_message(cx) {
            Poll::Ready(Some(Ok(msg))) => Poll::Ready(Some(Ok(TransportRecvMessage::SplitFirst {
                first: msg.id,
                rest: msg.payload,
            }))),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }

    fn peer_addr(&self) -> SocketAddr {
        self.inner.peer_addr()
    }
}
