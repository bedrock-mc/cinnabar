//! Application route selection; wire setup/lifecycles belong to bedrock-client-tokio.

pub(super) mod addressed;
mod endpoint;
pub(super) mod http;
pub(super) mod identity;
pub(super) mod lan;
mod messaging;
use super::{NativeConnectionTarget, NativeTransport};
use bedrock_client_tokio::transport::Transport;
#[cfg(test)]
use bedrock_net::BatchHeader;
#[cfg(test)]
use bytes::Bytes;
#[cfg(test)]
use nethernet_tokio::{ConnectionConfig, LanConfig, LanSignaling};
use std::io;
#[cfg(test)]
use std::net::SocketAddr;
#[cfg(test)]
use std::time::Instant;

#[cfg(test)]
pub(super) const LAN_CONNECTION_TYPE: u8 = 4;

/// Resolves the requested endpoint and establishes its explicitly selected transport.
pub(super) async fn connect(
    target: &NativeConnectionTarget,
    messaging: Option<super::authentication::MessagingAdmission>,
    identity: Option<&identity::TransportIdentity>,
    deadline: tokio::time::Instant,
) -> io::Result<Transport> {
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
        return messaging::connect(target, messaging).await;
    }
    if messaging.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "messaging handoff requires its exact target",
        ));
    }
    endpoint::connect(target, identity, deadline).await
}

#[cfg(test)]
pub(super) mod early_spawn_tests;

#[cfg(test)]
mod early_packet_tap_tests;

#[cfg(test)]
mod relay_startup_tests;

#[cfg(test)]
mod relay_raw_startup_tests;

#[cfg(test)]
mod startup_unknown_tests;

#[cfg(test)]
mod pack_url_policy_tests;

#[cfg(test)]
mod pack_url_parallel_tests;

#[cfg(test)]
#[path = "transport/inbox_actor_tests.rs"]
mod inbox_actor_tests;
#[cfg(test)]
#[path = "transport/raknet/tests.rs"]
mod raknet_tests;

/// Checks fixture advertisements against the selected library's LAN wire default.
#[test]
fn fixture_lan_connection_type_tracks_the_selected_wire_owner() {
    assert_eq!(
        LAN_CONNECTION_TYPE,
        nethernet_tokio::ServerData::new(String::new(), String::new()).connection_type
    );
}
