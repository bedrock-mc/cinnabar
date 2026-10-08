//! Signed offer scope and immutable bundle index.

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{crypto::fixed_hex, policy::*};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Ui,
    ModalUi,
    Input,
    Messaging,
    Scene,
    Media,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub permissions: BTreeSet<Permission>,
    pub origins: BTreeSet<String>,
    pub memory_bytes: u64,
    pub gpu_bytes: u64,
}

impl Scope {
    /// Validates the user-visible scope against host ceilings.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.memory_bytes <= MAX_SESSION_MEMORY,
            "memory grant too large"
        );
        ensure!(self.gpu_bytes <= MAX_GPU_BYTES, "GPU grant too large");
        ensure!(self.origins.len() <= MAX_ORIGINS, "too many origins");
        for origin in &self.origins {
            let url = crate::fetch::approved_url(origin, &self.origins)?;
            ensure!(
                url.origin().ascii_serialization() == *origin,
                "origin must be canonical"
            );
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PackageOffer {
    pub id: String,
    pub publisher_key: String,
    pub digest: String,
    pub bytes: u64,
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub version: u16,
    pub audience: String,
    pub server_key: String,
    pub revision: u64,
    pub expires_unix: u64,
    pub scope: Scope,
    pub packages: Vec<PackageOffer>,
    pub fallback: String,
    pub carrier: String,
}

impl Offer {
    /// Checks all pre-consent metadata without contacting an external origin.
    pub fn validate(&self, audience: &str, now_unix: u64) -> Result<()> {
        ensure!(
            self.version == WIRE_VERSION,
            "unsupported extension version"
        );
        ensure!(self.audience == audience, "wrong server audience");
        fixed_hex::<32>(&self.server_key)?;
        ensure!(self.expires_unix > now_unix, "offer expired");
        ensure!(
            self.expires_unix - now_unix <= MAX_OFFER_LIFETIME_SECS,
            "offer lifetime too long"
        );
        ensure!(
            self.carrier == protocol::EXPERIENCE_CHANNEL,
            "unsupported carrier"
        );
        ensure!(
            !self.packages.is_empty() && self.packages.len() <= MAX_BUNDLES,
            "invalid package count"
        );
        ensure!(
            plain_text(&self.fallback, MAX_FALLBACK_BYTES),
            "invalid fallback description"
        );
        self.scope.validate()?;
        let mut ids = BTreeSet::new();
        let mut total = 0u64;
        for package in &self.packages {
            ensure!(
                identifier(&package.id) && ids.insert(&package.id),
                "invalid package identity"
            );
            fixed_hex::<32>(&package.publisher_key)?;
            fixed_hex::<32>(&package.digest)?;
            ensure!(
                package.bytes > 0 && package.bytes <= MAX_BUNDLE_BYTES as u64,
                "invalid bundle size"
            );
            total = total
                .checked_add(package.bytes)
                .ok_or_else(|| anyhow::anyhow!("size overflow"))?;
            crate::fetch::approved_url(&package.url, &self.scope.origins)?;
        }
        ensure!(
            total <= MAX_EXPANDED_BYTES,
            "aggregate bundle size exceeded"
        );
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ContentFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u16,
    pub api: u16,
    pub id: String,
    pub publisher_key: String,
    pub package_version: String,
    pub permissions: BTreeSet<Permission>,
    pub component: Option<String>,
    pub channels: Vec<crate::wire::Channel>,
    pub actions: BTreeSet<String>,
    /// JSON-UI files the modal may open, each also indexed in `files`. Omitted when empty, so a
    /// manifest without screens keeps its canonical bytes.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub templates: BTreeSet<String>,
    pub files: Vec<ContentFile>,
}

impl Manifest {
    /// Bounds the modal screen files: every template is an indexed `ui/<name>.json`, and every
    /// image under `textures/` is a bounded PNG.
    pub fn validate_screens(&self) -> Result<()> {
        ensure!(self.templates.len() <= MAX_TEMPLATES, "too many templates");
        for template in &self.templates {
            ensure!(template_root(template).is_some(), "invalid template path");
            ensure!(
                self.files
                    .iter()
                    .any(|file| &file.path == template && file.bytes <= MAX_TEMPLATE_BYTES as u64),
                "template missing from index or too large"
            );
        }
        let mut textures = 0;
        for file in self
            .files
            .iter()
            .filter(|file| file.path.starts_with(TEXTURE_DIR))
        {
            let image = file.path.ends_with(".png") && file.bytes <= MAX_TEXTURE_BYTES as u64;
            let sidecar = file.path.ends_with(".json") && file.bytes <= MAX_TEMPLATE_BYTES as u64;
            ensure!(image || sidecar, "invalid texture file");
            textures += 1;
        }
        ensure!(textures <= MAX_TEXTURES, "too many textures");
        Ok(())
    }
}

/// Where a bundle keeps the images its screens may draw.
pub const TEXTURE_DIR: &str = "textures/";

/// The control a template file opens as: `ui/terminal.json` opens `terminal` of the bundle's
/// namespace. `None` for a path that is not one `ui/` file with a JSON-UI name.
pub fn template_root(path: &str) -> Option<&str> {
    let name = path.strip_prefix("ui/")?.strip_suffix(".json")?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_'))
    .then_some(name)
}

/// The JSON-UI namespace every template of bundle `id` declares: the id with each character a
/// JSON-UI name cannot hold replaced by `_`.
pub fn template_namespace(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Restricts identifiers to an unambiguous, portable owned namespace.
pub fn identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_IDENTIFIER_BYTES
        && text.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b':' | b'_' | b'-' | b'.')
        })
}

/// Keeps remote strings out of control markup and trusted UI formatting.
pub fn plain_text(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && !text.chars().any(|c| c.is_control() || c == '§')
}

/// Lists only capabilities with application adapters in this developer preview.
pub fn implemented_permissions() -> BTreeSet<Permission> {
    BTreeSet::from([
        Permission::Ui,
        Permission::ModalUi,
        Permission::Input,
        Permission::Messaging,
    ])
}

/// What the developer client advertises: the above plus media playback onto scene quads.
pub fn developer_permissions() -> BTreeSet<Permission> {
    let mut permissions = implemented_permissions();
    permissions.extend([Permission::Scene, Permission::Media]);
    permissions
}
