//! Exact addressed-server selection and the application's verified-key trust decision.

use super::{NativeConnectionTarget, NativeTransport};
use futures::future::BoxFuture;
use p384::ecdsa::VerifyingKey;
use std::{fmt, io, sync::Arc, time::Instant};

/// Application-owned first-use authorization after the actual transport verifies its server key.
/// This decision neither supplies account credentials nor replaces SDP/DTLS authentication.
pub trait NativeServerTrust: Send + Sync {
    /// Decides once under the original join cutoff; cancellation must withdraw this join's prompt.
    fn trust<'a>(
        &'a self,
        endpoint: &'a str,
        public_key: &'a VerifyingKey,
        deadline: Instant,
    ) -> BoxFuture<'a, io::Result<bool>>;
}

#[derive(Clone)]
pub(crate) struct AddressedRoute {
    address: String,
    pub(crate) host: String,
    pub(crate) port: Option<u16>,
    pub(crate) ipv6: bool,
    pub(crate) trust: Arc<dyn NativeServerTrust>,
}

impl fmt::Debug for AddressedRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AddressedRoute(..)")
    }
}

impl NativeConnectionTarget {
    /// Binds one addressed endpoint to its original application's trust capability before any I/O.
    ///
    /// # Errors
    /// Rejects empty, malformed, URL or service input; a bare server host may omit its port.
    pub fn addressed(address: String, trust: Arc<dyn NativeServerTrust>) -> io::Result<Self> {
        if address.is_empty()
            || address.contains(['/', '\\', '?', '#', '@'])
            || address
                .chars()
                .any(|value| value.is_whitespace() || value.is_control())
        {
            return Err(invalid());
        }
        let (host, explicit, ipv6) = if let Some((host, port)) = ipv6_authority(&address)? {
            (host.to_owned(), port, true)
        } else {
            let url = url::Url::parse(&format!("http://{address}")).map_err(|_| invalid())?;
            (
                url.host().ok_or_else(invalid)?.to_string(),
                address.rsplit_once(':').map(|(_, port)| port),
                false,
            )
        };
        let port = explicit
            .map(str::parse::<u16>)
            .transpose()
            .map_err(|_| invalid())?
            .filter(|port| *port != 0);
        Ok(Self {
            address: address.clone(),
            transport: NativeTransport::Addressed,
            local_lan: None,
            addressed: Some(AddressedRoute {
                address,
                host,
                port,
                ipv6,
                trust,
            }),
        })
    }

    /// Rejects changed public projections before credentials, discovery or trust can be used.
    pub(crate) fn addressed_route(&self) -> io::Result<&AddressedRoute> {
        match &self.addressed {
            Some(route)
                if self.transport == NativeTransport::Addressed
                    && self.address == route.address =>
            {
                Ok(route)
            }
            _ => Err(invalid()),
        }
    }
}

impl AddressedRoute {
    /// Builds the original explicit RakNet fallback without resolving or changing the route.
    pub(crate) fn raknet_target(&self) -> NativeConnectionTarget {
        let port = self.port.unwrap_or(bridge::contract::DEFAULT_BEDROCK_PORT);
        let address = if self.ipv6 {
            format!("[{}]:{port}", self.host)
        } else {
            format!("{}:{port}", self.host)
        };
        NativeConnectionTarget::new(address, NativeTransport::RakNet)
    }
}

/// Retains a numeric IPv6 host's optional interface zone independently of HTTP URL syntax.
fn ipv6_authority(address: &str) -> io::Result<Option<(&str, Option<&str>)>> {
    if let Some(bracketed) = address.strip_prefix('[') {
        let (host, suffix) = bracketed.split_once(']').ok_or_else(invalid)?;
        if !ipv6_host(host) {
            return Err(invalid());
        }
        let port = if suffix.is_empty() {
            None
        } else {
            Some(suffix.strip_prefix(':').ok_or_else(invalid)?)
        };
        return Ok(Some((host, port)));
    }
    Ok(ipv6_host(address).then_some((address, None)))
}

/// Validates the numeric address while leaving a nonempty zone for the existing socket resolver.
fn ipv6_host(host: &str) -> bool {
    let (address, valid_zone) = match host.rsplit_once('%') {
        Some((address, zone)) => (address, !zone.is_empty() && !zone.contains(['[', ']'])),
        None => (host, true),
    };
    valid_zone && address.parse::<std::net::Ipv6Addr>().is_ok()
}

/// Keeps endpoint and policy values out of diagnostics.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid native addressed route",
    )
}
