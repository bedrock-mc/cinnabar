//! Fixed desktop or public-only HTTP policy at the original native pack acquisition edge.

use super::invalid;
use bridge::{contract::GATEWAY_EGRESS_CANDIDATES, gateway_egress::public_gateway_upstream};
use reqwest::{Url, dns};
use std::{io, net::SocketAddr, sync::Arc, time::Duration};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(45);
const MAX_REDIRECTS: usize = 5;

/// A fixed policy retained beside the actual reusable reqwest client, without another I/O owner.
#[derive(Clone)]
pub(in crate::login::native_network::packs) struct Client {
    client: reqwest::Client,
    policy: Policy,
}

#[derive(Clone, Copy)]
enum Policy {
    Desktop,
    Public(Public),
}

impl Client {
    /// Validates the actual original URL before constructing its request; redirects repeat this check.
    pub(in crate::login::native_network::packs) fn get(
        &self,
        url: &str,
    ) -> io::Result<reqwest::RequestBuilder> {
        let url = Url::parse(url).map_err(|_| invalid("invalid pack CDN URL"))?;
        validate_url(&url)?;
        if let Policy::Public(policy) = self.policy {
            policy.admit(&url)?;
        }
        Ok(self.client.get(url))
    }
}

/// Preserves the desktop's original TLS, ambient proxy, redirect and timeout behavior.
pub(in crate::login::native_network::packs) fn http_client() -> io::Result<Client> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))
        .build()
        .map_err(|_| io::Error::other("pack HTTP client could not initialize"))?;
    Ok(Client {
        client,
        policy: Policy::Desktop,
    })
}

/// Connects only to admitted actual numeric candidates while retaining original verified TLS names.
pub(in crate::login::native_network::packs) fn public_http_client() -> io::Result<Client> {
    let policy = Public {
        #[cfg(test)]
        fixture: super::super::PUBLIC_CDN_ENDPOINT
            .try_with(|address| *address)
            .ok(),
    };
    let roots = rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let tls = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .map_err(|_| invalid("pack HTTP TLS could not initialize"))?
    .with_root_certificates(roots)
    .with_no_client_auth();
    let redirects = reqwest::redirect::Policy::limited(MAX_REDIRECTS);
    let client = reqwest::Client::builder()
        .tls_backend_preconfigured(tls)
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(REQUEST_TIMEOUT)
        .no_proxy()
        .dns_resolver(PublicResolver)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if policy.admit(attempt.url()).is_err() {
                attempt.error("unsupported pack CDN redirect")
            } else {
                redirects.redirect(attempt)
            }
        }))
        .build()
        .map_err(|_| invalid("pack HTTP client could not initialize"))?;
    Ok(Client {
        client,
        policy: Policy::Public(policy),
    })
}

/// Public policy has no configurable production bypass; a fixture may capture one exact tuple.
#[derive(Clone, Copy)]
struct Public {
    #[cfg(test)]
    fixture: Option<SocketAddr>,
}

impl Public {
    /// Checks literals here because the connector does not call a DNS resolver for numeric URLs.
    fn admit(self, url: &Url) -> io::Result<()> {
        validate_url(url)?;
        let port = url
            .port_or_known_default()
            .filter(|port| *port != 0)
            .ok_or_else(|| invalid("invalid pack CDN port"))?;
        let address = match url.host() {
            Some(url::Host::Ipv4(address)) => Some(address.into()),
            Some(url::Host::Ipv6(address)) => Some(address.into()),
            Some(url::Host::Domain(_)) => None,
            None => return Err(invalid("invalid pack CDN host")),
        };
        if let Some(address) = address {
            let address = SocketAddr::new(address, port);
            #[cfg(test)]
            if self.fixture == Some(address) {
                return Ok(());
            }
            if !public_gateway_upstream(address.ip()) {
                return Err(invalid("unsupported pack CDN address"));
            }
        }
        Ok(())
    }
}

/// Retains the original desktop URL restrictions for both policies, without exposing signed URLs.
fn validate_url(url: &Url) -> io::Result<()> {
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(invalid("unsupported pack CDN URL"));
    }
    Ok(())
}

/// Returns the exact filtered lookup result to reqwest's connector, without a second DNS lookup.
struct PublicResolver;

impl dns::Resolve for PublicResolver {
    fn resolve(&self, name: dns::Name) -> dns::Resolving {
        Box::pin(async move {
            let addresses = tokio::net::lookup_host((name.as_str(), 0))
                .await
                .map_err(|_| invalid("pack CDN resolution failed"))?;
            let addresses = public_candidates(addresses);
            if addresses.is_empty() {
                return Err(invalid("unsupported pack CDN addresses").into());
            }
            Ok(Box::new(addresses.into_iter()) as dns::Addrs)
        })
    }
}

/// Bounds the original candidate prefix and preserves usable alternatives in resolver order.
fn public_candidates(addresses: impl Iterator<Item = SocketAddr>) -> Vec<SocketAddr> {
    addresses
        .take(GATEWAY_EGRESS_CANDIDATES)
        .filter(|address| {
            !matches!(address, SocketAddr::V6(address) if address.scope_id() != 0)
                && public_gateway_upstream(address.ip())
        })
        .collect()
}

#[cfg(test)]
mod tests;
