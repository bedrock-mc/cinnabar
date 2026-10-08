//! Complete manifest/content verification before any component is compiled.

use crate::{
    crypto::{self, SignedDocument},
    manifest::{Manifest, PackageOffer, Scope},
    policy::*,
};
use anyhow::{Result, ensure};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
};

mod directory;

pub const MANIFEST_PATH: &str = "manifest.signed.json";

#[derive(Debug)]
pub struct VerifiedBundle {
    pub manifest: Manifest,
    files: BTreeMap<String, Vec<u8>>,
    digest: String,
}

impl VerifiedBundle {
    /// Accepts only indexed regular files, without extracting paths to disk.
    pub fn read(
        bytes: &[u8],
        offer: &PackageOffer,
        scope: &Scope,
        remaining_expanded: u64,
    ) -> Result<Self> {
        ensure!(
            bytes.len() <= MAX_BUNDLE_BYTES && bytes.len() as u64 == offer.bytes,
            "bundle size mismatch"
        );
        ensure!(
            crypto::digest(bytes) == offer.digest,
            "bundle hash mismatch"
        );
        let entries = directory::validate(bytes)?;
        let mut total = 0u64;
        for file in entries.values() {
            total = total
                .checked_add(file.size)
                .ok_or_else(|| anyhow::anyhow!("archive size overflow"))?;
            ensure!(
                total <= remaining_expanded.min(MAX_EXPANDED_BYTES),
                "expanded archive limit exceeded"
            );
        }
        let mut remaining = remaining_expanded.min(MAX_EXPANDED_BYTES);
        let manifest_size = entries
            .get(MANIFEST_PATH)
            .ok_or_else(|| anyhow::anyhow!("manifest missing"))?
            .size;
        ensure!(
            manifest_size <= MAX_MARKER_BYTES as u64,
            "manifest too large"
        );
        let signed = read_entry(&entries, MANIFEST_PATH, manifest_size, &mut remaining)?;
        let signed: SignedDocument = serde_json::from_slice(&signed)?;
        let (manifest, _): (Manifest, _) = signed.verify(
            &offer.publisher_key,
            crypto::MANIFEST_DOMAIN,
            MAX_MARKER_BYTES / 2,
        )?;
        ensure!(
            manifest.version == WIRE_VERSION && manifest.api == API_VERSION,
            "unsupported manifest API"
        );
        ensure!(
            manifest.id == offer.id && manifest.publisher_key == offer.publisher_key,
            "publisher substitution"
        );
        ensure!(
            manifest.permissions.is_subset(&scope.permissions),
            "undeclared permission"
        );
        ensure!(
            manifest.channels.len() <= MAX_CHANNELS && manifest.actions.len() <= MAX_ACTIONS,
            "declaration limit exceeded"
        );
        let mut channels = BTreeSet::new();
        for channel in &manifest.channels {
            ensure!(
                channel.id.starts_with(&format!("{}.", manifest.id))
                    && channel.declared()
                    && channels.insert((&channel.id, channel.schema)),
                "invalid channel declaration"
            );
        }
        ensure!(
            manifest
                .actions
                .iter()
                .all(|id| crate::manifest::identifier(id)),
            "invalid action declaration"
        );
        manifest.validate_screens()?;
        ensure!(
            manifest.files.len() + 1 == entries.len(),
            "unindexed archive entry"
        );
        let mut files = BTreeMap::new();
        for entry in &manifest.files {
            ensure!(
                safe_path(&entry.path) && entry.path != MANIFEST_PATH,
                "invalid index path"
            );
            crypto::fixed_hex::<32>(&entry.sha256)?;
            ensure!(entry.bytes <= MAX_EXPANDED_BYTES, "file size exceeded");
            let data = read_entry(&entries, &entry.path, entry.bytes, &mut remaining)?;
            ensure!(
                data.len() as u64 == entry.bytes && crypto::digest(&data) == entry.sha256,
                "content hash mismatch"
            );
            ensure!(
                files.insert(entry.path.clone(), data).is_none(),
                "duplicate index entry"
            );
        }
        if let Some(component) = &manifest.component {
            let data = files
                .get(component)
                .ok_or_else(|| anyhow::anyhow!("component missing from index"))?;
            ensure!(data.len() <= MAX_COMPONENT_BYTES, "component too large");
            ensure!(
                data.starts_with(b"\0asm"),
                "only portable WebAssembly is accepted"
            );
        }
        let namespace = crate::manifest::template_namespace(&manifest.id);
        for template in &manifest.templates {
            let data = files
                .get(template)
                .ok_or_else(|| anyhow::anyhow!("template missing from index"))?;
            crate::screen::validate_template(data, &namespace)?;
        }
        Ok(Self {
            manifest,
            files,
            digest: offer.digest.clone(),
        })
    }

