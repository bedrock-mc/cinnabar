//! The pinned GeyserMC sources: the manifest at `assets::facts::java_mapping::SOURCE_MANIFEST`
//! names a mappings-generator release archive (the published output of the pinned mappings
//! commit) by URL and SHA-256, and the SHA-256 of every file the generator reads from it; and
//! Geyser source files at one pinned commit, each by URL, SHA-256 and cache path.

use super::Result;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path};

const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// The fields the generator reads; provenance fields (repository, commit, editions, release)
/// document the pin for reviewers and the notice.
#[derive(Deserialize)]
struct Manifest {
    schema: u32,
    license: License,
    archive: Archive,
    files: Files,
    geyser: Geyser,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct License {
    spdx: String,
    path: String,
    sha256: String,
}

#[derive(Deserialize)]
struct Archive {
    url: String,
    sha256: String,
    size_bytes: u64,
    cache: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Files {
    blocks: File,
    java_blocks: File,
    tags: File,
    codec: File,
    biomes: File,
    items: File,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    path: String,
    sha256: String,
}

/// Geyser source files at one commit.
#[derive(Deserialize)]
struct Geyser {
    commit: String,
    spdx: String,
    files: GeyserFiles,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GeyserFiles {
    license: RemoteFile,
    vanilla_entities: RemoteFile,
    bedrock_entity_definitions: RemoteFile,
}

/// One repository file: its path at the commit, the URL serving exactly those bytes, its
/// SHA-256, and the cache path below the checkout root.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RemoteFile {
    path: String,
    url: String,
    sha256: String,
    cache: String,
}

/// The verified source files the generator reads.
pub struct SourceFiles {
    /// `blocks_debug.json`: Java block state to Bedrock block state.
    pub blocks: Vec<u8>,
    /// `Blocks.java`: Java blocks in registry order.
    pub java_blocks: String,
    /// `networkTags.nbt`: Java registry tags (blocks, biomes, ...) by registry id.
    pub tags: Vec<u8>,
    /// `networkCodec.nbt`: Java's synchronized data-driven registries, the biome registry
    /// among them, in network registry order.
    pub codec: Vec<u8>,
    /// `biomes.json`: Java biome to Bedrock biome id.
    pub biomes: Vec<u8>,
    /// `items.json`: Java item to Bedrock item identifier.
    pub items: Vec<u8>,
    /// Geyser's `VanillaEntities.java`: the Java entity types and their Bedrock entities.
    pub vanilla_entities: String,
    /// Geyser's `BedrockEntityDefinitions.java`: Bedrock entities shared by several Java types.
    pub bedrock_entity_definitions: String,
}

/// Reads and verifies every pinned file: archive files from `archive` or from the manifest's
/// cache below `root`, Geyser files from their caches below `root`, downloading any cache that
/// is missing or stale.
pub(super) fn load(root: &Path, manifest: &[u8], archive: Option<&Path>) -> Result<SourceFiles> {
    let manifest: Manifest = serde_json::from_slice(manifest)?;
    if manifest.schema != 1 || manifest.license.spdx != "MIT" || manifest.geyser.spdx != "MIT" {
        return Err("unsupported Geyser mappings source manifest".into());
    }
    let pinned = &manifest.archive;
    let bytes = match archive {
        Some(path) => read_verified(path, &pinned.sha256, Some(pinned.size_bytes))?
            .ok_or_else(|| format!("{} does not match the pinned archive", path.display()))?,
        None => {
            let cache = root.join(&pinned.cache);
            match read_verified(&cache, &pinned.sha256, Some(pinned.size_bytes))? {
                Some(bytes) => bytes,
                None => {
                    let bytes = download(&pinned.url, pinned.size_bytes)?;
                    if bytes.len() as u64 != pinned.size_bytes || !matches(&bytes, &pinned.sha256) {
                        return Err(
                            format!("{} does not match the pinned archive", pinned.url).into()
                        );
                    }
                    store(&cache, &bytes)?;
                    bytes
                }
            }
        }
    };
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
    let mut extract = |path: &str, sha256: &str| -> Result<Vec<u8>> {
        let mut entry = zip.by_name(path)?;
        let mut bytes = Vec::new();
        (&mut entry)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(format!("{path} exceeds its size limit").into());
        }
        if !matches(&bytes, sha256) {
            return Err(format!("{path} does not match its pinned SHA-256").into());
        }
        Ok(bytes)
    };
    extract(&manifest.license.path, &manifest.license.sha256)?;
    let files = &manifest.files;
    let geyser = &manifest.geyser;
    let remote = |file: &RemoteFile| -> Result<String> {
        Ok(String::from_utf8(fetch(root, &geyser.commit, file)?)?)
    };
    remote(&geyser.files.license)?;
    Ok(SourceFiles {
        blocks: extract(&files.blocks.path, &files.blocks.sha256)?,
        java_blocks: String::from_utf8(extract(
            &files.java_blocks.path,
            &files.java_blocks.sha256,
        )?)?,
        tags: extract(&files.tags.path, &files.tags.sha256)?,
        codec: extract(&files.codec.path, &files.codec.sha256)?,
        biomes: extract(&files.biomes.path, &files.biomes.sha256)?,
        items: extract(&files.items.path, &files.items.sha256)?,
        vanilla_entities: remote(&geyser.files.vanilla_entities)?,
        bedrock_entity_definitions: remote(&geyser.files.bedrock_entity_definitions)?,
    })
}

/// A Geyser file's bytes from its cache below `root`, downloaded into the cache when missing
/// or stale. Its URL must end in `/<commit>/<path>`, so the manifest cannot name one file
/// while serving another.
fn fetch(root: &Path, commit: &str, file: &RemoteFile) -> Result<Vec<u8>> {
    if !file.url.ends_with(&format!("/{commit}/{}", file.path)) {
        return Err(format!("{} is not {} at {commit}", file.url, file.path).into());
    }
    let cache = root.join(&file.cache);
    if let Some(bytes) = read_verified(&cache, &file.sha256, None)? {
        return Ok(bytes);
    }
    let bytes = download(&file.url, MAX_FILE_BYTES)?;
    if !matches(&bytes, &file.sha256) {
        return Err(format!("{} does not match its pinned SHA-256", file.url).into());
    }
    store(&cache, &bytes)?;
    Ok(bytes)
}

/// The bytes at `path` when they hash to `sha256` (and are `size` bytes long, checked first
/// when given); `None` when the file is missing or differs.
fn read_verified(path: &Path, sha256: &str, size: Option<u64>) -> Result<Option<Vec<u8>>> {
    match std::fs::metadata(path) {
        Ok(metadata) if size.is_none_or(|size| metadata.len() == size) => {}
        Ok(_) => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    }
    let bytes = std::fs::read(path)?;
    Ok(matches(&bytes, sha256).then_some(bytes))
}

fn matches(bytes: &[u8], sha256: &str) -> bool {
    format!("{:x}", Sha256::digest(bytes)) == sha256
}

/// Downloads `url`, failing once the body exceeds `limit` bytes; the caller verifies it.
fn download(url: &str, limit: u64) -> Result<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let mut response = reqwest::get(url).await?.error_for_status()?;
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await? {
            bytes.extend_from_slice(&chunk);
            if bytes.len() as u64 > limit {
                return Err::<_, Box<dyn std::error::Error>>(
                    format!("{url} exceeds its pinned size").into(),
                );
            }
        }
        Ok(bytes)
    })
}

/// Writes verified bytes to `cache` atomically, creating its directory.
fn store(cache: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(cache.parent().ok_or("cache path has no parent")?)?;
    assets::write_blob_atomic(cache, bytes)?;
    Ok(())
}
