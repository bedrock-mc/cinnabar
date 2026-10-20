//! Bounded provider acquisition shared by actual native account owners.

use super::{NativeAuthentication, NativeCredential, NativeHostingIdentity, NativeTokenVerifier};
use crate::RawClientData;
use p384::ecdsa::SigningKey;
use provider_http::{Client, StatusCode};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use url::Url;

const KEY_REFRESH_INTERVAL: Duration = Duration::from_secs(30 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

/// Owns reusable public verification metadata alongside the concrete account operation.
/// No mutex, task, timer or runtime is created here.
pub struct NativeProvider {
    issuer: Url,
    expected_issuer: String,
    client: Client,
    configuration: Option<Configuration>,
    verifier: Option<NativeTokenVerifier>,
    fetched: Option<Instant>,
}

#[derive(Deserialize)]
struct Configuration {
    jwks_uri: String,
    #[serde(default)]
    id_token_signing_alg_values_supported: Option<Vec<String>>,
}

impl NativeProvider {
    /// Seeds only a nondefault fixture's checked public response; normal acquisition remains HTTPS-only.
    #[cfg(feature = "test-support")]
    pub(crate) fn fixture_metadata(
        issuer: &str,
        user_agent: &str,
        jwks: &[u8],
        now: Instant,
    ) -> io::Result<Self> {
        let mut provider = Self::new(issuer, user_agent)?;
        provider.verifier = Some(NativeTokenVerifier::new(
            provider.expected_issuer.clone(),
            &["RS256".into()],
            jwks,
        )?);
        provider.fetched = Some(now);
        Ok(provider)
    }

    /// Builds the existing redirect-free HTTPS client with the owning service's user agent.
    /// The caller retains this cache alongside its concrete account and original deadline.
    ///
    /// # Errors
    /// Rejects malformed provider URLs or unavailable HTTP/TLS configuration.
    pub fn new(issuer: &str, user_agent: &str) -> io::Result<Self> {
        let expected_issuer = intended_issuer(issuer)?;
        Ok(Self {
            issuer: secure_url(&expected_issuer)?,
            expected_issuer,
            client: Client::builder()
                .https_only(true)
                .redirect(provider_http::redirect::Policy::none())
                .no_gzip()
                .no_brotli()
                .no_deflate()
                .no_zstd()
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(REQUEST_TIMEOUT)
                .user_agent(user_agent)
                .build()
                .map_err(|_| failed())?,
            configuration: None,
            verifier: None,
            fetched: None,
        })
    }

    /// Prevents reuse across a different discovered environment.
    ///
    /// # Errors
    /// Rejects a malformed discovered issuer before comparing policy.
    pub fn matches(&self, issuer: &str) -> io::Result<bool> {
        Ok(self.expected_issuer == intended_issuer(issuer)?)
    }

    /// Qualifies one key-bound credential without replacing its original service-time observation.
    /// Known keys are reused; an unknown kid can refresh only after the baseline's 30-minute interval.
    ///
    /// # Errors
    /// Rejects expired, malformed, wrong-account/key or untrusted credentials and bounded HTTP failures.
    pub async fn authentication(
        &mut self,
        key: SigningKey,
        credential: NativeCredential,
        client_data: &RawValue,
        xuid: &str,
    ) -> io::Result<NativeAuthentication> {
        self.acquire_verifier(&credential).await?;
        NativeAuthentication::new(
            key,
            credential,
            client_data,
            self.verifier.as_ref().ok_or_else(failed)?,
            xuid,
            Instant::now(),
        )
    }

    /// Qualifies an accepting host's real service mint with the same provider cache and selected proof key.
    ///
    /// # Errors
    /// Rejects untrusted, expired or wrong-account/key credentials without creating player client data.
    pub async fn hosting_identity(
        &mut self,
        key: Arc<SigningKey>,
        credential: NativeCredential,
        xuid: &str,
    ) -> io::Result<NativeHostingIdentity> {
        self.acquire_verifier(&credential).await?;
        NativeHostingIdentity::new(
            key,
            credential,
            self.verifier.as_ref().ok_or_else(failed)?,
            xuid,
            Instant::now(),
        )
    }

    /// Reuses the same provider cache and original credential while preserving relay metadata values.
    /// The application owns full metadata validation before calling this credential qualifier.
    pub async fn authentication_relay(
        &mut self,
        key: SigningKey,
        credential: NativeCredential,
        client_data: RawClientData,
        xuid: &str,
    ) -> io::Result<NativeAuthentication> {
        self.acquire_verifier(&credential).await?;
        NativeAuthentication::new_relay(
            key,
            credential,
            client_data,
            self.verifier.as_ref().ok_or_else(failed)?,
            xuid,
            Instant::now(),
        )
    }

    /// Keeps the existing refresh decision, original credential check and HTTP owner in one place.
    async fn acquire_verifier(&mut self, credential: &NativeCredential) -> io::Result<()> {
        credential.ensure_current(Instant::now())?;
        let unknown = match &self.verifier {
            Some(verifier) => !verifier.recognizes(credential.token())?,
            None => true,
        };
        self.refresh_if_needed(unknown).await
    }

    /// Acquires the same bounded public peer-verifier snapshot without minting a player credential.
    pub async fn peer_verifier(&mut self) -> io::Result<NativeTokenVerifier> {
        self.refresh_if_needed(true).await?;
        self.verifier.clone().ok_or_else(failed)
    }

    /// Shares the original unknown-key refresh interval across both actual consumers.
    async fn refresh_if_needed(&mut self, unknown: bool) -> io::Result<()> {
        let now = Instant::now();
        if self.verifier.is_none()
            || (unknown
                && self
                    .fetched
                    .is_some_and(|last| now.duration_since(last) >= KEY_REFRESH_INTERVAL))
        {
            self.refresh().await?;
        }
        Ok(())
    }

    /// Acquires configuration once and atomically replaces the last successful bounded key set.
    async fn refresh(&mut self) -> io::Result<()> {
        if self.configuration.is_none() {
            let mut url = self.issuer.clone();
            url.path_segments_mut()
                .map_err(|_| failed())?
                .pop_if_empty()
                .extend([".well-known", "openid-configuration"]);
            let bytes = self.document(url).await?;
            let configuration: Configuration =
                serde_json::from_slice(&bytes).map_err(|_| failed())?;
            secure_url(&configuration.jwks_uri)?;
            self.configuration = Some(configuration);
        }
        let configuration = self
            .configuration
            .as_ref()
            .expect("acquired OpenID configuration");
        let bytes = self.document(secure_url(&configuration.jwks_uri)?).await?;
        let verifier = NativeTokenVerifier::new(
            self.expected_issuer.clone(),
            configuration
                .id_token_signing_alg_values_supported
                .as_deref()
                .unwrap_or(&[]),
            &bytes,
        )?;
        self.verifier = Some(verifier);
        self.fetched = Some(Instant::now());
        Ok(())
    }

    /// Bounds declared and streamed public metadata before deserialization; errors reveal no body or URL.
    async fn document(&self, url: Url) -> io::Result<Vec<u8>> {
        let mut response = self
            .client
            .get(url)
            .header("accept", "application/json")
            .header("content-type", "application/json")
            .send()
            .await
            .map_err(|_| failed())?;
        let limit = NativeTokenVerifier::MAX_DOCUMENT_BYTES;
        if response.status() != StatusCode::OK
            || response
                .content_length()
                .is_some_and(|length| length > limit as u64)
        {
            return Err(failed());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| failed())? {
            if chunk.len() > limit.saturating_sub(bytes.len()) {
                return Err(failed());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}

/// Matches the configured Go URL.JoinPath() path cleaning, preserving whether a trailing slash existed.
/// The expected issuer is independent of Rust's HTTP URL serialization (which adds a root slash).
fn intended_issuer(value: &str) -> io::Result<String> {
    let url = secure_url(value)?;
    let authority = value.find("://").ok_or_else(failed)? + 3;
    let path_start = value[authority..]
        .find('/')
        .map_or(value.len(), |index| authority + index);
    let path = &value[path_start..];
    let authority = format!("{}://{}", url.scheme(), &value[authority..path_start]);
    if path.is_empty() {
        return Ok(authority);
    }
    let mut segments = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(segment),
        }
    }
    let mut result = format!("{}/{}", authority, segments.join("/"));
    if path.ends_with('/') && !result.ends_with('/') {
        result.push('/');
    }
    Ok(result)
}

/// Public verification metadata never follows plaintext, credentials, fragments or query policy overrides.
fn secure_url(value: &str) -> io::Result<Url> {
    if value.len() > NativeTokenVerifier::MAX_PROVIDER_URL_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(failed());
    }
    let url = Url::parse(value).map_err(|_| failed())?;
    if url.scheme() != "https"
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.query().is_some()
    {
        return Err(failed());
    }
    Ok(url)
}

/// Keeps service and environment diagnostics out of the public authentication error surface.
fn failed() -> io::Error {
    io::Error::other("native multiplayer provider acquisition failed; retry or sign in again")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Prevents HTTP URL canonicalization from silently changing the configured JWT issuer policy.
    #[test]
    fn native_provider_preserves_configured_issuer_path_and_trailing_slash() {
        for (configured, expected) in [
            ("https://issuer.example", "https://issuer.example"),
            ("https://issuer.example/", "https://issuer.example/"),
            (
                "https://issuer.example/tenant",
                "https://issuer.example/tenant",
            ),
            (
                "https://issuer.example/tenant/",
                "https://issuer.example/tenant/",
            ),
            (
                "https://issuer.example/a//b/../tenant/",
                "https://issuer.example/a/tenant/",
            ),
        ] {
            assert_eq!(intended_issuer(configured).unwrap(), expected);
        }
    }
}
