//! Publisher-side validation before a local server offers an archive.

use std::collections::BTreeSet;

use anyhow::{Result, ensure};
use serde::Serialize;

use super::{MANIFEST_PATH, VerifiedBundle, directory, read_entry};
use crate::{
    crypto::{self, SignedDocument},
    manifest::{Manifest, PackageOffer, Scope, identifier},
    policy::{MAX_BUNDLE_BYTES, MAX_EXPANDED_BYTES, MAX_MARKER_BYTES},
};

#[derive(Debug, Serialize)]
pub struct Metadata {
    pub sha256: String,
    pub bytes: u64,
    pub manifest: Manifest,
}

/// Verifies an archive under its publisher's declared identity and binds metadata to its bytes.
pub fn verify(bytes: &[u8]) -> Result<Metadata> {
    ensure!(
        !bytes.is_empty() && bytes.len() <= MAX_BUNDLE_BYTES,
        "invalid bundle size"
    );
    let entries = directory::validate(bytes)?;
    let size = entries
        .get(MANIFEST_PATH)
        .ok_or_else(|| anyhow::anyhow!("manifest missing"))?
        .size;
    ensure!(size <= MAX_MARKER_BYTES as u64, "manifest too large");
    let mut remaining = MAX_EXPANDED_BYTES;
    let signed = read_entry(&entries, MANIFEST_PATH, size, &mut remaining)?;
    let signed: SignedDocument = serde_json::from_slice(&signed)?;
    let payload = crypto::unhex(&signed.payload)?;
    let manifest: Manifest = serde_json::from_slice(&payload)?;
    ensure!(identifier(&manifest.id), "invalid package identity");
    let offer = PackageOffer {
        id: manifest.id,
        publisher_key: manifest.publisher_key,
        digest: crypto::digest(bytes),
        bytes: bytes.len() as u64,
        url: String::new(),
    };
    let scope = Scope {
        permissions: manifest.permissions,
        origins: BTreeSet::new(),
        memory_bytes: 0,
        gpu_bytes: 0,
        personal_mods: Default::default(),
    };
    let verified = VerifiedBundle::read(bytes, &offer, &scope, MAX_EXPANDED_BYTES)?;
    Ok(Metadata {
        sha256: offer.digest,
        bytes: offer.bytes,
        manifest: verified.manifest,
    })
}
