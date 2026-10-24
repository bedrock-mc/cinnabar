//! Application admission/configuration for the shared directed LAN connection owner.

use super::{Transport, identity::TransportIdentity};
use bedrock_client_tokio::connect::lan::{DirectedLanConfig, LanSelection};
use nethernet_tokio::{ClientIceConfig, ConnectionConfig};
use std::{io, net::SocketAddr};

/// Retains actual selected proof/trust and the original direct online/offline wire policy.
pub(super) fn direct_options(identity: Option<&TransportIdentity>) -> DirectedLanConfig {
    let mut connection = ConnectionConfig {
        max_incoming_message_bytes: bedrock_net::BatchLimits::default().max_encoded_bytes,
        allow_unsigned_answers: true,
        attempts: 1,
        ..ConnectionConfig::default()
    };
    if let Some(identity) = identity {
        connection.identity = Some(std::sync::Arc::clone(&identity.identity));
        connection.server_trust = Some(identity.trust.clone());
        connection.client_ice = Some(ClientIceConfig {
            turn_streams: Vec::new(),
            turn_tls: None,
            ice: Default::default(),
            bind_addr: None,
        });
    }
    DirectedLanConfig {
        connection,
        selection: if identity.is_some() {
            LanSelection::Online
        } else {
            LanSelection::SelfSigned
        },
        max_nonce_bytes: crate::MAX_NATIVE_JOIN_NONCE_BYTES,
        credential_expires: identity.and_then(|identity| identity.expires),
    }
}

/// Qualifies the original selected local level before handing off explicit transport configuration.
pub(in crate::login::native_network) async fn connect_local(
    address: SocketAddr,
    level: &str,
    discovery_budget: std::time::Duration,
    deadline: tokio::time::Instant,
) -> io::Result<(Transport, u64)> {
    if !address.is_ipv4()
        || !address.ip().is_loopback()
        || address.port() == 0
        || level.is_empty()
        || discovery_budget.is_zero()
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid selected local LAN route",
        ));
    }
    let options = DirectedLanConfig {
        connection: ConnectionConfig {
            server_trust: Some(super::super::authentication::offline_answer_trust()),
            allow_anonymous_answers: true,
            client_ice: Some(ClientIceConfig {
                turn_streams: Vec::new(),
                turn_tls: None,
                ice: Default::default(),
                bind_addr: Some(SocketAddr::new(address.ip(), 0)),
            }),
            ..direct_options(None).connection
        },
        selection: LanSelection::Local {
            level: level.to_owned(),
            discovery_budget,
        },
        max_nonce_bytes: crate::MAX_NATIVE_JOIN_NONCE_BYTES,
        credential_expires: None,
    };
    let _ = rustls::crypto::ring::default_provider().install_default();
    bedrock_client_tokio::connect::lan::connect_directed(address, &options, deadline).await
}

#[cfg(test)]
mod tests;
