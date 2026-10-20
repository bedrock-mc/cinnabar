//! Concrete wire ownership for an already admitted NetherNet server association.

mod packs;
pub(super) mod send;
mod world;
pub use packs::native_host_packs;
pub use send::{
    NativeAcceptingCapacity, NativeAcceptingSend, NativeAcceptingSendOutcome,
    NativeAcceptingSendRetirement,
};
pub use world::NativeHostWorld;

use super::{Connection, PacketConnection, transport::Transport};
use bedrock_client::server::{ServerHandshake, ServerHandshakeOptions};
use bedrock_net::encryption::{BatchDecryptor, BatchEncryptor};
use bedrock_net::{BatchLimits, CompressionSettings};
use bedrock_protocol_1_26_51::{packets, wire};
use bytes::Bytes;
use p384::ecdsa::SigningKey;
use std::{io, sync::Arc, time::Instant};

/// The sole codec/cipher and receive FIFO for one actual established server association.
/// Construction grants no Login, hosted membership or selected-world authority.
#[must_use]
pub struct NativeAcceptingIo {
    pub(in crate::login::native_network) connection: Connection,
    skipped: u64,
}

/// Local send admission ended; the same already accepted receive prefix can still drain.
#[must_use]
pub struct NativeAcceptingRetired {
    connection: Connection,
}

impl NativeAcceptingIo {
    /// Takes an actual Ready association and binds Login to its original authenticated SDP key.
    /// The caller must have configured its server's real transport identity/signaling policy and
    /// supply that same server proof key. This performs no dial or provider/account acquisition.
    pub async fn from_nether(
        accepted: nethernet_tokio::AcceptedSession,
        key: Arc<SigningKey>,
        mut options: ServerHandshakeOptions,
        deadline: Instant,
    ) -> io::Result<(Self, ServerHandshake)> {
        let player = wait(deadline, accepted.session.player())
            .await?
            .ok_or_else(denied)?;
        let transport_key = player.client_public_key().map_err(|_| denied())?;
        // The actual reliable association is already DTLS protected.
        options.batch_encryption = false;
        let handshake =
            ServerHandshake::new(key, Some(transport_key), options, deadline, Instant::now())
                .map_err(|_| denied())?;
        let defaults = BatchLimits::default();
        // Accepted sessions retain their incoming packet-count limit.
        let limits = BatchLimits {
            max_incoming_packets: Some(defaults.max_packets),
            ..defaults
        };
        Ok((
            Self {
                connection: Connection::with_limits(Transport::from_accepted(accepted), limits),
                skipped: 0,
            },
            handshake,
        ))
    }

    /// Reads one primary generated packet while retaining every original batch sibling.
    /// Unknown or secondary packets the accepting owner does not use are counted and skipped.
    pub async fn receive(&mut self, deadline: Instant) -> io::Result<packets::Packet> {
        loop {
            let raw = self.receive_raw(deadline).await?;
            let mut reader = wire::Reader::from_shared(&raw);
            let header =
                wire::PacketHeader::from_raw(reader.read_var_u32().map_err(|_| invalid())?);
            let id = packets::PacketId::from_raw(header.id);
            if header.sender_subclient != 0 || header.target_subclient != 0 || id.is_none() {
                self.skipped = self.skipped.saturating_add(1);
                tracing::debug!(skipped = self.skipped, "skipped unused accepting packet");
                continue;
            }
            let body = raw.slice(reader.position()..);
            return crate::codec::decode_client_body(id.expect("classified packet"), &body)
                .map_err(|_| invalid());
        }
    }

    /// Ends decoded peer terminals and counts known input unused by the accepting continuation.
    pub fn handle_unhandled(&mut self, packet: &packets::Packet) -> io::Result<()> {
        if matches!(
            packet,
            packets::Packet::Disconnect(_) | packets::Packet::Transfer(_)
        ) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "native hosted peer ended",
            ));
        }
        self.skipped = self.skipped.saturating_add(1);
        tracing::debug!(id = ?packet.id(), skipped = self.skipped, "skipped unused native hosted packet");
        Ok(())
    }

    /// Installs only the settings whose exact plaintext response has actually flushed.
    pub fn compression_flushed(&mut self, settings: CompressionSettings) -> io::Result<()> {
        self.connection.enable_compression(settings)
    }

    /// Installs original streams only after their exact plaintext token has actually flushed.
    pub fn encryption_flushed(
        &mut self,
        ciphers: (BatchEncryptor, BatchDecryptor),
    ) -> io::Result<()> {
        self.connection.enable_encryption(ciphers)
    }

    /// Takes the original complete packet header/body; no decode/re-encode changes play packets.
    pub async fn receive_raw(&mut self, deadline: Instant) -> io::Result<Bytes> {
        wait(deadline, self.connection.next_raw()).await?
    }

    /// Waits for ordinary initialized play without imposing a new idle timeout.
    pub async fn receive_play(&mut self) -> io::Result<Bytes> {
        self.connection.next_raw().await
    }

    /// Consumes local send capability and retains the actual accepted receiver and decoded FIFO.
    pub fn begin_close(mut self) -> NativeAcceptingRetired {
        self.connection.transport.retire_accepted();
        NativeAcceptingRetired {
            connection: self.connection,
        }
    }
}

impl NativeAcceptingRetired {
    /// Drains the original prefix without minting another established send capability.
    pub async fn receive_raw(&mut self, deadline: Instant) -> io::Result<Bytes> {
        wait(deadline, self.connection.next_raw()).await?
    }
    /// Completes original transport retirement without starting a replacement association.
    pub async fn finish_close(&mut self, deadline: Instant) -> io::Result<()> {
        wait(deadline, self.connection.close()).await?
    }
}

/// Keeps every wait under its original edge deadline, including a late ready result.
async fn wait<T>(deadline: Instant, future: impl std::future::Future<Output = T>) -> io::Result<T> {
    if Instant::now() >= deadline {
        return Err(expired());
    }
    let result = tokio::time::timeout_at(deadline.into(), future)
        .await
        .map_err(|_| expired())?;
    if Instant::now() >= deadline {
        return Err(expired());
    }
    Ok(result)
}

/// Redacts signed metadata and provider diagnostics at the process bridge boundary.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid native accepting packet",
    )
}
/// Reports only the failed authority class.
fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native accepting identity rejected",
    )
}
/// Uses one static timeout cause without identity-bearing values.
fn expired() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "native accepting deadline expired")
}
