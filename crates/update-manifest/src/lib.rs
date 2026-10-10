//! Signed release manifests: the published format, its Ed25519 signing and verification, and the
//! version check that decides whether a newer build exists.

use std::collections::BTreeMap;

use base64::{Engine, engine::general_purpose::STANDARD};
use ring::{
    rand::{SecureRandom, SystemRandom},
    signature::{ED25519, Ed25519KeyPair, KeyPair, UnparsedPublicKey},
};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

#[cfg(test)]
mod tests;
mod wire;

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

/// Verifies `envelope_json` against the clock and evaluates it for the running build.
pub fn check(
    envelope_json: &[u8],
    keys: &TrustedKeys,
    channel: &str,
    platform: &str,
    current: &str,
) -> Result<Verdict, Error> {
    let manifest = verify(envelope_json, keys, OffsetDateTime::now_utc())?;
    evaluate(&manifest, channel, platform, current)
}

/// Compares a verified manifest with the running build; a missing platform is not an error.
pub fn evaluate(
    manifest: &Manifest,
    channel: &str,
    platform: &str,
    current: &str,
) -> Result<Verdict, Error> {
    if manifest.channel != channel {
        return Err(Error::Channel {
            found: manifest.channel.clone(),
            expected: channel.to_owned(),
        });
    }
    let mut verdict = Verdict {
        current: current.to_owned(),
        latest: manifest.version.clone(),
        notes_url: (!manifest.notes_url.is_empty()).then(|| manifest.notes_url.clone()),
        ..Verdict::default()
    };
    if !newer(&manifest.version, current)? {
        return Ok(verdict);
    }
    let Some(artifact) = manifest.artifacts.get(platform) else {
        return Ok(verdict);
    };
    validate_artifact(artifact)?;
    verdict.available = true;
    verdict.artifact = Some(artifact.clone());
    Ok(verdict)
}

/// Whether `candidate` is a higher dotted-numeric version than `current`; a pre-release sorts lower.
pub fn newer(candidate: &str, current: &str) -> Result<bool, Error> {
    let (candidate, current) = (Version::parse(candidate)?, Version::parse(current)?);
    if candidate.parts != current.parts {
        return Ok(candidate.parts > current.parts);
    }
    Ok(!current.pre.is_empty() && candidate.pre.is_empty())
}

struct Version<'a> {
    parts: [u32; 3],
    pre: &'a str,
}

impl<'a> Version<'a> {
    /// `[v]MAJOR.MINOR.PATCH[-pre]` with plain decimal fields.
    fn parse(raw: &'a str) -> Result<Self, Error> {
        let invalid = || Error::Version(raw.to_owned());
        let trimmed = raw.strip_prefix('v').unwrap_or(raw);
        let (core, pre) = trimmed.split_once('-').unwrap_or((trimmed, ""));
        let mut fields = core.split('.');
        let mut parts = [0; 3];
        for part in &mut parts {
            let field = fields.next().ok_or_else(invalid)?;
            if field.is_empty() || !field.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(invalid());
            }
            *part = field.parse().map_err(|_| invalid())?;
        }
        if fields.next().is_some() {
            return Err(invalid());
        }
        Ok(Self { parts, pre })
    }
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
pub fn generate_seed() -> Result<[u8; KEY_BYTES], Error> {
    let mut seed = [0; KEY_BYTES];
    SystemRandom::new()
        .fill(&mut seed)
        .map_err(|_| Error::SigningKey)?;
    Ok(seed)
}

/// The public key a trusted-key list carries for `seed`.
pub fn public_key(seed: &[u8]) -> Result<[u8; KEY_BYTES], Error> {
    let pair = key_pair(seed)?;
    <[u8; KEY_BYTES]>::try_from(pair.public_key().as_ref()).map_err(|_| Error::SigningKey)
}

fn key_pair(seed: &[u8]) -> Result<Ed25519KeyPair, Error> {
    if seed.len() != KEY_BYTES {
        return Err(Error::SigningKey);
    }
    Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_| Error::SigningKey)
}
