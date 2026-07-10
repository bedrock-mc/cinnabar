//! Transport abstraction for Bedrock protocol.
//!
//! This module provides a unified transport trait that works with both
//! RakNet (UDP) and NetherNet (WebRTC) transports.

mod inner;

#[cfg(feature = "raknet")]
pub mod raknet;

#[cfg(feature = "nethernet")]
pub mod nethernet;

pub use inner::BedrockTransport;

#[cfg(feature = "raknet")]
pub use raknet::RakNetTransport;

#[cfg(feature = "nethernet")]
pub use nethernet::NetherNetTransport;

use bytes::Bytes;
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::Poll;

/// Unified transport message for Bedrock protocol.
///
/// Used by all transport implementations to send/receive data.
#[derive(Debug, Clone)]
pub struct TransportMessage {
    /// The payload data.
    pub buffer: Bytes,
    /// Whether to use reliable ordered delivery.
    /// `true` = ReliableOrdered (required for encrypted traffic)
    /// `false` = Unreliable (for streaming data like maps)
    pub reliable: bool,
}

/// Inbound transport payload.
///
/// Some transports can surface a frame as a split first byte plus the remaining
/// payload without rebuilding a contiguous buffer.
#[derive(Debug, Clone)]
pub enum TransportRecvMessage {
    Contiguous(Bytes),
    SplitFirst { first: u8, rest: Bytes },
}

impl TransportRecvMessage {
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Contiguous(bytes) => bytes.is_empty(),
            Self::SplitFirst { .. } => false,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Contiguous(bytes) => bytes.len(),
            Self::SplitFirst { rest, .. } => 1 + rest.len(),
        }
    }

    pub fn into_bytes(self) -> Bytes {
        match self {
            Self::Contiguous(bytes) => bytes,
            Self::SplitFirst { first, rest } => {
                let mut out = bytes::BytesMut::with_capacity(1 + rest.len());
                out.extend_from_slice(&[first]);
                out.extend_from_slice(&rest);
                out.freeze()
            }
        }
    }

    pub fn copy_into(&self, out: &mut bytes::BytesMut) {
        out.clear();
        out.reserve(self.len());
        match self {
            Self::Contiguous(bytes) => out.extend_from_slice(bytes),
            Self::SplitFirst { first, rest } => {
                out.extend_from_slice(&[*first]);
                out.extend_from_slice(rest);
            }
        }
    }
}

impl TransportMessage {
    /// Create a new reliable message.
    pub fn reliable(buffer: impl Into<Bytes>) -> Self {
        Self {
            buffer: buffer.into(),
            reliable: true,
        }
    }

    /// Create a new unreliable message.
    pub fn unreliable(buffer: impl Into<Bytes>) -> Self {
        Self {
            buffer: buffer.into(),
            reliable: false,
        }
    }
}

/// Transport layer trait for Bedrock connections.
///
/// Implementations must provide:
/// - `Stream` yielding `Result<Bytes, Error>` for incoming data
/// - `Sink` accepting `TransportMessage` for outgoing data
/// - `peer_addr()` for logging/debugging
///
/// Both `RakNetTransport` and `NetherNetTransport` implement this trait.
pub trait Transport: Unpin + Send {
    /// Error type for this transport.
    type Error: std::error::Error + Send + 'static;

    /// Whether this transport uses the 0xFE batch ID prefix.
    ///
    /// - **RakNet**: `true` - Packets are wrapped as `[0xFE][compression_alg][data]`
    /// - **NetherNet**: `false` - Packets are just `[compression_alg][data]`
    ///
    /// This affects both encoding (send) and decoding (recv) of batch packets.
    const USES_BATCH_PREFIX: bool;

    /// Send a message over the transport.
    fn poll_send(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        msg: TransportMessage,
    ) -> Poll<Result<(), Self::Error>>;

    /// Receive the next message from the transport.
    fn poll_recv(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Result<TransportRecvMessage, Self::Error>>>;

    /// Returns the peer address (or placeholder for WebRTC).
    fn peer_addr(&self) -> SocketAddr;
}
