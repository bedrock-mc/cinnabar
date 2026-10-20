//! Native direct joins using the shared Rust spawn and play sequence.

mod accepting;
mod authentication;
pub use accepting::{
    NativeAcceptingCapacity, NativeAcceptingIo, NativeAcceptingRetired, NativeAcceptingSend,
    NativeAcceptingSendOutcome, NativeAcceptingSendRetirement, NativeHostWorld, native_host_packs,
};
pub use authentication::{
    NativeAuthentication, NativeCredential, NativeHostingIdentity, NativeProvider,
    NativeServiceClock, NativeTokenVerifier,
};
mod connection;
mod hosted;
pub use hosted::{
    NativeHostedRelay, NativeHostedRelayClosing, NativeHostedRelayProgress, NativeHostedUpstream,
    NativeHostedUpstreamClosing,
};
#[cfg(feature = "test-support")]
pub(crate) mod fixtures;
mod negotiated;
mod packs;
mod relay;
pub use relay::{
    NativeRelayClosing, NativeRelayConnecting, NativeRelayConnection, NativeRelayDriver,
    NativeRelayHandoff, NativeRelayInbound, NativeRelayRetired, NativeRelaySender,
    NativeRelayWritten,
};
mod spawn;
mod transport;
pub(crate) use connection::{Connection, PacketConnection};

use super::{LoginSequence, PlaySession, SessionLogin};
use crate::codec::SessionFailure;
use crate::{GameData, ProtocolError};
use crate::{NativeConnectionTarget, NativeTransport};
use bedrock_client::handshake::{ClientKeyExchange, HandshakeError};
use bedrock_packs::ResourcePackHandoff;
use bedrock_protocol_1_26_51::packets;
use std::io;

/// Retains a decoded negotiation boundary when a response write fails.
async fn preserve_negotiation_boundary(
    connection: &mut impl PacketConnection,
    error: io::Error,
) -> ProtocolError {
    let drain = async {
        let limit = bedrock_net::BatchLimits::default().max_decoded_bytes;
        let mut retained = 0_usize;
        loop {
            let body = connection.recv_body().await.ok()?;
            retained = retained.checked_add(body.len())?;
            if retained > limit {
                return None;
            }
            for raw in crate::raw_packet::decode_batch_body(body, limit).ok()? {
                if matches!(
                    raw.id,
                    packets::PacketId::Transfer | packets::PacketId::Disconnect
                ) {
                    let packet = raw.decode().ok()?;
                    match negotiation_boundary(*packet.data) {
                        Ok(Some(error)) => return Some(error),
                        Err(reason) => tracing::warn!(
                            ?reason,
                            "skipped unusable transfer while draining negotiation"
                        ),
                        Ok(None) => {}
                    }
                }
            }
        }
    };
    match tokio::time::timeout(super::WRITE_FAILURE_DRAIN, drain).await {
        Ok(Some(boundary)) => boundary,
        _ => failure(error),
    }
}

/// Returns only a validated redirect or the server's decoded disconnect reason.
fn negotiation_boundary(
    packet: packets::Packet,
) -> Result<Option<ProtocolError>, crate::ServerTransferRejection> {
    if let packets::Packet::Disconnect(packet) = packet {
        return Ok(Some(disconnected(packet)));
    }
    Ok(crate::ServerTransferEvent::from_packet_data(&packet)?
        .map(|target| SessionFailure::ServerTransfer(target).into()))
}

#[cfg(test)]
mod tests;

/// Classifies a decoded negotiation packet that did not advance the current phase.
fn negotiation_failure(
    packet: packets::Packet,
) -> Result<ProtocolError, crate::ServerTransferRejection> {
    let id = packet.id();
    if let Some(boundary) = negotiation_boundary(packet)? {
        return Ok(boundary);
    }
    Ok(failure(io::Error::new(
        io::ErrorKind::InvalidData,
        format!("unexpected negotiation packet: {id:?}"),
    )))
}

impl LoginSequence {
    /// Joins directly without a Go process. The timeout includes transport, Login,
    /// resource-pack transfer and spawn; dropping the future cancels the connection.
    pub async fn connect_native<P: Send + 'static>(
        target: &NativeConnectionTarget,
        login: SessionLogin<'_>,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession, P), ProtocolError> {
        native_join(target, login, None, prepare).await
    }

    /// Joins the selected native transport with an acquired account credential.
    ///
    /// Account HTTP and persistence stay with the application. The connection
    /// retains this key through signed client data and the server key exchange.
    /// Player Messaging additionally requires a protected socket and joined-session target.
    ///
    /// # Errors
    /// Rejects unsupported transport or invalid framing, and returns connection,
    /// Login, pack, spawn or overall join-deadline failures.
    pub async fn connect_native_authenticated<P: Send + 'static>(
        target: &NativeConnectionTarget,
        login: SessionLogin<'_>,
        authentication: NativeAuthentication,
        prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
    ) -> Result<(PlaySession, P), ProtocolError> {
        native_join(target, login, Some(authentication), prepare).await
    }
}

/// Applies the same cancellation and join deadline to either explicit identity.
async fn native_join<P: Send + 'static>(
    target: &NativeConnectionTarget,
    login: SessionLogin<'_>,
    authentication: Option<NativeAuthentication>,
    prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
) -> Result<(PlaySession, P), ProtocolError> {
    let deadline = tokio::time::Instant::now() + crate::NATIVE_JOIN_TIMEOUT;
    tokio::time::timeout_at(
        deadline,
        connect(target, login, authentication, deadline, prepare),
    )
    .await
    .map_err(|_| {
        failure(io::Error::new(
            io::ErrorKind::TimedOut,
            "native join timed out",
        ))
    })?
}

/// Negotiates one native connection before handing the same owner to play.
async fn connect<P: Send + 'static>(
    target: &NativeConnectionTarget,
    login: SessionLogin<'_>,
    authentication: Option<NativeAuthentication>,
    deadline: tokio::time::Instant,
    prepare: impl FnOnce(ResourcePackHandoff, GameData) -> P + Send + 'static,
) -> Result<(PlaySession, P), ProtocolError> {
    let negotiated = negotiated::negotiate(target, &login, authentication, deadline).await?;
    spawn::continue_join(negotiated, login, prepare).await
}

/// Installs the verified streams and flushes their acknowledgment before returning the same owner.
async fn apply_key_exchange<C>(
    connection: &mut Connection,
    exchange: ClientKeyExchange<C>,
) -> Result<C, ProtocolError> {
    let (continuation, ciphers, response) = exchange.into_parts();
    if let Some(ciphers) = ciphers {
        connection.enable_encryption(*ciphers).map_err(failure)?;
    }
    connection.send(response).await?;
    Ok(continuation)
}

/// Keeps the reusable handshake failure as a typed cause of the native connection error.
fn handshake_failure(error: HandshakeError) -> ProtocolError {
    let kind = if matches!(error, HandshakeError::LoginRefused(_)) {
        io::ErrorKind::PermissionDenied
    } else {
        io::ErrorKind::InvalidData
    };
    failure(io::Error::new(kind, error))
}

/// Preserves the server's disconnect text through the existing startup error model.
fn disconnected(packet: packets::Disconnect) -> ProtocolError {
    let (message, filtered) = crate::disconnect::packet_texts(&packet);
    crate::disconnect::join_disconnect("native Login", &packet.reason, &message, &filtered).into()
}

/// Attaches the native transport error without labelling it as malformed packet input.
fn failure(error: io::Error) -> ProtocolError {
    SessionFailure::NativeTransport(error).into()
}
