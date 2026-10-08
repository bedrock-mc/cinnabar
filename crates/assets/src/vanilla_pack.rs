//! Bounded, link-free extraction of the pinned sample-pack archive and its no-replace
//! publication. First-run setup and `assetc vanilla-pack` both unpack through here.

mod entries;
mod extract;
mod publish;
#[cfg(test)]
mod tests;

use std::{
    fs::{self, File},
    io::{self, Read},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

use crate::VanillaSource;

/// Workspace directory that holds verified archive downloads.
pub const DOWNLOAD_DIR: &str = ".local/assets/downloads";
/// Every manifest `cache_dir` must stay below this workspace directory.
const CACHE_ROOT: &str = ".local/assets/";
/// Exists only inside a complete pack.
const MARKER: &str = "resource_pack/blocks.json";

/// Extraction bounds, checked against the central directory and again against written bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UnpackLimits {
    pub max_entries: u64,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    /// Entries (and archives) with fewer compressed bytes skip the ratio checks.
    pub min_ratio_sample: u64,
    pub max_entry_ratio: f64,
    pub max_aggregate_ratio: f64,
}

impl UnpackLimits {
    /// Headroom over the pinned v1.26.50.4 pack: 22,859 entries, 319,437,100 bytes expanded,
    /// largest file 3,746,670 bytes, worst entry ratio about 120.5, aggregate 1.95. Raise a bound
    /// only after re-measuring a newer pin.
    pub const PINNED: Self = Self {
        max_entries: 65_536,
        max_file_bytes: 64 << 20,
        max_total_bytes: 1 << 30,
        min_ratio_sample: 4096,
        max_entry_ratio: 500.0,
        max_aggregate_ratio: 100.0,
    };
}

#[derive(Debug, thiserror::Error)]
pub enum UnpackError {
    /// The manifest or archive breaks the extraction contract.
    #[error("{0}")]
    Rejected(String),
    #[error("{context}: {error}")]
    Io { context: String, error: io::Error },
    #[error("unpacking was cancelled")]
    Cancelled,
}

impl UnpackError {
    fn io(context: impl std::fmt::Display) -> impl FnOnce(io::Error) -> Self {
        move |error| Self::Io {
            context: context.to_string(),
            error,
        }
    }
}

fn rejected(message: impl Into<String>) -> UnpackError {
    UnpackError::Rejected(message.into())
}

/// Workspace locations for one pinned pack, validated by [`VanillaSource::local_paths`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackPaths {
    pub archive: PathBuf,
    pub partial: PathBuf,
    pub cache: PathBuf,
}

impl PackPaths {
    /// True once a complete pack is published at `cache`.
    #[must_use]
    pub fn is_unpacked(&self) -> bool {
        self.cache.join(MARKER).is_file()
    }

    /// Removes staging directories abandoned beside the cache by killed runs; returns how many.
    pub fn reclaim_stale_staging(&self) -> usize {
        publish::reclaim_stale_staging(&self.cache, std::time::SystemTime::now())
    }
}

impl VanillaSource {
    /// Parses a workspace copy of the manifest.
    pub fn read(path: &Path) -> Result<Self, UnpackError> {
        let bytes = fs::read(path).map_err(UnpackError::io(format!("read {}", path.display())))?;
        serde_json::from_slice(&bytes)
            .map_err(|error| rejected(format!("parse {}: {error}", path.display())))
    }

