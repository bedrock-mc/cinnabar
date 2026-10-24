//! Verification of exact server bytes; transport hints never authorize installation.

use anyhow::{Result, ensure};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use bridge::contract::CinnabarModEntry;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// An operator-pinned key set. A server response cannot establish its own trust root.
pub struct Keys(Vec<Vec<u8>>);

impl Keys {
    /// Reads raw 32-byte Ed25519 public keys encoded as base64; empty sets keep installs disabled.
    pub fn parse(json: &[u8]) -> Result<Self> {
        #[derive(Deserialize)]
        struct Config {
            ed25519: Vec<String>,
        }
        let config: Config = serde_json::from_slice(json)?;
        ensure!(
            !config.ed25519.is_empty() && config.ed25519.len() <= 8,
            "Marketplace signing keys have not been configured."
        );
        let keys = config
            .ed25519
            .iter()
            .map(|key| {
                let key = STANDARD.decode(key)?;
                ensure!(key.len() == 32, "Invalid marketplace public key.");
                Ok(key)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self(keys))
    }

    /// Checks a signature over the original UTF-8 bytes, without JSON reserialization.
    fn verify(&self, text: &[u8], signature: &str) -> Result<()> {
        let signature = URL_SAFE_NO_PAD.decode(signature)?;
        ensure!(
            self.0
                .iter()
                .any(|key| UnparsedPublicKey::new(&ED25519, key)
                    .verify(text, &signature)
                    .is_ok()),
            "Marketplace signature verification failed."
        );
        Ok(())
    }
}

/// The server's original canonical statement and its detached signature, persisted together.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SignedRelease {
    pub statement: String,
    pub signature: String,
}

/// Signed fields used for local admission; additional signed presentation fields stay opaque.
#[derive(Clone, Debug, Deserialize)]
pub struct Manifest {
    pub slug: String,
    pub version: String,
    pub sha256: String,
    pub bytes: u64,
    pub grants: serde_json::Value,
    pub approved_at: u64,
    pub client_revision: String,
    pub policy: String,
    pub publication: String,
}

impl SignedRelease {
    /// Binds a verified release to this explicit library selection and the caller's source ceiling.
    pub fn verify(
        &self,
        keys: &Keys,
        selected: &CinnabarModEntry,
        limit: usize,
        now: u64,
    ) -> Result<Manifest> {
        keys.verify(self.statement.as_bytes(), &self.signature)?;
        let manifest: Manifest = serde_json::from_str(&self.statement)?;
        ensure!(
            manifest.slug == selected.slug
                && manifest.version == selected.version
                && manifest.sha256 == selected.sha256
                && manifest.bytes == selected.bytes,
            "The signed release does not match the selected mod."
        );
        ensure!(
            digest_valid(&manifest.sha256) && manifest.bytes > 0 && manifest.bytes <= limit as u64,
            "Invalid marketplace artifact size or digest."
        );
        ensure!(
            manifest.approved_at <= now.saturating_add(CLOCK_SKEW_MS)
                && manifest.client_revision.len() == 40
                && manifest
                    .client_revision
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit())
                && !manifest.policy.is_empty()
                && matches!(manifest.publication.as_str(), "manual" | "automatic"),
            "Unsupported marketplace release evidence."
        );
        Ok(manifest)
    }
}

pub(super) const CLOCK_SKEW_MS: u64 = 5 * 60_000;
const MAX_REVOCATION_LIFETIME_MS: u64 = 60 * 60_000;

/// Signed withdrawal evidence; expiry is never extended by a failed refresh or restart.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SignedRevocations {
    pub body: String,
    pub signature: String,
}

#[derive(Deserialize)]
struct Revocations {
    issued_at: u64,
    expires_at: u64,
    revoked: Vec<Revoked>,
}
#[derive(Deserialize)]
struct Revoked {
    sha256: String,
    #[serde(default)]
    reason: String,
}

/// Verified revocation checkpoint shared with runtime admission.
pub struct RevocationState {
    pub issued_at: u64,
    pub expires_at: u64,
    pub revoked: HashMap<String, String>,
}

impl SignedRevocations {
    /// Rejects expired, future-dated and rollback snapshots before exposing their hashes.
    pub fn verify(&self, keys: &Keys, now: u64, previous: u64) -> Result<RevocationState> {
        self.check(keys, now, previous, true)
    }

    /// Retains authenticated rollback and withdrawal history across restarts, even after expiry.
    pub fn restore(&self, keys: &Keys, now: u64) -> Result<RevocationState> {
        self.check(keys, now, 0, false)
    }

    /// Checks framing, signature and clock bounds before applying the caller's freshness requirement.
    fn check(&self, keys: &Keys, now: u64, previous: u64, fresh: bool) -> Result<RevocationState> {
        keys.verify(self.body.as_bytes(), &self.signature)?;
        let value: Revocations = serde_json::from_str(&self.body)?;
        ensure!(
            value.issued_at >= previous
                && value.issued_at <= now.saturating_add(CLOCK_SKEW_MS)
                && (!fresh || value.expires_at > now)
                && value.expires_at > value.issued_at
                && value.expires_at - value.issued_at <= MAX_REVOCATION_LIFETIME_MS,
            "Marketplace revocation evidence is stale or rolled back."
        );
        ensure!(
            value
                .revoked
                .iter()
                .all(|entry| digest_valid(&entry.sha256)),
            "Malformed revocation digest."
        );
        Ok(RevocationState {
            issued_at: value.issued_at,
            expires_at: value.expires_at,
            revoked: value
                .revoked
                .into_iter()
                .map(|entry| (entry.sha256, entry.reason))
                .collect(),
        })
    }
}

/// Checks digest framing before it becomes a storage or URL selector.
pub fn digest_valid(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

/// Rechecks bytes after staging and immediately before atomic publication.
pub fn verify_bytes(bytes: &[u8], manifest: &Manifest) -> Result<()> {
    ensure!(
        bytes.len() as u64 == manifest.bytes
            && format!("{:x}", Sha256::digest(bytes)) == manifest.sha256,
        "Marketplace artifact hash or size mismatch."
    );
    Ok(())
}
