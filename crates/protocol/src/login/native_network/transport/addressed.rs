//! Address discovery and first-use authorization on the original native join lifetime.

use super::{Transport, endpoint, http, identity::TransportIdentity};
use crate::NativeConnectionTarget;
mod cache;
#[cfg(test)]
mod tests;
use cache::ProbeCache;
use std::{
    io,
    net::{IpAddr, SocketAddr},
    sync::{Mutex, OnceLock},
    time::{Duration, Instant},
};

const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
const TRUST_REDIAL_AFTER: Duration = Duration::from_secs(5);

/// The process's original addressed-server probe choices; no account or credential is cached.
fn probes() -> &'static Mutex<ProbeCache> {
    static PROBES: OnceLock<Mutex<ProbeCache>> = OnceLock::new();
    PROBES.get_or_init(Mutex::default)
}

/// Selects once and carries the actual HTTP association into the same Login/pack/spawn driver.
pub(in crate::login::native_network) async fn connect(
    target: &NativeConnectionTarget,
    identity: TransportIdentity,
    deadline: tokio::time::Instant,
) -> io::Result<(Transport, String)> {
    let route = target.addressed_route()?;
    let cutoff = identity
        .expires
        .map_or(deadline, |expires| deadline.min(expires.into()));
    check(cutoff, identity.expires)?;
    let client = http::client(cutoff)?;
    let key = format!("{}:{}", route.host, route.port.unwrap_or(0));
    let cached = probes()
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .lookup(&key, Instant::now());
    let choice = match cached {
        Some(choice) => choice,
        None => {
            let choice = if route.ipv6 {
                None
            } else {
                probe(&client, &route.host, route.port, cutoff).await
            };
            check(cutoff, identity.expires)?;
            let now = Instant::now();
            let mut cache = probes().lock().unwrap_or_else(|error| error.into_inner());
            cache.store(key.clone(), choice.clone(), now);
            choice
        }
    };
    let result = match choice {
        Some(url) => connect_http(&url, &identity, &client, route.trust.as_ref(), cutoff).await,
        None => {
            let target = route.raknet_target();
            let address = target.address.clone();
            endpoint::connect(&target, None, cutoff)
                .await
                .map(|transport| (transport, address))
        }
    };
    // Dropping a canceled caller never reaches this invalidation or starts a replacement attempt.
    if result.is_err() {
        probes()
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .remove(&key);
    }
    result
}

/// Tests HTTPS before HTTP under one shared probe cutoff; only a complete 2xx response answers.
async fn probe(
    client: &reqwest::Client,
    host: &str,
    port: Option<u16>,
    caller: tokio::time::Instant,
) -> Option<String> {
    // Selected Go does not successfully probe IPv6 literals. Keep that observed family limit explicit.
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        return None;
    }
    let deadline = caller.min(tokio::time::Instant::now() + PROBE_TIMEOUT);
    let probing = async {
        for scheme in ["https", "http"] {
            let candidates = match port {
                Some(port) => vec![format!("{scheme}://{host}:{port}")],
                None => vec![
                    format!(
                        "{scheme}://{host}:{}",
                        bridge::contract::DEFAULT_BEDROCK_PORT
                    ),
                    format!("{scheme}://{host}"),
                ],
            };
            for candidate in candidates {
                let request = format!(
                    "{candidate}{}",
                    nethernet::signaling::http::join::STATUS_PATH
                );
                let Ok(mut response) = client.get(request).send().await else {
                    continue;
                };
                let success = response.status().is_success();
                let complete = loop {
                    match response.chunk().await {
                        Ok(Some(_)) => {}
                        Ok(None) => break true,
                        Err(_) => break false,
                    }
                };
                if success && complete {
                    return Some(candidate);
                }
            }
        }
        None
    };
    let result = tokio::time::timeout_at(deadline, probing)
        .await
        .ok()
        .flatten();
    (tokio::time::Instant::now() < deadline)
        .then_some(result)
        .flatten()
}

/// Authorizes only an already verified Ready key, retaining its exact selected URL and original proof.
async fn connect_http(
    selected: &str,
    identity: &TransportIdentity,
    http_client: &reqwest::Client,
    trust: &dyn crate::NativeServerTrust,
    deadline: tokio::time::Instant,
) -> io::Result<(Transport, String)> {
    let url = url::Url::parse(selected).map_err(|_| invalid())?;
    let port = url.port_or_known_default().ok_or_else(invalid)?;
    let endpoint = format!(
        "{}://{}:{port}",
        url.scheme(),
        url.host_str().ok_or_else(invalid)?
    );
    // Keep explicit loopback routes local; otherwise retain Nether's original interface selection.
    let bind = url
        .host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .filter(IpAddr::is_loopback)
        .map(|ip| SocketAddr::new(ip, 0));
    let mut trusted = Vec::new();
    loop {
        check(deadline, identity.expires)?;
        let client = http::dial(&endpoint, identity, http_client.clone(), bind, deadline).await?;
        let Some(key) = client.server_public_key() else {
            drop(client.close());
            return Err(untrusted());
        };
        if !trusted.contains(key) {
            let started = Instant::now();
            let decision =
                tokio::time::timeout_at(deadline, trust.trust(selected, key, deadline.into_std()))
                    .await
                    .map_err(|_| expired())??;
            check(deadline, identity.expires)?;
            if !decision {
                drop(client.close());
                return Err(untrusted());
            }
            if started.elapsed() > TRUST_REDIAL_AFTER {
                trusted.push(*key);
                drop(client.close());
                continue;
            }
        }
        let address = format!("{endpoint}:{port}");
        return Ok((
            Transport::Http(Box::new(super::AcceptedTransport::new(
                client.into_accepted(),
            ))),
            address,
        ));
    }
}

/// Keeps original account validity separate from transport timeout classification.
fn check(deadline: tokio::time::Instant, expires: Option<Instant>) -> io::Result<()> {
    if let Some(expires) = expires {
        super::super::authentication::ensure_current(expires, Instant::now())?;
    }
    if tokio::time::Instant::now() >= deadline {
        return Err(expired());
    }
    Ok(())
}

/// Omits selected server endpoints from errors.
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "invalid native HTTP route")
}
/// Refuses the remote server without granting account authority.
fn untrusted() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "native server identity was not trusted",
    )
}
/// Keeps the original join timeout finite through discovery and user decisions.
fn expired() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "native addressed join timed out")
}
