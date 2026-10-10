//! The one bounded remote image cache: server form buttons stay in memory, while launcher and
//! Marketplace artwork persist on disk. Every surface downloads through the same policy-checked
//! client and bounded body reader.

use std::{net::SocketAddr, sync::Arc, time::Duration};

use reqwest::{StatusCode, redirect::Policy};
use server_experience::fetch::public_address;
use url::{Host, Url};

mod disk;
mod memory;
pub use disk::ImageDirectory;
#[cfg(test)]
pub(crate) use memory::tests;
pub(crate) use memory::{RemoteImages, RemoteState, is_remote};

/// Download and storage limits of one image surface.
#[derive(Clone, Copy, Debug)]
pub struct Surface {
    pub max_bytes: usize,
    /// Files a disk directory keeps; the least recently used go first.
    pub max_files: usize,
    /// Directory byte bound; zero leaves only the file count.
    pub max_dir_bytes: u64,
    /// Longest accepted URL; zero leaves it unbounded.
    pub max_url_bytes: usize,
    pub max_redirects: usize,
    pub timeout: Duration,
    pub user_agent: Option<&'static str>,
    /// Fixed file extension; `None` names files by their image signature.
    pub extension: Option<&'static str>,
    /// HTTPS only, without embedded credentials.
    pub https_only: bool,
    /// Public addresses only: hosts must resolve, and IP literals must be, outside local networks.
    pub public_hosts: bool,
}

/// Server form button images, as the vanilla client fetches them.
pub const FORM_IMAGES: Surface = Surface {
    max_bytes: 2 * 1024 * 1024,
    max_files: 0,
    max_dir_bytes: 0,
    max_url_bytes: 0,
    max_redirects: 9,
    timeout: Duration::from_secs(10),
    user_agent: None,
    extension: None,
    https_only: false,
    public_hosts: false,
};

/// Featured, Home, Profile and friends artwork from the account's services.
pub const LAUNCHER_ART: Surface = Surface {
    max_bytes: launcher::accounts::MAX_ARTWORK_BYTES,
    max_files: 256,
    max_dir_bytes: 0,
    max_url_bytes: 0,
    max_redirects: 9,
    timeout: Duration::from_secs(8),
    user_agent: Some("Cinnabar/1.0"),
    extension: Some(".img"),
    https_only: true,
    public_hosts: true,
};

/// Marketplace offer thumbnails and key art.
pub const STORE_ART: Surface = Surface {
    max_bytes: 4 << 20,
    max_files: 512,
    max_dir_bytes: 256 << 20,
    max_url_bytes: 1024,
    max_redirects: 3,
    timeout: Duration::from_secs(20),
    user_agent: Some("libhttpclient/1.0.0.0"),
    extension: None,
    https_only: true,
    public_hosts: true,
};

impl Surface {
    /// Whether this surface may request `raw`; redirects are held to the same rule.
    pub fn accepts(&self, raw: &str) -> bool {
        if self.max_url_bytes > 0 && raw.len() > self.max_url_bytes {
            return false;
        }
        let Ok(url) = Url::parse(raw) else {
            return false;
        };
        let scheme = if self.https_only {
            url.scheme() == "https" && url.username().is_empty() && url.password().is_none()
        } else {
            matches!(url.scheme(), "http" | "https")
        };
        let host = match url.host() {
            Some(Host::Domain(domain)) => !domain.is_empty(),
            Some(Host::Ipv4(ip)) => !self.public_hosts || public_address(ip.into()),
            Some(Host::Ipv6(ip)) => !self.public_hosts || public_address(ip.into()),
            None => false,
        };
        scheme && host
    }
}

type ResolveError = Box<dyn std::error::Error + Send + Sync>;

/// Resolves only hosts whose every address is public, so DNS cannot point a download inward.
struct PublicResolver;

impl reqwest::dns::Resolve for PublicResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_owned();
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|error| -> ResolveError { Box::new(error) })?
                .collect();
            if addresses.is_empty() || !addresses.iter().all(|address| public_address(address.ip()))
            {
                return Err(ResolveError::from("image host is not public"));
            }
            Ok::<reqwest::dns::Addrs, ResolveError>(Box::new(addresses.into_iter()))
        })
    }
}

/// A client enforcing `surface`'s timeout, agent, redirect bound and address policy.
pub(crate) fn client(surface: &Surface) -> Option<reqwest::Client> {
    client_with(surface, Arc::new(PublicResolver))
}

/// [`client`] with the resolver public hosts go through; tests substitute one.
fn client_with<R: reqwest::dns::Resolve + 'static>(
    surface: &Surface,
    resolver: Arc<R>,
) -> Option<reqwest::Client> {
    let checked = *surface;
    let mut builder = reqwest::Client::builder()
        .timeout(surface.timeout)
        .redirect(Policy::custom(move |attempt| {
            if attempt.previous().len() > checked.max_redirects
                || !checked.accepts(attempt.url().as_str())
            {
                attempt.error("image redirect rejected")
            } else {
                attempt.follow()
            }
        }));
    if let Some(agent) = surface.user_agent {
        builder = builder.user_agent(agent);
    }
    if surface.public_hosts {
        // A proxy would resolve the host itself, past the public-address check.
        builder = builder.dns_resolver(resolver).no_proxy();
    }
    builder.build().ok()
}

/// The body of `url` within `surface`'s byte limit; `only_ok` refuses any status but 200.
pub(crate) async fn download(
    client: &reqwest::Client,
    surface: &Surface,
    url: &str,
    only_ok: bool,
) -> Option<Vec<u8>> {
    if !surface.accepts(url) {
        return None;
    }
    let mut response = client.get(url).send().await.ok()?;
    let status = response.status();
    if status != StatusCode::OK && (only_ok || !status.is_success()) {
        return None;
    }
    if response
        .content_length()
        .is_some_and(|length| length > surface.max_bytes as u64)
    {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        if body.len() + chunk.len() > surface.max_bytes {
            return None;
        }
        body.extend_from_slice(&chunk);
    }
    Some(body)
}

/// The extension of a PNG, JPEG, GIF or BMP by its signature.
pub(crate) fn image_extension(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(".png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some(".jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some(".gif")
    } else if bytes.starts_with(b"BM") {
        Some(".bmp")
    } else {
        None
    }
}
