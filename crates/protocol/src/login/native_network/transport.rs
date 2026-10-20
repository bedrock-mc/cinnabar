//! Exclusive native transport ownership for one direct connection.

mod accepted;
pub(super) mod addressed;
mod endpoint;
pub(super) mod http;
pub(super) mod identity;
mod inbox;
pub(super) mod lan;
mod messaging;
mod raknet;
use accepted::AcceptedTransport;
use inbox::Inbox;
use lan::LanTransport;
use messaging::MessagingTransport;
use raknet::RakNetTransport;

use super::{NativeConnectionTarget, NativeTransport};
use bedrock_net::{BatchHeader, raknet::RakNetConnection};
use bytes::Bytes;
#[cfg(feature = "test-support")]
use nethernet_tokio::NetherClient;
#[cfg(test)]
use nethernet_tokio::{ConnectionConfig, LanConfig, LanSignaling};
#[cfg(test)]
use std::net::SocketAddr;
use std::{future::Future, io, time::Instant};

pub(super) const LAN_CONNECTION_TYPE: u8 = 4;

pub(super) enum Transport {
    RakNet(RakNetTransport),
    NetherNetLan(Box<LanTransport>),
    Messaging(Box<MessagingTransport>),
    Accepted(Box<AcceptedTransport>),
    Http(Box<AcceptedTransport>),
}

impl Transport {
    /// Observes the selected transport's original full round-trip estimate.
    pub(super) fn observe_rtt(&self) -> Option<crate::RoundTripTime> {
        match self {
            Self::RakNet(transport) => transport.observe_rtt(),
            Self::NetherNetLan(transport) => transport.observe_rtt(),
            Self::Messaging(transport) => transport.observe_rtt(),
            Self::Accepted(transport) | Self::Http(transport) => transport.observe_rtt(),
        }
    }

    /// Reuses the actual accepted IO driver for a fixture's ready signed client and original challenge.
    #[cfg(feature = "test-support")]
    pub(super) fn fixture_nether(client: NetherClient, nonce: String) -> Self {
        Self::Accepted(Box::new(AcceptedTransport::fixture_client(client, nonce)))
    }

    /// Takes a private capacity handle only from the same established accepted association.
    pub(super) fn accepted_sender(&self) -> io::Result<nethernet_tokio::SessionSender> {
        match self {
            Self::Accepted(accepted) | Self::Http(accepted) => accepted.sender(),
            Self::NetherNetLan(lan) => lan.connection.sender(),
            _ => Err(prepared_unsupported()),
        }
    }

    /// Keeps the original accepted input progressing beside one retained operation.
    pub(super) async fn progress_accepted<T: Send>(
        &mut self,
        progress: impl Future<Output = T> + Send,
    ) -> io::Result<T> {
        match self {
            Self::Accepted(accepted) | Self::Http(accepted) => accepted.progress(progress).await,
            Self::NetherNetLan(lan) => lan.connection.progress(progress).await,
            _ => Err(prepared_unsupported()),
        }
    }

    /// Finishes an existing receipt after local admission has been sealed.
    pub(super) async fn settle_accepted<T: Send>(
        &mut self,
        progress: impl Future<Output = T> + Send,
    ) -> io::Result<T> {
        match self {
            Self::Accepted(accepted) | Self::Http(accepted) => accepted.settle(progress).await,
            Self::NetherNetLan(lan) => lan.connection.settle(progress).await,
            _ => Err(prepared_unsupported()),
        }
    }

    /// Takes only the caller's actual Ready association, never dialing another backend.
    pub(super) fn from_accepted(accepted: nethernet_tokio::AcceptedSession) -> Self {
        Self::Accepted(Box::new(AcceptedTransport::new(accepted)))
    }

    /// Ends server sending synchronously while retaining the same receiver prefix.
    pub(super) fn retire_accepted(&mut self) {
        if let Self::Accepted(accepted) = self {
            accepted.close();
        }
    }
    /// Retains the caller's already-established exact numeric RakNet socket without resolving again.
    pub(super) fn from_raknet(connection: RakNetConnection) -> Self {
        Self::RakNet(RakNetTransport::new(connection))
    }

