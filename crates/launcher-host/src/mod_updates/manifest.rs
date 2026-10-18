//! A small HTTPS version/checksum manifest, independent of any release hosting service.

use super::{download::Asset, worker::Release};
use anyhow::{Result, ensure};
use serde::Deserialize;
use ui::mod_metadata::{MAX_VERSION_BYTES, ModUpdateUrl};

const MAX_MANIFEST_BYTES: usize = 16 * 1024;
pub const FORMAT: &str = "cinnabar-mod-update-v1";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: String,
    url: String,
    sha256: String,
    size: u64,
}

/// Validates metadata before a versioned WASM URL can reach the downloader.
pub(super) fn parse(bytes: &[u8], source: &url::Url) -> Result<Release> {
    ensure!(
        bytes.len() <= MAX_MANIFEST_BYTES,
        "mod update manifest exceeds byte limit"
    );
    let manifest: Manifest = serde_json::from_slice(bytes)?;
    ensure!(
        manifest.format == FORMAT,
        "unsupported mod update manifest format"
    );
    ensure!(
        manifest.version.len() <= MAX_VERSION_BYTES
            && update_manifest::select::newer(&manifest.version, &manifest.version).is_ok(),
        "invalid mod update version"
    );
    ensure!(manifest.size > 0, "empty mod update payload");
    ensure!(
        manifest.sha256.len() == 64
            && manifest
                .sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "invalid mod update SHA-256"
    );
    ensure!(
        ModUpdateUrl::parse(&manifest.url).is_some(),
        "mod payload must use credential-free HTTPS"
    );
    let payload = url::Url::parse(&manifest.url)?;
    ensure!(
        payload.origin() == source.origin() && payload.path().ends_with(".wasm"),
        "mod payload must be a same-origin WASM URL"
    );
    Ok(Release {
        tag_name: manifest.version,
        draft: false,
        prerelease: false,
        manifest_url: Some(source.as_str().into()),
        assets: vec![Asset {
            id: 0,
            name: source.as_str().into(),
            size: manifest.size,
            digest: Some(format!("sha256:{}", manifest.sha256)),
            download_url: Some(payload.into()),
        }],
    })
}

/// Fetches one bounded manifest over TLS with no cookies, credentials or redirects.
pub(super) async fn fetch(client: &reqwest::Client, source: &url::Url) -> Result<Release> {
    let mut response = client
        .get(source.clone())
        .send()
        .await?
        .error_for_status()?;
    ensure!(response.status().is_success(), "mod manifest redirected");
    ensure!(
        response
            .content_length()
            .is_none_or(|size| size <= MAX_MANIFEST_BYTES as u64),
        "mod update manifest exceeds byte limit"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        ensure!(
            bytes.len() + chunk.len() <= MAX_MANIFEST_BYTES,
            "mod update manifest exceeds byte limit"
        );
        bytes.extend_from_slice(&chunk);
    }
    parse(&bytes, source)
}

#[cfg(test)]
mod tests;
