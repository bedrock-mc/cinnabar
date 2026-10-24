//! Immutable source bytes with immediate metadata checks and periodic content revalidation.

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

/// Rehash unchanged sources every two seconds: four normal 500 ms polls share one read.
/// Metadata alone cannot detect every write, including writes through existing mappings.
pub const CONTENT_REVALIDATION_INTERVAL: Duration = Duration::from_secs(2);

/// Bounds how long unchanged metadata may stand in for a fresh content read.
pub(crate) struct ContentDeadline(Instant);

impl ContentDeadline {
    /// Starts the next content revalidation window after a bounded read.
    pub(crate) fn new(now: Instant) -> Self {
        Self(now + CONTENT_REVALIDATION_INTERVAL)
    }

    /// Allows metadata-only reuse strictly before the next content revalidation.
    pub(crate) fn allows_reuse(&self, now: Instant) -> bool {
        now < self.0
    }
}

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
    deadline: ContentDeadline,
}

/// A worker-owned cache; each poll still reopens paths with the regular-file/link policy.
/// Equal metadata permits reuse only until the content revalidation deadline.
/// Finish each bounded source poll with `retain_observed`, including empty polls.
#[derive(Default)]
pub struct ObservationCache {
    files: BTreeMap<(PathBuf, usize), Cached>,
}

impl ObservationCache {
    /// Reads and hashes changed files, or shares the prior bytes after checking the opened leaf.
    pub fn read(&mut self, path: &Path, limit: usize, now: Instant) -> io::Result<Snapshot> {
        self.read_at(path, limit, now, Identity::of, |opened| opened.read(limit))
    }

    /// Drops files absent from the completed poll and starts tracking the next poll's reads.
    pub fn retain_observed(&mut self) {
        self.files.retain(|_, cached| {
            let observed = cached.observed;
            cached.observed = false;
            observed
        });
    }

    /// Revalidates content at each deadline even when metadata never changes.
    fn read_at(
        &mut self,
        path: &Path,
        limit: usize,
        now: Instant,
        identify: impl Fn(&File) -> io::Result<Identity>,
        read: impl FnOnce(&mut Opened) -> io::Result<Vec<u8>>,
    ) -> io::Result<Snapshot> {
        let key = (path.to_owned(), limit);
        let mut opened = open(path)?;
        let before = identify(&opened.file)?;
        if let Some(cached) = self.files.get_mut(&key)
            && before == cached.identity
            && cached.deadline.allows_reuse(now)
        {
            cached.observed = true;
            return Ok(cached.snapshot.clone());
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
        self.files.insert(
            key,
            Cached {
                identity: after,
                snapshot: snapshot.clone(),
                observed: true,
                deadline: ContentDeadline::new(now),
            },
        );
        Ok(snapshot)
    }
}

/// File identity plus content and metadata change times; access times are deliberately excluded.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Identity {
    created: Option<std::time::SystemTime>,
    modified: Option<std::time::SystemTime>,
    len: u64,
    #[cfg(unix)]
    values: (u64, u64, i64, i64),
}

impl Identity {
    /// Captures best-effort change metadata; the deadline covers writes it cannot distinguish.
    pub(crate) fn of(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        Ok(Self {
            created: metadata.created().ok(),
            modified: metadata.modified().ok(),
            len: metadata.len(),
            #[cfg(unix)]
            values: {
                use std::os::unix::fs::MetadataExt;
                (
                    metadata.dev(),
                    metadata.ino(),
                    metadata.ctime(),
                    metadata.ctime_nsec(),
                )
            },
        })
    }
}

#[cfg(test)]
mod tests;
