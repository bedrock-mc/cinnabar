//! Resolved alternatives share the original join cutoff and own their actual connection attempts.

use super::{NativeConnectionTarget, NativeTransport, Transport, lan};
use bedrock_net::raknet::RakNetOptions;
use std::{future::Future, io};
use tokio::time::Instant;

/// Resolves once without extending the caller's existing transport/Login/packs/spawn budget.
pub(super) async fn connect(
    target: &NativeConnectionTarget,
    identity: Option<&super::identity::TransportIdentity>,
    deadline: Instant,
) -> io::Result<Transport> {
    let addresses = before_deadline(deadline, tokio::net::lookup_host(&target.address)).await?;
    match target.transport {
        NativeTransport::RakNet => {
            bedrock_client_tokio::connect::raknet::connect_resolved(
                addresses,
                RakNetOptions::default(),
                deadline,
            )
            .await
        }
        NativeTransport::NetherNetLan => {
            let _ = rustls::crypto::ring::default_provider().install_default();
            let options = lan::direct_options(identity);
            bedrock_client_tokio::connect::lan::connect_resolved(addresses, &options, deadline)
                .await
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "messaging handoff cannot resolve direct endpoints",
        )),
    }
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
