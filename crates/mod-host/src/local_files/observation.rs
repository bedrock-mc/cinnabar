//! Immutable source bytes reused only while a newly opened leaf has the same change identity.

use super::{Opened, open};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io,
    path::Path,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// A subsecond timestamp can repeat until its clock tick ends; confirm once after that window.
pub(crate) const CONFIRMATION_DELAY: Duration = Duration::from_secs(1);

/// One bounded source read and its digest, shared without copying the source buffer.
#[derive(Clone)]
pub struct Snapshot {
    pub bytes: Arc<[u8]>,
    pub digest: [u8; 32],
}

struct Cached {
    identity: Identity,
    snapshot: Snapshot,
    observed: bool,
    confirm_after: Option<Instant>,
}

/// A worker-owned cache; each poll still reopens paths with the regular-file/link policy.
/// Files without a reliable change identity are read on every poll.
/// Finish each bounded source poll with `retain_observed`, including empty polls.
#[derive(Default)]
pub struct ObservationCache {
    files: BTreeMap<(PathBuf, usize), Cached>,
}

impl ObservationCache {
    /// Reads and hashes changed files, or shares the prior bytes after checking the opened leaf.
    pub fn read(&mut self, path: &Path, limit: usize) -> io::Result<Snapshot> {
        self.read_with(path, limit, |opened| opened.read(limit))
    }

    /// Drops files absent from the completed poll and starts tracking the next poll's reads.
    pub fn retain_observed(&mut self) {
        self.files.retain(|_, cached| {
            let observed = cached.observed;
            cached.observed = false;
            observed
        });
    }

    /// Uses the filesystem edge clock for changed reads and one confirmation of a new stamp.
    fn read_with(
        &mut self,
        path: &Path,
        limit: usize,
        read: impl FnOnce(&mut Opened) -> io::Result<Vec<u8>>,
    ) -> io::Result<Snapshot> {
        self.read_at(path, limit, Instant::now(), Identity::of, read)
    }

    /// Confirms a newly observed stamp once after its possible subsecond collision window.
    fn read_at(
        &mut self,
        path: &Path,
        limit: usize,
        now: Instant,
        identify: impl Fn(&File) -> io::Result<Option<Identity>>,
        read: impl FnOnce(&mut Opened) -> io::Result<Vec<u8>>,
    ) -> io::Result<Snapshot> {
        let key = (path.to_owned(), limit);
        let mut opened = open(path)?;
        let before = identify(&opened.file)?;
        let mut confirming = false;
        if let Some(cached) = self.files.get_mut(&key)
            && before.as_ref() == Some(&cached.identity)
        {
            confirming = cached.confirm_after.is_some_and(|deadline| now >= deadline);
            if !confirming {
                cached.observed = true;
                return Ok(cached.snapshot.clone());
            }
        }
        let previous = self.files.remove(&key);
        if opened.file.metadata()?.len() > limit as u64 {
            return Err(io::Error::other("source exceeds byte limit"));
        }
        let bytes = read(&mut opened)?;
        let after = identify(&opened.file)?;
        if before != after {
            return Err(io::Error::other("source changed while being read"));
        }
        let digest = Sha256::digest(&bytes).into();
        let snapshot = match previous {
            Some(previous) if previous.snapshot.digest == digest => previous.snapshot,
            _ => Snapshot {
                digest,
                bytes: bytes.into(),
            },
        };
        if let Some(identity) = after {
            self.files.insert(
                key,
                Cached {
                    identity,
                    snapshot: snapshot.clone(),
                    observed: true,
                    confirm_after: (!confirming).then_some(now + CONFIRMATION_DELAY),
                },
            );
        }
        Ok(snapshot)
    }
}

/// File identity plus content and metadata change times; access times are deliberately excluded.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    #[cfg(unix)]
    created: Option<std::time::SystemTime>,
    #[cfg(unix)]
    values: (u64, u64, u64, i64, i64, i64, i64),
}

impl Identity {
    /// Uses Unix file identities with subsecond stamps; other platforms keep fresh bounded reads.
    pub(crate) fn of(file: &File) -> io::Result<Option<Self>> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = file.metadata()?;
            if metadata.mtime_nsec() == 0 || metadata.ctime_nsec() == 0 {
                return Ok(None);
            }
            Ok(Some(Self {
                created: metadata.created().ok(),
                values: (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.len(),
                    metadata.mtime(),
                    metadata.mtime_nsec(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                ),
            }))
        }
        #[cfg(not(unix))]
        {
            // Windows mapped writes need not update file timestamps.
            let _ = file;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
