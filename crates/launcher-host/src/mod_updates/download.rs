//! Exact-asset downloads with GitHub's digest, bounded staging and cancellation fencing.

use super::{Candidate, progress::Progress};
use anyhow::{Context, Result, ensure};
use launcher::mods::UpdatePhase;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

#[derive(Clone, Deserialize)]
pub(super) struct Asset {
    pub(super) id: u64,
    pub(super) name: String,
    pub(super) size: u64,
    pub(super) digest: Option<String>,
    /// Set only by the checked HTTPS manifest parser, never by GitHub JSON.
    #[serde(skip)]
    pub(super) download_url: Option<String>,
}

/// A verified release artifact. Dropping it removes its temporary file.
pub struct Downloaded {
    pub id: u64,
    pub version: String,
    pub source: String,
    pub asset: String,
    file: tempfile::NamedTempFile,
    generation: Arc<AtomicU64>,
    revision: u64,
}

impl Downloaded {
    /// The complete, SHA-256-verified component, awaiting host validation.
    pub fn path(&self) -> &Path {
        self.file.path()
    }

    /// Whether player preferences still authorize this download; the installer rechecks its file.
    pub fn is_current(&self) -> bool {
        self.generation.load(Ordering::SeqCst) == self.revision
    }

    /// Creates an isolated artifact for installer tests, without starting a network worker.
    #[cfg(any(test, feature = "test-support"))]
    pub fn fixture(id: u64, version: &str, repository: &str, asset: &str, bytes: &[u8]) -> Self {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(bytes).unwrap();
        Self {
            id,
            version: version.into(),
            source: repository.into(),
            asset: asset.into(),
            file,
            generation: Arc::new(AtomicU64::new(1)),
            revision: 1,
        }
    }

    /// Invalidates a fixture to exercise cancellation after download.
    #[cfg(any(test, feature = "test-support"))]
    pub fn cancel_fixture(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }
}

/// Selects exactly one named, bounded WASM asset with a usable SHA-256 digest.
pub(super) fn select<'a>(assets: &'a [Asset], name: &str, limit: usize) -> Option<&'a Asset> {
    let mut matching = assets.iter().filter(|asset| asset.name == name);
    let asset = matching.next()?;
    if matching.next().is_some()
        || asset.size == 0
        || asset.size > limit as u64
        || (asset.download_url.is_none() && !name.ends_with(".wasm"))
        || digest(asset).is_none()
    {
        return None;
    }
    Some(asset)
}

/// Decodes only GitHub's SHA-256 digest format, rejecting missing or malformed hashes.
fn digest(asset: &Asset) -> Option<[u8; 32]> {
    let hex = asset.digest.as_ref()?.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut bytes = [0; 32];
    for (byte, [high, low]) in bytes.iter_mut().zip(hex.as_bytes().as_chunks::<2>().0) {
        let high = char::from(*high).to_digit(16)?;
        let low = char::from(*low).to_digit(16)?;
        *byte = ((high << 4) | low) as u8;
    }
    Some(bytes)
}

/// Allows GitHub's asset redirect hosts, always over credential-free HTTPS.
fn trusted(url: &url::Url) -> bool {
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port_or_known_default() == Some(443)
        && matches!(
            url.host_str(),
            Some(
                "api.github.com"
                    | "github.com"
                    | "release-assets.githubusercontent.com"
                    | "objects.githubusercontent.com"
            )
        )
}

/// Builds the asset client separately from the API client, which rejects all redirects.
pub(super) fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!(
            "{}/{}",
            launcher::PRODUCT_NAME,
            launcher::PRODUCT_VERSION
        ))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() > 4 || !trusted(attempt.url()) {
                attempt.error("untrusted mod asset redirect")
            } else {
                attempt.follow()
            }
        }))
        .build()?)
}

/// Manifest downloads stay on the declared origin; HTTP redirects are never followed.
pub(super) fn manifest_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .user_agent(format!(
            "{}/{}",
            launcher::PRODUCT_NAME,
            launcher::PRODUCT_VERSION
        ))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// Library revisions cancel obsolete transfers; preference revisions also fence installation.
pub(super) struct Fence {
    pub generation: Arc<AtomicU64>,
    pub revision: u64,
    pub permission: Arc<AtomicU64>,
    pub authorization: u64,
    pub progress: Progress,
}

/// Fetches the selected immutable asset ID, never a moving `latest` URL, and verifies its digest.
pub(super) async fn fetch(
    client: &reqwest::Client,
    candidate: &Candidate,
    version: &str,
    asset: &Asset,
    fence: Fence,
    limit: usize,
) -> Result<Downloaded> {
    fence.progress.set(
        candidate.id,
        fence.revision,
        UpdatePhase::Downloading,
        0,
        asset.size,
    );
    let url = asset.download_url.clone().unwrap_or_else(|| {
        format!(
            "https://api.github.com/repos/{}/releases/assets/{}",
            candidate.source, asset.id
        )
    });
    let response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/octet-stream")
        .send()
        .await?
        .error_for_status()?;
    ensure!(response.status().is_success(), "mod download redirected");
    receive(response, candidate, version, asset, fence, limit).await
}

/// Stages one response without trusting its length, framing, or contents.
async fn receive(
    mut response: reqwest::Response,
    candidate: &Candidate,
    version: &str,
    asset: &Asset,
    fence: Fence,
    limit: usize,
) -> Result<Downloaded> {
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size == asset.size),
        "asset length changed"
    );
    let mut file = tempfile::NamedTempFile::new().context("stage mod download")?;
    let mut length = 0;
    let mut hash = Sha256::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            fence.generation.load(Ordering::SeqCst) == fence.revision,
            "mod download cancelled"
        );
        length += chunk.len();
        ensure!(
            length <= limit && length as u64 <= asset.size,
            "mod download exceeds byte limit"
        );
        hash.update(&chunk);
        file.write_all(&chunk)?;
        fence.progress.set(
            candidate.id,
            fence.revision,
            UpdatePhase::Downloading,
            length as u64,
            asset.size,
        );
    }
    ensure!(length as u64 == asset.size, "incomplete mod download");
    fence.progress.set(
        candidate.id,
        fence.revision,
        UpdatePhase::Verifying,
        length as u64,
        asset.size,
    );
    let expected = digest(asset).context("missing mod SHA-256")?;
    ensure!(
        hash.finalize().as_slice() == expected,
        "mod download SHA-256 mismatch"
    );
    Ok(Downloaded {
        id: candidate.id,
        version: version.into(),
        source: candidate.source.clone(),
        asset: asset.name.clone(),
        file,
        generation: fence.permission,
        revision: fence.authorization,
    })
}

#[cfg(test)]
mod tests;
