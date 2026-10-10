use std::io::Read;
use std::path::{Component, Path, PathBuf};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use super::invalid;
use crate::BridgeError;

/// An archive retained in the core's cache for the lifetime of the session.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CachedArchive {
    pub path: PathBuf,
    pub sha256: [u8; 32],
}

impl CachedArchive {
    /// Reads only from the client's trusted cache root and verifies the exact archive bytes.
    /// Call this on a blocking worker; the root must come from client configuration, not the handoff.
    pub(super) fn read(&self, root: Option<&Path>, size: u64) -> Result<Vec<u8>, BridgeError> {
        let root = root.ok_or(invalid(
            "cached archive without a configured cache directory",
        ))?;
        let root = root.canonicalize()?;
        if !self.path.is_absolute() {
            return Err(invalid("cached archive path is not absolute"));
        }
        if self
            .path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(invalid("cached archive path contains traversal"));
        }
        // Resolve both paths with the same platform rules, including Windows verbatim prefixes
        // and the case-folded paths published by the Go cache.
        let path = self.path.canonicalize()?;
        let relative = path
            .strip_prefix(&root)
            .map_err(|_| invalid("cached archive path is outside the cache directory"))?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            return Err(invalid("cached archive path contains traversal"));
        }
        // Directory-relative opens prevent symlinked parent directories escaping the cache,
        // including when another process changes them between validation and open.
        let directory = Dir::open_ambient_dir(&root, ambient_authority())?;
        if !directory.symlink_metadata(relative)?.is_file() {
            return Err(invalid("cached archive is not a regular file"));
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let file = directory.open_with(relative, &options)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != size {
            return Err(invalid("cached archive size does not match the handoff"));
        }
        let limit = size
            .checked_add(1)
            .ok_or(invalid("cached archive size overflows"))?;
        let mut archive = Vec::new();
        file.take(limit).read_to_end(&mut archive)?;
        if archive.len() as u64 != size {
            return Err(invalid("cached archive size changed while reading"));
        }
        if <[u8; 32]>::from(Sha256::digest(&archive)) != self.sha256 {
            return Err(invalid("cached archive hash does not match the handoff"));
        }
        Ok(archive)
    }
}
