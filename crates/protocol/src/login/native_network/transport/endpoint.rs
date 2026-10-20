//! Resolved alternatives share the original join cutoff and own their actual connection attempts.

use super::{LanTransport, NativeConnectionTarget, NativeTransport, RakNetTransport, Transport};
use bedrock_net::raknet::{RakNetConnection, RakNetOptions};
use futures::{StreamExt, stream::FuturesUnordered};
use std::{future::Future, io, net::SocketAddr};
use tokio::time::Instant;

/// Resolves once without extending the caller's existing transport/Login/packs/spawn budget.
pub(super) async fn connect(
    target: &NativeConnectionTarget,
    identity: Option<&super::identity::TransportIdentity>,
    deadline: Instant,
) -> io::Result<Transport> {
    let addresses = before_deadline(deadline, tokio::net::lookup_host(&target.address)).await?;
    connect_resolved(target.transport, addresses, identity, deadline).await
}

/// Drives every compatible resolved alternative so a silent first answer cannot starve the rest.
/// Returning one ready transport or canceling this wait drops all other actual socket owners.
async fn connect_resolved(
    transport: NativeTransport,
    addresses: impl Iterator<Item = SocketAddr>,
    identity: Option<&super::identity::TransportIdentity>,
    deadline: Instant,
) -> io::Result<Transport> {
    before_deadline(deadline, async {
        let mut candidates = FuturesUnordered::new();
        let mut seen = std::collections::HashSet::new();
        for address in addresses {
            if address.port() == 0
                || (transport == NativeTransport::NetherNetLan && !address.is_ipv4())
                || !seen.insert(address)
            {
                continue;
            }
            candidates.push(async move {
                match transport {
                    NativeTransport::RakNet => {
                        let defaults = RakNetOptions::default();
                        let options = RakNetOptions {
                            connect_timeout: defaults
                                .connect_timeout
                                .min(deadline.saturating_duration_since(Instant::now())),
                            ..defaults
                        };
                        RakNetConnection::connect(address, options)
                            .await
                            .map(RakNetTransport::new)
                            .map(Transport::RakNet)
                            .map_err(io::Error::other)
                    }
                    NativeTransport::NetherNetLan => {
                        LanTransport::connect(address, identity, deadline)
                            .await
                            .map(Box::new)
                            .map(Transport::NetherNetLan)
                    }
                    NativeTransport::Addressed
                    | NativeTransport::NetherNetMessaging
                    | NativeTransport::NetherNetLegacy
                    | NativeTransport::LocalNetherNetHttp
                    | NativeTransport::LocalNetherNetLan => Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "messaging handoff cannot resolve direct endpoints",
                    )),
                }
            });
        }
        let mut failure = io::Error::new(
            io::ErrorKind::AddrNotAvailable,
            "native endpoint has no compatible resolved address",
        );
        while let Some(result) = candidates.next().await {
            match result {
                Ok(transport) => return Ok(transport),
                Err(error) => failure = error,
            }
        }
        Err(failure)
    })
    .await
}

/// Rejects even a ready result at the original cutoff, retaining no newly successful owner afterward.
async fn before_deadline<T>(
    deadline: Instant,
    operation: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    if Instant::now() >= deadline {
        return Err(timed_out());
    }
    let result = tokio::time::timeout_at(deadline, operation)
        .await
        .map_err(|_| timed_out())?;
    if Instant::now() >= deadline {
        return Err(timed_out());
    }
    result
}

/// Preserves the native join's existing finite timeout classification.
fn timed_out() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "native join timed out")
}

#[cfg(test)]
mod tests;