    /// Resolves the requested endpoint and establishes its explicitly selected transport.
    pub(super) async fn connect(
        target: &NativeConnectionTarget,
        messaging: Option<super::authentication::MessagingAdmission>,
        identity: Option<&identity::TransportIdentity>,
        deadline: tokio::time::Instant,
    ) -> io::Result<Self> {
        if matches!(
            target.transport,
            NativeTransport::Addressed
                | NativeTransport::LocalNetherNetHttp
                | NativeTransport::LocalNetherNetLan
        ) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "selected transport requires its original route and Login policy",
            ));
        }
        if matches!(
            target.transport,
            NativeTransport::NetherNetMessaging | NativeTransport::NetherNetLegacy
        ) {
            return MessagingTransport::connect(target, messaging)
                .await
                .map(Box::new)
                .map(Self::Messaging);
        }
        if messaging.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "messaging handoff requires its exact target",
            ));
        }
        endpoint::connect(target, identity, deadline).await
    }

    /// Borrows the LAN discovery admission challenge, absent on RakNet.
    pub(super) fn nonce(&self) -> &str {
        match self {
            Self::NetherNetLan(transport) => transport.nonce(),
            Self::Messaging(transport) => transport.nonce(),
            #[cfg(feature = "test-support")]
            Self::Accepted(transport) => transport.fixture_nonce(),
            _ => "",
        }
    }

    /// Selects the transport's Bedrock batch envelope.
    pub(super) fn header(&self) -> BatchHeader {
        match self {
            Self::RakNet(_) => BatchHeader::RakNet,
            Self::NetherNetLan(_) => BatchHeader::NetherNet,
            Self::Messaging(_) => BatchHeader::NetherNet,
            Self::Accepted(_) | Self::Http(_) => BatchHeader::NetherNet,
        }
    }

    /// Application ciphers are required on RakNet; NetherNet already owns DTLS.
    pub(super) fn batch_encryption(&self) -> bool {
        matches!(self, Self::RakNet(_))
    }

    /// Applies a credential deadline to output admission, preserving any already accepted write's progress.
    pub(super) async fn send_until(
        &mut self,
        message: Bytes,
        deadline: Option<Instant>,
    ) -> io::Result<()> {
        match self {
            Self::Messaging(transport) => transport.send_until(message, deadline).await,
            Self::RakNet(transport) => transport.send_until(message, deadline).await,
            Self::NetherNetLan(transport) => transport.send_until(message, deadline).await,
            Self::Accepted(transport) => transport.send_until(message, deadline).await,
            Self::Http(transport) => {
                let result = transport.send_until(message, deadline).await;
                if let Some(deadline) = deadline {
                    super::authentication::ensure_current(deadline, Instant::now())?;
                }
                result
            }
        }
    }

    /// Receives one complete message; both drivers retain partial input on cancellation.
    pub(super) async fn recv(&mut self) -> io::Result<Bytes> {
        match self {
            Self::Messaging(transport) => transport.recv().await,
            Self::RakNet(transport) => transport.recv().await,
            Self::NetherNetLan(transport) => transport.recv().await,
            Self::Accepted(transport) | Self::Http(transport) => transport.recv().await,
        }
    }

    /// Polls service/control progress while application payload admission is paused.
    pub(super) async fn maintain(&mut self) {
        match self {
            Self::Messaging(transport) => transport.maintain().await,
            Self::RakNet(transport) => transport.maintain().await,
            _ => std::future::pending().await,
        }
    }

    /// Consumes local admission while retaining the same queues across any canceled teardown wait.
    pub(super) async fn close(&mut self) -> io::Result<()> {
        match self {
            Self::Messaging(transport) => {
                transport.close();
                Ok(())
            }
            Self::RakNet(transport) => transport.close().await,
            Self::NetherNetLan(transport) => {
                transport.close();
                Ok(())
            }
            Self::Accepted(transport) | Self::Http(transport) => {
                transport.close();
                Ok(())
            }
        }
    }
}

/// Rejects application sends after the actual lifecycle capability was consumed.
fn closing() -> io::Error {
    io::Error::new(io::ErrorKind::NotConnected, "native transport is closing")
}

#[cfg(test)]
pub(super) mod early_spawn_tests;

#[cfg(test)]
mod early_packet_tap_tests;

#[cfg(test)]
mod relay_startup_tests;

#[cfg(test)]
mod startup_unknown_tests;

#[cfg(test)]
mod pack_url_policy_tests;

#[cfg(test)]
mod pack_url_parallel_tests;

/// Rejects prepared effects on a different transport without starting another association.
fn prepared_unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "prepared send requires accepted NetherNet",
    )
}