    /// Returns the verified archive digest selected by the signed package offer.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// Reads only an immutable, already verified bundle asset.
    pub fn file(&self, path: &str) -> Option<&[u8]> {
        self.files.get(path).map(Vec::as_slice)
    }

    /// Returns the optional portable component, never serialized native code.
    pub fn component(&self) -> Option<&[u8]> {
        self.manifest
            .component
            .as_deref()
            .and_then(|path| self.file(path))
    }

    /// Moves the verified component to its helper without copying the payload.
    pub fn into_component(mut self) -> Option<Vec<u8>> {
        self.take_component()
    }

    /// Moves the component out while keeping the other assets, such as media descriptors.
    pub fn take_component(&mut self) -> Option<Vec<u8>> {
        self.manifest
            .component
            .as_deref()
            .and_then(|path| self.files.remove(path))
    }

    /// Moves the modal's templates and textures to the presenter. A bundle that holds `media`
    /// keeps a copy of its textures, since a media descriptor's poster may be one of them.
    pub fn take_screen_files(&mut self) -> crate::screen::Files {
        let templates = self
            .manifest
            .templates
            .iter()
            .filter_map(|path| Some((path.clone(), self.files.remove(path)?)))
            .collect();
        let is_texture = |path: &String| path.starts_with(crate::manifest::TEXTURE_DIR);
        let textures = if self
            .manifest
            .permissions
            .contains(&crate::manifest::Permission::Media)
        {
            self.files
                .iter()
                .filter(|(path, _)| is_texture(path))
                .map(|(path, bytes)| (path.clone(), bytes.clone()))
                .collect()
        } else {
            let paths: Vec<String> = self
                .files
                .keys()
                .filter(|path| is_texture(path))
                .cloned()
                .collect();
            paths
                .into_iter()
                .filter_map(|path| {
                    let bytes = self.files.remove(&path)?;
                    Some((path, bytes))
                })
                .collect()
        };
        crate::screen::Files {
            namespace: crate::manifest::template_namespace(&self.manifest.id),
            templates,
            textures,
        }
    }

    /// Counts actual retained file bytes for the aggregate session budget.
    pub fn expanded_bytes(&self) -> u64 {
        self.files.values().map(|bytes| bytes.len() as u64).sum()
    }

    /// Lists only signed and hash-verified asset identities.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }
}

/// Permits a narrow portable path subset with no ambiguous aliases.
fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 256
        && path.split('/').count() <= 8
        && path.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'/' | b'.' | b'_' | b'-')
        })
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Checks declared size before decompression and caps reads independently.
fn read_entry(
    entries: &BTreeMap<&str, directory::Entry<'_>>,
    path: &str,
    size: u64,
    remaining: &mut u64,
) -> Result<Vec<u8>> {
    let entry = entries
        .get(path)
        .ok_or_else(|| anyhow::anyhow!("indexed entry missing"))?;
    ensure!(entry.size == size, "entry size differs from signed size");
    ensure!(size <= *remaining, "expanded archive limit exceeded");
    *remaining -= size;
    let mut reader = entry.local;
    let mut file = zip::read::read_zipfile_from_stream(&mut reader)?
        .ok_or_else(|| anyhow::anyhow!("missing local entry"))?;
    let mut bytes = vec![0; usize::try_from(size)?];
    file.read_exact(&mut bytes)?;
    ensure!(file.read(&mut [0])? == 0, "entry expanded beyond limit");
    Ok(bytes)
}
