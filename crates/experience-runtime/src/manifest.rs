//! `experience.toml`: an artifact's identity and the index that every other file must match.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::hex;
use crate::host::Api;
use crate::limits::{MAX_MANIFEST_BYTES, MAX_VERSION_BYTES};

/// The manifest's file name; `[files]` indexes every other file in the artifact.
pub const MANIFEST_FILE: &str = "experience.toml";
/// The guest core module.
pub const SERVER_WASM: &str = "server.wasm";
/// The directory that texture paths are relative to.
pub const ASSETS_DIR: &str = "assets";
/// The block-data schema this runtime implements.
pub const DATA_SCHEMA: u32 = 1;
const MAX_ID_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct Manifest {
    /// Matches `^[a-z][a-z0-9_]{0,31}$` and owns the block namespace `<id>:`.
    pub id: String,
    pub version: String,
    /// The server WIT's `major.minor`, one of those the runtime implements.
    pub api: String,
    pub data_schema: u32,
    /// `/`-separated relative path → lowercase hex SHA-256.
    pub files: BTreeMap<String, String>,
}

/// Reads and verifies `dir/experience.toml`: its size, the id, version, api and data schema, then
/// the index. Every file except the manifest must be indexed under a relative `/`-separated path
/// with its hash, and the artifact may hold nothing but regular files and directories.
pub fn read_manifest(dir: &Path) -> Result<Manifest> {
    let found = artifact_files(dir)?;
    let mut bytes = Vec::new();
    File::open(dir.join(MANIFEST_FILE))
        .and_then(|file| {
            file.take(MAX_MANIFEST_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .with_context(|| format!("reading {MANIFEST_FILE}"))?;
    ensure!(
        bytes.len() <= MAX_MANIFEST_BYTES,
        "{MANIFEST_FILE} exceeds {MAX_MANIFEST_BYTES} bytes"
    );
    let text = String::from_utf8(bytes).with_context(|| format!("{MANIFEST_FILE} is not UTF-8"))?;
    let manifest: Manifest =
        toml::from_str(&text).with_context(|| format!("parsing {MANIFEST_FILE}"))?;
    ensure!(
        is_id(&manifest.id),
        "invalid id \"{}\": ids match ^[a-z][a-z0-9_]{{0,31}}$",
        manifest.id
    );
    ensure!(
        is_version(&manifest.version),
        "invalid version: a version has 1 to {MAX_VERSION_BYTES} bytes and no control characters"
    );
    if Api::of(&manifest.api).is_none() {
        let supported: Vec<&str> = Api::ALL.into_iter().map(Api::api).collect();
        bail!(
            "unsupported api \"{}\"; this runtime implements {supported:?}",
            manifest.api
        );
    }
    ensure!(
        manifest.data_schema == DATA_SCHEMA,
        "unsupported data-schema {}; this runtime implements {DATA_SCHEMA}",
        manifest.data_schema
    );
    for path in manifest.files.keys() {
        ensure!(
            is_index_path(path) && path != MANIFEST_FILE,
            "invalid path \"{path}\" in [files]: paths are relative, use '/', stay inside the \
             artifact and exclude {MANIFEST_FILE}"
        );
    }
    if let Some(path) = found
        .iter()
        .find(|path| *path != MANIFEST_FILE && !manifest.files.contains_key(*path))
    {
        bail!("unindexed file {path}");
    }
    for (path, hash) in &manifest.files {
        ensure!(found.contains(path), "indexed file {path} is missing");
        let actual = sha256(&resolve(dir, path)).with_context(|| format!("hashing {path}"))?;
        let actual = hex::encode(&actual);
        ensure!(
            actual == *hash,
            "hash mismatch for {path}: [files] has {hash}, the file hashes to {actual}"
        );
    }
    Ok(manifest)
}

/// Joins a `/`-separated index path onto `dir` with the platform's separator.
pub(crate) fn resolve(dir: &Path, path: &str) -> PathBuf {
    path.split('/')
        .fold(dir.to_owned(), |full, part| full.join(part))
}

fn sha256(path: &Path) -> io::Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    io::copy(&mut File::open(path)?, &mut hasher)?;
    Ok(hasher.finalize().into())
}

/// `^[a-z][a-z0-9_]{0,31}$`.
fn is_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    matches!(bytes.first(), Some(b'a'..=b'z'))
        && bytes.len() <= MAX_ID_BYTES
        && bytes
            .iter()
            .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

/// 1 to [`MAX_VERSION_BYTES`] bytes without a control character.
fn is_version(version: &str) -> bool {
    (1..=MAX_VERSION_BYTES).contains(&version.len()) && !version.chars().any(char::is_control)
}

/// A path that can only name something inside the artifact on every platform: `/`-separated
/// components that are never empty, `.` or `..`, with no `\` and no `:` (drive prefixes and
/// alternate data streams).
fn is_index_path(path: &str) -> bool {
    path.split('/')
        .all(|part| !matches!(part, "" | "." | "..") && !part.contains(['\\', ':']))
}

/// Every file below `dir` as a `/`-separated relative path. Symlinks, non-UTF-8 names and
/// anything but regular files and directories are refused.
fn artifact_files(dir: &Path) -> Result<BTreeSet<String>> {
    let mut files = BTreeSet::new();
    let mut pending = vec![(dir.to_owned(), String::new())];
    while let Some((path, prefix)) = pending.pop() {
        let entries = fs::read_dir(&path).with_context(|| format!("listing {}", path.display()))?;
        for entry in entries {
            let entry = entry.with_context(|| format!("listing {}", path.display()))?;
            let name = entry
                .file_name()
                .into_string()
                .map_err(|name| anyhow!("{prefix}{} is not a UTF-8 name", name.display()))?;
            let relative = format!("{prefix}{name}");
            let kind = entry
                .file_type()
                .with_context(|| format!("inspecting {relative}"))?;
            if kind.is_symlink() {
                bail!("{relative} is a symlink");
            } else if kind.is_dir() {
                pending.push((entry.path(), format!("{relative}/")));
            } else if kind.is_file() {
                files.insert(relative);
            } else {
                bail!("{relative} is not a regular file or directory");
            }
        }
    }
    Ok(files)
}
