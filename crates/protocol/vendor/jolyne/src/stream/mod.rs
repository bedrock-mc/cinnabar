//! Bedrock protocol stream types.
//!
//! Provides strongly-typed, state-aware Bedrock connections using the Typestate pattern.

use std::marker::PhantomData;
use std::sync::Arc;

use crate::config::BedrockListenerConfig;
use crate::error::JolyneError;
use crate::valentine::{McpePacket, McpePacketArgs};
use transport::{BedrockTransport, Transport};

pub mod transport;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "server")]
pub mod server;

// Re-export transport types
#[cfg(feature = "raknet")]
pub use transport::RakNetTransport;

#[cfg(feature = "nethernet")]
pub use transport::NetherNetTransport;

/// A strongly-typed, state-aware Bedrock protocol stream.
///
/// This struct enforces protocol correctness at compile time using the Typestate pattern.
/// - `S` represents the current protocol state (e.g., `Login`, `Play`)
/// - `R` represents the connection role (`Client` or `Server`)
/// - `T` represents the underlying transport (`RakNetTransport` or `NetherNetTransport`)
pub struct BedrockStream<S: State, R: Role, T: Transport> {
    pub(crate) transport: BedrockTransport<T>,
    pub(crate) state: S,
    pub(crate) _role: PhantomData<R>,
}

impl<S: State, R: Role, T: Transport> BedrockStream<S, R, T> {
    pub fn peer_addr(&self) -> std::net::SocketAddr {
        self.transport.peer_addr()
    }

    /// Consumes the stream and returns the underlying transport.
    /// This allows bypassing the state machine for proxying or raw access.
    pub fn into_transport(self) -> BedrockTransport<T> {
        self.transport
    }

    /// Returns a reference to the current state.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Configures the flushing strategy.
    ///
    /// - `true` (Default): `send()` sends packets immediately (low latency, high overhead).
    /// - `false`: `send()` queues packets. You MUST call `flush()` to send them (high throughput).
    pub fn set_auto_flush(&mut self, auto: bool) {
        self.transport.set_auto_flush(auto);
    }

    /// Returns the current packet args derived from transport session state.
    pub fn packet_args(&self) -> McpePacketArgs {
        self.transport.packet_args()
    }

    /// Flushes all buffered packets as a single batch (ReliableOrdered).
    /// Does nothing if the buffer is empty.
    pub async fn flush(&mut self) -> Result<(), JolyneError> {
        self.transport.flush().await
    }

    /// Sends a list of packets as a single batch with specified reliability.
    ///
    /// This bypasses the internal `write_buffer` and sends immediately.
    /// Useful for streaming data (e.g. video/maps) that should use `Unreliable`.
    pub async fn send_batch_with_reliability(
        &mut self,
        packets: &[McpePacket],
        reliable: bool,
    ) -> Result<(), JolyneError> {
        self.transport
            .send_batch_with_reliability(packets, reliable)
            .await
    }
}

/// Marker trait for protocol states.
pub trait State {}

// --- Granular Handshake States ---

/// Initial state: Connected, waiting for RequestNetworkSettings.
pub struct Handshake {
    pub config: Option<Arc<BedrockListenerConfig>>,
}
impl State for Handshake {}

/// State: Network settings agreed, waiting for Login packet.
pub struct Login {
    pub config: Option<Arc<BedrockListenerConfig>>,
}
impl State for Login {}

/// State: Authenticated, negotiating encryption (ServerToClient/ClientToServer).
pub struct SecurePending {
    pub config: Option<Arc<BedrockListenerConfig>>,
}
impl State for SecurePending {}

/// State: Negotiating resource packs (ResourcePacksInfo, ResourcePackStack).
pub struct ResourcePacks {
    /// Some servers (like LBSG) send ResourcePacksInfo before PlayStatus.
    /// If we received it early during handshake, it's stored here.
    pub early_packet: Option<McpePacket>,
}
impl State for ResourcePacks {}

/// State: Connection is initialized, waiting for StartGame packet/processing.
pub struct StartGame;
impl State for StartGame {}

// --- Main Game State ---

/// Final State: Fully authenticated, in-game. Ready to exchange game packets.
pub struct Play;
impl State for Play {}

// --- Roles ---

/// Marker trait for connection roles.
pub trait Role {}

/// Marker for a Server connection.
pub struct Server;
impl Role for Server {}

/// Marker for a Client connection.
pub struct Client;
impl Role for Client {}

// --- User-Friendly Type Aliases ---

// RakNet type aliases (default transport)
#[cfg(feature = "raknet")]
pub mod raknet_types {
    use super::*;

    /// Entry point for Server connection over RakNet.
    pub type ServerLogin = BedrockStream<Handshake, Server, RakNetTransport>;
    pub type ServerSecurePending = BedrockStream<SecurePending, Server, RakNetTransport>;
    pub type ServerResourcePacks = BedrockStream<ResourcePacks, Server, RakNetTransport>;
    pub type ServerStartGame = BedrockStream<StartGame, Server, RakNetTransport>;
    pub type ServerPlay = BedrockStream<Play, Server, RakNetTransport>;

    /// Entry point for Client connection over RakNet.
    pub type ClientLogin = BedrockStream<Handshake, Client, RakNetTransport>;
    pub type ClientSecurePending = BedrockStream<SecurePending, Client, RakNetTransport>;
    pub type ClientResourcePacks = BedrockStream<ResourcePacks, Client, RakNetTransport>;
    pub type ClientStartGame = BedrockStream<StartGame, Client, RakNetTransport>;
    pub type ClientPlay = BedrockStream<Play, Client, RakNetTransport>;
}

// Re-export raknet types only when nethernet is NOT enabled (avoids ambiguity)
#[cfg(all(feature = "raknet", not(feature = "nethernet")))]
pub use raknet_types::*;

// NetherNet type aliases
#[cfg(feature = "nethernet")]
pub mod nethernet_types {
    use super::*;

    /// Entry point for Server connection over NetherNet (WebRTC).
    pub type ServerLogin = BedrockStream<Handshake, Server, NetherNetTransport>;
    pub type ServerSecurePending = BedrockStream<SecurePending, Server, NetherNetTransport>;
    pub type ServerResourcePacks = BedrockStream<ResourcePacks, Server, NetherNetTransport>;
    pub type ServerStartGame = BedrockStream<StartGame, Server, NetherNetTransport>;
    pub type ServerPlay = BedrockStream<Play, Server, NetherNetTransport>;

    /// Entry point for Client connection over NetherNet (WebRTC).
    pub type ClientLogin = BedrockStream<Handshake, Client, NetherNetTransport>;
    pub type ClientSecurePending = BedrockStream<SecurePending, Client, NetherNetTransport>;
    pub type ClientResourcePacks = BedrockStream<ResourcePacks, Client, NetherNetTransport>;
    pub type ClientStartGame = BedrockStream<StartGame, Client, NetherNetTransport>;
    pub type ClientPlay = BedrockStream<Play, Client, NetherNetTransport>;
}

// Re-export nethernet types (preferred when both features enabled)
#[cfg(feature = "nethernet")]
pub use nethernet_types::*;
