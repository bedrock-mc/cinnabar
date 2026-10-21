//! Staging directories beside the cache and their atomic, never-replacing publication.

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use super::{UnpackError, rejected};

/// Leftovers of interrupted runs older than this are reclaimed.
const STALE_STAGING_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
/// At most this many fresher leftovers survive, youngest first.
const STALE_STAGING_KEEP: usize = 4;

fn staging_prefix(cache: &Path) -> Option<String> {
    Some(format!("{}.extracting", cache.file_name()?.to_str()?))
}

/// A new, empty staging directory beside `cache`, on the same volume so publication is a rename.
pub(super) fn create_staging(cache: &Path) -> Result<PathBuf, UnpackError> {
    let prefix = staging_prefix(cache).ok_or_else(|| rejected("cache directory has no name"))?;
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let staging = cache.with_file_name(format!("{prefix}-{}-{nanos}", std::process::id()));
    fs::create_dir(&staging).map_err(UnpackError::io(format!("create {}", staging.display())))?;
    Ok(staging)
}

/// Removes staging siblings abandoned by killed runs; returns how many were removed. A leftover
/// whose age is unknown counts as fresh.
pub(super) fn reclaim_stale_staging(cache: &Path, now: SystemTime) -> usize {
    let (Some(prefix), Some(parent)) = (staging_prefix(cache), cache.parent()) else {
        return 0;
    };
    let Ok(children) = fs::read_dir(parent) else {
        return 0;
    };
    let mut candidates: Vec<(Duration, PathBuf)> = children
        .flatten()
        .filter(|child| child.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|child| {
            child
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(&prefix))
        })
        .map(|child| {
            let age = child
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|modified| now.duration_since(modified).ok())
                .unwrap_or_default();
            (age, child.path())
        })
        .collect();
    candidates.sort();
    candidates
        .into_iter()
        .enumerate()
        .filter(|(kept, (age, _))| *kept >= STALE_STAGING_KEEP || *age > STALE_STAGING_MAX_AGE)
        .filter(|(_, (_, path))| fs::remove_dir_all(path).is_ok())
        .count()
}

/// Renames `from` to `to` only while `to` does not exist, in one atomic operation.
pub(super) fn rename_no_replace(from: &Path, to: &Path) -> Result<(), UnpackError> {
    match crate::rename_no_replace(from, to) {
        Ok(()) => Ok(()),
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::AlreadyExists | io::ErrorKind::DirectoryNotEmpty
            ) =>
        {
            Err(rejected(format!(
                "cache directory appeared during extraction: {}",
                to.display()
            )))
        }
        Err(error) => Err(UnpackError::io(format!(
            "atomic no-replace directory rename failed ({} -> {})",
            from.display(),
            to.display()
        ))(error)),
    }
}