    /// Rejects a manifest that could read or write outside `.local/assets` of `workspace`.
    pub fn local_paths(&self, workspace: &Path) -> Result<PackPaths, UnpackError> {
        if self.schema != 1 {
            return Err(rejected(format!(
                "unsupported vanilla source manifest schema: {}",
                self.schema
            )));
        }
        for (key, value) in [("url", &self.url), ("sha256", &self.sha256)] {
            if value.trim().is_empty() {
                return Err(rejected(format!(
                    "vanilla source manifest is missing '{key}'"
                )));
            }
        }
        let archive = &*self.archive;
        if !is_plain_component(archive) {
            return Err(rejected("archive must be exactly one nonempty basename"));
        }
        if &*self.artifact_policy != "local-only" {
            return Err(rejected(
                "vanilla source manifest must declare artifact_policy 'local-only'",
            ));
        }
        let cache = &*self.cache_dir;
        let Some(suffix) = cache.strip_prefix(CACHE_ROOT) else {
            return Err(rejected(format!(
                "cache_dir must stay below .local/assets: {cache}"
            )));
        };
        if suffix.contains('\\') {
            return Err(rejected(format!(
                "cache_dir must use forward-slash path components: {cache}"
            )));
        }
        if suffix
            .split('/')
            .any(|part| matches!(part, "" | "." | ".."))
        {
            return Err(rejected(format!(
                "cache_dir must not contain empty or traversal components: {cache}"
            )));
        }
        if !suffix.split('/').all(is_plain_component) {
            return Err(rejected(format!(
                "cache_dir must not contain drive, UNC or stream components: {cache}"
            )));
        }
        let root = workspace.join(CACHE_ROOT);
        let cache_path = suffix
            .split('/')
            .fold(root.clone(), |path, part| path.join(part));
        let archive_path = workspace.join(DOWNLOAD_DIR).join(archive);
        // Belt and braces over the component rules: a prefix component would replace the base.
        if !cache_path.starts_with(&root) || !archive_path.starts_with(workspace.join(DOWNLOAD_DIR))
        {
            return Err(rejected(format!(
                "cache_dir must stay below .local/assets: {cache}"
            )));
        }
        Ok(PackPaths {
            partial: archive_path.with_file_name(format!("{archive}.partial")),
            archive: archive_path,
            cache: cache_path,
        })
    }
}

/// One ordinary path component on every platform: no separators, traversal, or `:`, which
/// Windows reads as a drive prefix (`C:x`) or an alternate data stream.
fn is_plain_component(part: &str) -> bool {
    !matches!(part, "" | "." | "..")
        && !part.contains(['/', '\\', ':'])
        && matches!(
            Path::new(part).components().collect::<Vec<_>>().as_slice(),
            [std::path::Component::Normal(_)]
        )
}

/// Lowercase hex SHA-256 of the file at `path`.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unpacked {
    AlreadyPresent,
    Published,
}

/// Extracts the already verified `paths.archive` into `paths.cache` unless a complete pack is
/// there; never replaces an existing directory. `cancelled` is polled between entries.
pub fn unpack(
    paths: &PackPaths,
    limits: &UnpackLimits,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<Unpacked, UnpackError> {
    paths.reclaim_stale_staging();
    if paths.is_unpacked() {
        return Ok(Unpacked::AlreadyPresent);
    }
    if paths.cache.exists() {
        return Err(rejected(format!(
            "cache directory exists without {MARKER}: {}",
            paths.cache.display()
        )));
    }
    let parent = paths
        .cache
        .parent()
        .ok_or_else(|| rejected("cache directory has no parent"))?;
    fs::create_dir_all(parent).map_err(UnpackError::io(format!("create {}", parent.display())))?;
    let staging = publish::create_staging(&paths.cache)?;
    let result = extract::extract(&paths.archive, &staging, limits, cancelled).and_then(|()| {
        let root = normalized_root(&staging)?;
        publish::rename_no_replace(&root, &paths.cache)?;
        if root != staging {
            fs::remove_dir(&staging)
                .map_err(UnpackError::io(format!("remove {}", staging.display())))?;
        }
        Ok(())
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    if !paths.is_unpacked() {
        return Err(rejected(format!(
            "normalized source was not published: {}",
            paths.cache.join(MARKER).display()
        )));
    }
    Ok(Unpacked::Published)
}

/// The staged directory holding `resource_pack/`: staging itself or its single top-level child.
fn normalized_root(staging: &Path) -> Result<PathBuf, UnpackError> {
    if staging.join(MARKER).is_file() {
        return Ok(staging.to_path_buf());
    }
    let children = fs::read_dir(staging)
        .and_then(Iterator::collect::<io::Result<Vec<_>>>)
        .map_err(UnpackError::io(format!("read {}", staging.display())))?;
    let [only] = children.as_slice() else {
        return Err(rejected(
            "archive must contain exactly one top-level directory",
        ));
    };
    if !only.file_type().is_ok_and(|kind| kind.is_dir()) {
        return Err(rejected(
            "archive must contain exactly one top-level directory",
        ));
    }
    let root = only.path();
    if !root.join(MARKER).is_file() {
        return Err(rejected(format!("archive is missing {MARKER}")));
    }
    Ok(root)
}
