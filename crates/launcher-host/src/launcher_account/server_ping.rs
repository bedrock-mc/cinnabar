//! Finite anonymous server-row pings driven on the existing application reactor.

use bridge::ServerPing;
use futures::{
    StreamExt,
    stream::{self, FuturesUnordered},
};
use raknet::prelude::{
    ClientGuid, RakClient, RakClientConfig, RakClientOutput, RakClock, canonical_socket_addr,
    is_transient_udp_read_error,
};
use std::{
    collections::{HashMap, HashSet},
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
};
use tokio::{net::UdpSocket, time::Instant};

const TIMEOUT: std::time::Duration =
    std::time::Duration::from_millis(bridge::contract::PING_TIMEOUT_MILLIS as u64);

/// Bounds actual target work; dropping the round drops all pending DNS/socket owners.
pub(super) async fn round(addresses: &[String]) -> Vec<ServerPing> {
    let mut results = stream::iter(
        addresses
            .iter()
            .take(bridge::contract::MAX_PING_TARGETS)
            .enumerate(),
    )
    .map(|(index, address)| async move { (index, one(address).await) })
    .buffer_unordered(bridge::contract::PING_WORKERS)
    .collect::<Vec<_>>()
    .await;
    results.sort_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

/// Resolves once under the original whole-attempt budget, never interpreting service namespaces as DNS.
async fn one(address: &str) -> ServerPing {
    let mut result = ServerPing {
        address: address.to_owned(),
        ..Default::default()
    };
    let bridge::ConnectTarget::RakNet(target) = crate::launcher_core::target_for(address) else {
        return result;
    };
    if address.trim().is_empty() {
        return result;
    }
    let started = Instant::now();
    let deadline = started + TIMEOUT;
    let response = tokio::time::timeout_at(deadline, async {
        let addresses = tokio::net::lookup_host(target).await?;
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        resolved(addresses, deadline).await
    })
    .await;
    let Ok(Ok(status)) = response else {
        return result;
    };
    if Instant::now() >= deadline {
        return result;
    }
    result.online = true;
    result.motd = status.name.trim().to_owned();
    result.players = u32::try_from(status.players.max(0)).unwrap_or(u32::MAX);
    result.max_players = u32::try_from(status.max_players.max(0)).unwrap_or(u32::MAX);
    result.ping_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    result
}

/// Every resolved peer is probed without allocating a socket per DNS answer.
/// At most two actual sockets exist for this target; the first valid family result wins.
async fn resolved(
    addresses: impl Iterator<Item = SocketAddr>,
    deadline: Instant,
) -> io::Result<protocol::ServerListStatus> {
    let mut seen = HashSet::new();
    let (mut v4, mut v6) = (Vec::new(), Vec::new());
    for peer in addresses.map(canonical_socket_addr) {
        if peer.port() != 0 && seen.insert(peer) {
            if peer.is_ipv4() {
                v4.push(peer);
            } else {
                v6.push(peer);
            }
        }
    }
    let mut families = FuturesUnordered::new();
    for peers in [v4, v6] {
        if !peers.is_empty() {
            families.push(family(peers, deadline));
        }
    }
    let mut error = io::Error::new(
        io::ErrorKind::AddrNotAvailable,
        "server ping has no resolved address",
    );
    while let Some(result) = families.next().await {
        match result {
            Ok(status) => return Ok(status),
            Err(failure) => error = failure,
        }
    }
    Err(error)
}

/// Retains one actual datagram while servicing reads, so send backpressure cannot hide a ready pong.
async fn family(
    peers: Vec<SocketAddr>,
    deadline: Instant,
) -> io::Result<protocol::ServerListStatus> {
    if Instant::now() >= deadline {
        return Err(timed_out());
    }
    let bind = if peers[0].is_ipv4() {
        SocketAddr::from((Ipv4Addr::UNSPECIFIED, 0))
    } else {
        SocketAddr::from((Ipv6Addr::UNSPECIFIED, 0))
    };
    let socket = UdpSocket::bind(bind).await?;
    let now = Instant::now();
    if now >= deadline {
        return Err(timed_out());
    }
    let clock = RakClock::new(now.into_std());
    let mut client = RakClient::new(RakClientConfig::default(), clock).map_err(io::Error::other)?;
    let guid = ClientGuid::generate()?;
    let mut peers = peers.into_iter();
    let mut sent = HashMap::new();
    let mut pending = None;
    let mut input = vec![0; usize::from(u16::MAX)];
    loop {
        if Instant::now() >= deadline {
            return Err(timed_out());
        }
        while let Some(output) = client.poll() {
            match output {
                RakClientOutput::SocketDatagram(bytes, peer) => {
                    if pending.is_some() {
                        return Err(io::Error::other("unexpected concurrent ping datagram"));
                    }
                    pending = Some((bytes, peer));
                }
                RakClientOutput::Pong {
                    addr,
                    message,
                    echoed_timestamp,
                    ..
                } if sent.get(&canonical_socket_addr(addr)) == Some(&echoed_timestamp) => {
                    if let Ok(status) = protocol::ServerListStatus::parse_pong(&message) {
                        return Ok(status);
                    }
                }
                _ => {}
            }
        }
        if pending.is_none()
            && let Some(peer) = peers.next()
        {
            let now = Instant::now().into_std();
            sent.insert(peer, clock.timestamp(now));
            client.ping(peer, guid, now).map_err(io::Error::other)?;
            continue;
        }
        tokio::select! {
            _ = tokio::time::sleep_until(deadline) => return Err(timed_out()),
            read = socket.recv_from(&mut input) => match read {
                Ok((length, peer)) if sent.contains_key(&canonical_socket_addr(peer)) => {
                    // A malformed response does not retire other already-probed alternatives.
                    let _ = client.receive(input[..length].to_vec().into_boxed_slice(), peer, Instant::now().into_std());
                }
                Ok(_) => {},
                Err(error) if error.kind() == io::ErrorKind::Interrupted
                    || is_transient_udp_read_error(&error) => {},
                Err(error) => return Err(error),
            },
            write = async {
                match &pending {
                    Some((bytes, peer)) => socket.send_to(bytes, *peer).await,
                    None => std::future::pending().await,
                }
            } => match write {
                Ok(length) => {
                    let (bytes, peer) = pending.take().expect("pending ping datagram");
                    if length != bytes.len() { sent.remove(&peer); }
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {},
                Err(_) => {
                    let (_, peer) = pending.take().expect("pending ping datagram");
                    sent.remove(&peer);
                }
            },
        }
    }
}

/// Every phase shares the original finite attempt cutoff.
fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "server ping timed out")
}

#[cfg(test)]
mod tests;
