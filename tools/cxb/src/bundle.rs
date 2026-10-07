//! Builds a `.cxb` and checks it with the client's own bundle verifier before returning it.

use std::{
    borrow::Cow,
    collections::BTreeSet,
    io::{Cursor, Write},
    path::Path,
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use server_experience::{
    bundle::{MANIFEST_PATH, VerifiedBundle},
    crypto::{self, Ed25519KeyPair, SignedDocument},
    manifest::{ContentFile, Manifest, PackageOffer, Permission, Scope},
    policy::{API_VERSION, MAX_EXPANDED_BYTES, WIRE_VERSION},
    wire::Channel,
};
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter, write::SimpleFileOptions};

use crate::keys;

/// Where every bundle built here keeps its component.
pub const COMPONENT_PATH: &str = "component.wasm";

/// The publisher-written part of a manifest. Versions, the publisher key and the file index are
/// derived, so they cannot disagree with the bundle.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub package_version: String,
    #[serde(default)]
    pub permissions: BTreeSet<Permission>,
    #[serde(default)]
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub actions: BTreeSet<String>,
}

impl Source {
    /// Reads TOML or JSON, chosen by the file extension.
    pub fn read(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let parsed = match path.extension().and_then(|extension| extension.to_str()) {
            Some("toml") => toml::from_str(&text).map_err(anyhow::Error::from),
            Some("json") => serde_json::from_str(&text).map_err(anyhow::Error::from),
            _ => bail!("{}: expected a .toml or .json manifest", path.display()),
        };
        parsed.with_context(|| format!("parsing {}", path.display()))
    }
}

pub struct Bundle {
    /// The archive bytes; their sha256 and length are what an offer names.
    pub bytes: Vec<u8>,
    /// The signed manifest, byte for byte the archive's `manifest.signed.json`.
    pub manifest: SignedDocument,
}

/// Signs the manifest and packs it with the component and the `(path, bytes)` assets.
pub fn build(
    source: Source,
    wasm: &[u8],
    assets: &[(String, Vec<u8>)],
    publisher: &Ed25519KeyPair,
) -> Result<Bundle> {
    let component = componentize(wasm)?;
    let mut entries: Vec<(&str, &[u8])> = vec![(COMPONENT_PATH, &component)];
    for (path, bytes) in assets {
        ensure!(
            path != MANIFEST_PATH && path != COMPONENT_PATH,
            "asset {path} would replace a bundle file"
        );
        entries.push((path, bytes));
    }
    let manifest = Manifest {
        version: WIRE_VERSION,
        api: API_VERSION,
        id: source.id,
        publisher_key: keys::public_key(publisher),
        package_version: source.package_version,
        permissions: source.permissions,
        component: Some(COMPONENT_PATH.to_owned()),
        channels: source.channels,
        actions: source.actions,
        files: entries
            .iter()
            .map(|(path, bytes)| ContentFile {
                path: (*path).to_owned(),
                bytes: bytes.len() as u64,
                sha256: crypto::digest(bytes),
            })
            .collect(),
    };
    let signed = crypto::sign(&manifest, crypto::MANIFEST_DOMAIN, publisher)?;
    let signed_bytes = serde_json::to_vec(&signed)?;
    let mut archived = vec![(MANIFEST_PATH, signed_bytes.as_slice())];
    archived.extend(entries.iter().copied());
    let bytes = archive(&archived)?;
    let offer = PackageOffer {
        digest: crypto::digest(&bytes),
        bytes: bytes.len() as u64,
        id: manifest.id,
        publisher_key: manifest.publisher_key,
        url: String::new(),
    };
    let scope = Scope {
        permissions: manifest.permissions,
        origins: BTreeSet::new(),
        memory_bytes: 0,
        gpu_bytes: 0,
    };
    VerifiedBundle::read(&bytes, &offer, &scope, MAX_EXPANDED_BYTES)
        .context("the client would reject this bundle")?;
    Ok(Bundle {
        bytes,
        manifest: signed,
    })
}

/// Encodes a core module with the same encoder settings as `mod-host pack`; a component is
/// stored as given.
fn componentize(wasm: &[u8]) -> Result<Cow<'_, [u8]>> {
    ensure!(
        wasm.starts_with(b"\0asm"),
        "the component is not WebAssembly"
    );
    // The preamble's layer field is 0 for a core module and 1 for a component.
    match wasm.get(6..8) {
        Some([0, 0]) => Ok(Cow::Owned(
            wit_component::ComponentEncoder::default()
                .module(wasm)?
                .validate(true)
                .encode()?,
        )),
        Some([1, 0]) => Ok(Cow::Borrowed(wasm)),
        _ => bail!("unknown WebAssembly layer"),
    }
}

/// Stores regular files with a fixed timestamp, so equal inputs give an equal digest on any
/// machine and with any ZIP feature set.
fn archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Stored)
        .last_modified_time(DateTime::default());
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (path, bytes) in entries {
        writer.start_file(*path, options)?;
        writer.write_all(bytes)?;
    }
    let mut bytes = writer.finish()?.into_inner();
    // zip selects the central-directory creator OS from the build host. Pin it to DOS,
    // matching our original bundles, so signatures and content hashes remain portable.
    // These entries are regular, writable files; their other attributes are already equal.
    let offsets = {
        let mut archive = ZipArchive::new(Cursor::new(&bytes))?;
        (0..archive.len())
            .map(|index| {
                let file = archive.by_index(index)?;
                Ok(usize::try_from(file.central_header_start())? + 5)
            })
            .collect::<Result<Vec<_>>>()?
    };
    for offset in offsets {
        bytes[offset] = 0;
    }
    Ok(bytes)
}

/// Reads every regular file under `dir` as an asset at its `/`-separated relative path, sorted.
pub fn read_assets(dir: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    let mut assets = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in
            std::fs::read_dir(&next).with_context(|| format!("reading {}", next.display()))?
        {
            let entry = entry?;
            let kind = entry.file_type()?;
            if kind.is_dir() {
                pending.push(entry.path());
                continue;
            }
            ensure!(
                kind.is_file(),
                "{}: assets must be regular files",
                entry.path().display()
            );
            let relative = entry.path().strip_prefix(dir)?.to_owned();
            let path = relative
                .components()
                .map(|part| part.as_os_str().to_str())
                .collect::<Option<Vec<_>>>()
                .with_context(|| format!("{}: not UTF-8", relative.display()))?
                .join("/");
            assets.push((path, std::fs::read(entry.path())?));
        }
    }
    assets.sort();
    Ok(assets)
}
