//! Signed release manifests: the published format, its Ed25519 signing and verification, and the
//! version check that decides whether a newer build exists.

use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::STANDARD};
#[cfg(not(target_arch = "wasm32"))]
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

pub mod build;
pub mod channel;
pub mod platforms;
pub mod runtime;
pub mod select;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
mod wire;
pub use platforms::platform_key;

/// Largest served envelope accepted.
pub const MAX_ENVELOPE_BYTES: usize = 1 << 20;
/// The only manifest schema this build understands.
pub const SCHEMA: i64 = 1;
/// Ed25519 public key and private seed length.
pub const KEY_BYTES: usize = 32;

/// Trusted public keys by key ID.
pub type TrustedKeys = BTreeMap<String, [u8; KEY_BYTES]>;

/// Why a manifest was rejected or could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("decode envelope: {0}")]
    Envelope(serde_json::Error),
    #[error("unknown signing key {0:?}")]
    UnknownKey(String),
    #[error("decode manifest: invalid base64")]
    ManifestEncoding,
    #[error("manifest signature is invalid")]
    Signature,
    #[error("decode signed manifest: {0}")]
    Manifest(serde_json::Error),
    #[error("unsupported manifest schema {0}")]
    Schema(i64),
    #[error("manifest has expired")]
    Expired,
    #[error("manifest channel {found:?} does not match {expected:?}")]
    Channel { found: String, expected: String },
    #[error("invalid version {0:?}")]
    Version(String),
    #[error("artifact URL {0:?} is not HTTPS")]
    ArtifactUrl(String),
    #[error("artifact sha256 is malformed")]
    ArtifactDigest,
    #[error("artifact size must be positive")]
    ArtifactSize,
    #[error("invalid trusted key entry {0:?}")]
    TrustedKey(String),
    #[error("invalid signing key")]
    SigningKey,
    #[error("encode manifest: {0}")]
    Encode(serde_json::Error),
}

/// The served document: a base64 manifest plus a detached Ed25519 signature over its bytes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Envelope {
    #[serde(default)]
    pub key_id: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub manifest: String,
}

/// One downloadable build for a platform key such as `macos-arm64`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Artifact {
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size: i64,
}

/// The newest build on a channel.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct Manifest {
    #[serde(default)]
    pub schema: i64,
    #[serde(default)]
    pub channel: String,
    #[serde(default)]
    pub version: String,
    /// Monotonic publication number for builds with the same app version.
    #[serde(default)]
    pub build_number: u64,
    #[serde(with = "wire::rfc3339")]
    pub expires: OffsetDateTime,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes_url: String,
    #[serde(default, deserialize_with = "wire::null_as_empty")]
    pub artifacts: BTreeMap<String, Artifact>,
}

/// Outcome of a check; `artifact` is set only when `available`.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct Verdict {
    pub available: bool,
    pub current: String,
    pub latest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<Artifact>,
}

/// The signed, unexpired manifest in `envelope_json` when a trusted key signed it.
#[cfg(not(target_arch = "wasm32"))]
pub fn verify(
    envelope_json: &[u8],
    keys: &TrustedKeys,
    now: OffsetDateTime,
) -> Result<Manifest, Error> {
    let envelope: Envelope = serde_json::from_slice(envelope_json).map_err(Error::Envelope)?;
    let key = keys
        .get(&envelope.key_id)
        .ok_or_else(|| Error::UnknownKey(envelope.key_id.clone()))?;
    let payload = STANDARD
        .decode(&envelope.manifest)
        .map_err(|_| Error::ManifestEncoding)?;
    let signature = STANDARD
        .decode(&envelope.signature)
        .map_err(|_| Error::Signature)?;
    UnparsedPublicKey::new(&ED25519, key)
        .verify(&payload, &signature)
        .map_err(|_| Error::Signature)?;
    let manifest: Manifest = serde_json::from_slice(&payload).map_err(Error::Manifest)?;
    if manifest.schema != SCHEMA {
        return Err(Error::Schema(manifest.schema));
    }
    if manifest.expires <= now {
        return Err(Error::Expired);
    }
    Ok(manifest)
}

/// Build number baked into packaged clients; old or developer builds have no publication number.
pub fn build_number() -> u64 {
    option_env!("CINNABAR_BUILD_NUMBER")
        .or(option_env!("GITHUB_RUN_NUMBER"))
        .and_then(|number| number.parse().ok())
        .unwrap_or(0)
}

/// Rejects an artifact that is not an HTTPS download with a SHA-256 digest and a positive size.
pub fn validate_artifact(artifact: &Artifact) -> Result<(), Error> {
    let https = url::Url::parse(&artifact.url)
        .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some_and(|h| !h.is_empty()));
    if !https {
        return Err(Error::ArtifactUrl(artifact.url.clone()));
    }
    if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::ArtifactDigest);
    }
    if artifact.size <= 0 {
        return Err(Error::ArtifactSize);
    }
    Ok(())
}

/// Decodes an `id:base64,id:base64` trusted-key list as baked in at build time.
pub fn parse_keys(list: &str) -> Result<TrustedKeys, Error> {
    let mut keys = TrustedKeys::new();
    for entry in list
        .split(',')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
    {
        let (id, encoded) = entry.split_once(':').unwrap_or((entry, ""));
        let key = STANDARD
            .decode(encoded)
            .ok()
            .and_then(|raw| <[u8; KEY_BYTES]>::try_from(raw).ok())
            .filter(|_| !id.is_empty())
            .ok_or_else(|| Error::TrustedKey(id.to_owned()))?;
        keys.insert(id.to_owned(), key);
    }
    Ok(keys)
}

/// Serializes `manifest` and wraps it in an envelope signed by the key derived from `seed`.
#[cfg(not(target_arch = "wasm32"))]
pub fn sign(manifest: &Manifest, key_id: &str, seed: &[u8]) -> Result<Vec<u8>, Error> {
    let pair = key_pair(seed)?;
    let payload = serde_json::to_vec(manifest).map_err(Error::Encode)?;
    serde_json::to_vec(&Envelope {
        key_id: key_id.to_owned(),
        signature: STANDARD.encode(pair.sign(&payload)),
        manifest: STANDARD.encode(&payload),
    })
    .map_err(Error::Encode)
}

/// A fresh random signing seed.
#[cfg(not(target_arch = "wasm32"))]
pub fn generate_seed() -> Result<[u8; KEY_BYTES], Error> {
    let mut seed = [0; KEY_BYTES];
    SystemRandom::new()
        .fill(&mut seed)
        .map_err(|_| Error::SigningKey)?;
    Ok(seed)
}

/// The public key a trusted-key list carries for `seed`.
#[cfg(not(target_arch = "wasm32"))]
pub fn public_key(seed: &[u8]) -> Result<[u8; KEY_BYTES], Error> {
    let pair = key_pair(seed)?;
    <[u8; KEY_BYTES]>::try_from(pair.public_key().as_ref()).map_err(|_| Error::SigningKey)
}

#[cfg(not(target_arch = "wasm32"))]
fn key_pair(seed: &[u8]) -> Result<Ed25519KeyPair, Error> {
    if seed.len() != KEY_BYTES {
        return Err(Error::SigningKey);
    }
    Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| Error::SigningKey)
}
