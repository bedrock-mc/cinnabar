//! Signed offer scope and immutable bundle index.

// BEGIN GENERATED EXTENSION CONTRACT
// Generated from crates/server-experience/schema/extension.json; do not edit.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub permissions: BTreeSet<Permission>,
    pub origins: BTreeSet<String>,
    pub memory_bytes: u64,
    pub gpu_bytes: u64,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub personal_mods: BTreeSet<String>,
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
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub templates: BTreeSet<String>,
    pub files: Vec<ContentFile>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Ui,
    ModalUi,
    Input,
    Messaging,
    Scene,
    Media,
    Items,
    WorldDraw,
    EntityObserve,
    EntityHighlight,
    RenderThroughWalls,
    RenderMaterial,
    RenderPost,
}

/// Restricts identifiers to the shared portable namespace.
pub fn identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_IDENTIFIER_BYTES
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b":_-.".contains(&b))
}
/// Lists permissions with application adapters in the developer preview.
pub fn implemented_permissions() -> BTreeSet<Permission> {
    BTreeSet::from([
        Permission::Ui,
        Permission::ModalUi,
        Permission::Input,
        Permission::Messaging,
        Permission::WorldDraw,
        Permission::EntityObserve,
        Permission::EntityHighlight,
        Permission::RenderThroughWalls,
        Permission::RenderMaterial,
        Permission::RenderPost,
    ])
}
// END GENERATED EXTENSION CONTRACT

use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

use crate::{crypto::fixed_hex, policy::*};

impl Scope {
    /// Validates the user-visible scope against host ceilings.
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.memory_bytes <= MAX_SESSION_MEMORY,
            "memory grant too large"
        );
        ensure!(self.gpu_bytes <= MAX_GPU_BYTES, "GPU grant too large");
        ensure!(self.origins.len() <= MAX_ORIGINS, "too many origins");
        ensure!(
            self.personal_mods.len() <= MAX_BUNDLES,
            "too many personal mod grants"
        );
        for digest in &self.personal_mods {
            fixed_hex::<32>(digest)?;
        }
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

impl Manifest {
    /// Bounds the modal screen files: every template is an indexed `ui/<name>.json`, and every
    /// image under `textures/` is a bounded PNG.
    pub fn validate_screens(&self) -> Result<()> {
        ensure!(self.templates.len() <= MAX_TEMPLATES, "too many templates");
        for template in &self.templates {
            ensure!(
                crate::screen::template_root(template).is_some(),
                "invalid template path"
            );
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

/// Keeps remote strings out of control markup and trusted UI formatting.
pub fn plain_text(text: &str, limit: usize) -> bool {
    !text.is_empty() && text.len() <= limit && !text.chars().any(|c| c.is_control() || c == '§')
}

/// What the developer client advertises: the above plus media playback onto scene quads and
/// reading the session's items.
pub fn developer_permissions() -> BTreeSet<Permission> {
    let mut permissions = implemented_permissions();
    permissions.extend([Permission::Scene, Permission::Media, Permission::Items]);
    permissions
}
