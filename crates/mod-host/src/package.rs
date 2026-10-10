//! A player mod package: a directory with `mod.toml`, the component, its JSON-UI templates and
//! textures. Every file is read bounded and checked against the manifest's SHA-256 before
//! anything compiles.

use crate::MAX_COMPONENT_BYTES;
use anyhow::{Context, Result, ensure};
use experience_sdk::mod_manifest::{COMPONENT, ModManifest};
use server_experience::{
    policy::{MAX_TEMPLATE_BYTES, MAX_TEMPLATES, MAX_TEXTURE_BYTES, MAX_TEXTURES},
    screen,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

/// The manifest file at a package's root.
pub const MANIFEST: &str = "mod.toml";
const MAX_MANIFEST_BYTES: usize = 64 * 1024;

/// A verified package, ready to instantiate.
pub struct Package {
    pub manifest: ModManifest,
    pub component: Vec<u8>,
    /// The templates and textures, as a client part's modal holds them.
    pub files: Arc<screen::Files>,
    /// The SHA-256 of the manifest and every file, which reload compares.
    pub digest: [u8; 32],
}

impl Package {
    /// Reads and verifies the package at `dir`.
    pub fn read(dir: &Path) -> Result<Self> {
        let text = read_bounded(&dir.join(MANIFEST), MAX_MANIFEST_BYTES)?;
        let text = String::from_utf8(text).context("mod.toml is not UTF-8")?;
        let manifest =
            ModManifest::parse(&text).map_err(|error| anyhow::anyhow!("{MANIFEST}: {error}"))?;
        ensure!(
            manifest.templates.len() <= MAX_TEMPLATES,
            "too many templates"
        );
        ensure!(manifest.textures.len() <= MAX_TEXTURES, "too many textures");
        let mut digest = Sha256::new();
        digest.update(text.as_bytes());
        let mut read = |path: &str, limit: usize| -> Result<Vec<u8>> {
            let bytes = read_bounded(&package_path(dir, path), limit)?;
            let hash = Sha256::digest(&bytes);
            ensure!(
                manifest.hash(path) == Some(hex(&hash).as_str()),
                "{path} does not match its hash in {MANIFEST}"
            );
            digest.update(hash);
            Ok(bytes)
        };
        let component = read(COMPONENT, MAX_COMPONENT_BYTES)?;
        let namespace = manifest.namespace();
        let mut files = screen::Files {
            namespace: namespace.clone(),
            ..screen::Files::default()
        };
        for path in &manifest.templates {
            let bytes = read(path, MAX_TEMPLATE_BYTES)?;
            screen::validate_template(&bytes, &namespace).with_context(|| path.clone())?;
            files.templates.insert(path.clone(), bytes);
        }
        for path in &manifest.textures {
            files
                .textures
                .push((path.clone(), read(path, MAX_TEXTURE_BYTES)?));
        }
        Ok(Self {
            manifest,
            component,
            files: Arc::new(files),
            digest: digest.finalize().into(),
        })
    }
}

/// A manifest path under the package directory; the manifest already refused `..` and
/// absolute paths.
fn package_path(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_owned(), |out, part| out.join(part))
}

/// The path whose extension becomes `settings.json` for the package at `dir`, so its settings
/// live beside the directory as `<directory name>.settings.json`.
pub(crate) fn settings_anchor(dir: &Path) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_default().to_owned();
    name.push(".package");
    dir.with_file_name(name)
}

/// Reads at most `limit` bytes, refusing a larger file even if it grows while read.
pub(crate) fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("open {}", path.display()))?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "{} exceeds its byte limit",
        path.display()
    );
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests;
