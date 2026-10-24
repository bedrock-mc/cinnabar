//! Exact numeric upstream admission before the one retained RakNet socket is connected.

use super::session::PlayLease;
use bedrock_net::raknet::{RakNetConnection, RakNetOptions};
use bridge::{
    ConnectTarget, contract::GATEWAY_EGRESS_CANDIDATES, gateway_egress::public_gateway_upstream,
};
use protocol::NativeRelayConnecting;
use std::{io, net::SocketAddr, time::Instant};

#[cfg(test)]
tokio::task_local! {
    pub(super) static FIXTURE_UPSTREAM: SocketAddr;
}

/// Selects only the explicit direct host:nonzero-port grammar of the existing Go gateway.
pub(super) fn select(target: Option<ConnectTarget>) -> io::Result<(String, String, u16)> {
    let Some(ConnectTarget::RakNet(address)) = target else {
        return Err(refused());
    };
    let address = address.trim().to_owned();
    let (host, port) = address.rsplit_once(':').ok_or_else(refused)?;
    let port = port
        .parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(refused)?;
    let host = if let Some(host) = host
        .strip_prefix('[')
        .and_then(|host| host.strip_suffix(']'))
    {
        host.parse::<std::net::Ipv6Addr>().map_err(|_| refused())?;
        host
    } else {
        if host.contains([':', '[', ']']) {
            return Err(refused());
        }
        host
    };
    if host.is_empty()
        || host.contains('%')
        || host.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return Err(refused());
    }
    Ok((address.clone(), host.to_owned(), port))
}

/// Resolves once, checks each exact numeric peer immediately before connecting, and never redials a name.
pub(super) async fn connect(
    lease: &PlayLease,
    target: Option<ConnectTarget>,
    deadline: Instant,
) -> io::Result<NativeRelayConnecting> {
    let (address, host, port) = select(target)?;
    current(lease, deadline)?;
    let resolved = tokio::time::timeout_at(
        deadline.into(),
        tokio::net::lookup_host((host.as_str(), port)),
    )
    .await
    .map_err(|_| elapsed())??;
    current(lease, deadline)?;
    let candidates = resolved.take(GATEWAY_EGRESS_CANDIDATES).collect::<Vec<_>>();
    let mut failure = refused();
    for candidate in candidates {
        current(lease, deadline)?;
        if matches!(candidate, SocketAddr::V6(peer) if peer.scope_id() != 0) {
            continue;
        }
        let candidate = SocketAddr::new(candidate.ip().to_canonical(), candidate.port());
        if !admitted(candidate) {
            continue;
        }
        let connecting = RakNetConnection::connect(candidate, RakNetOptions::default());
        let connection = match tokio::time::timeout_at(deadline.into(), connecting).await {
            Ok(Ok(connection)) => connection,
            Ok(Err(error)) => {
                failure = io::Error::other(error);
                continue;
            }
            Err(_) => return Err(elapsed()),
        };
        // Keep this actual socket reachable for caller-owned cleanup even if admission retired while awaiting it.
        let connection = NativeRelayConnecting::from_raknet(connection, address.clone());
        if let Err(error) = current(lease, deadline) {
            let mut retiring = connection.begin_close();
            let _ = retiring.finish_close().await;
            return Err(error);
        }
        return Ok(connection);
    }
    Err(failure)
}

/// Checks the final numeric peer; test serving futures may admit their one exact fixture socket.
fn admitted(peer: SocketAddr) -> bool {
    let public = public_gateway_upstream(peer.ip());
    #[cfg(test)]
    {
        public
            || FIXTURE_UPSTREAM
                .try_with(|expected| *expected == peer)
                .unwrap_or(false)
    }
    #[cfg(not(test))]
    {
        public
    }
}

/// Rechecks the same join/account/controller and original deadline after every caller wait.
pub(super) fn current(lease: &PlayLease, deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        return Err(elapsed());
    }
    lease.current()
}

/// Redacts provider, address and resolver details at the application boundary.
fn refused() -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, "gateway upstream refused")
}

/// Keeps expiration distinct from a replaced controller without exposing its identity.
fn elapsed() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "gateway join timed out")
}
