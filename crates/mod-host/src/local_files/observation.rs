//! Immutable source bytes reused only while a newly opened leaf has the same change identity.

use super::{Opened, open};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs::File, io, path::Path, path::PathBuf, sync::Arc};

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
}

/// A worker-owned cache; each poll still reopens paths with the regular-file/link policy.
/// Files without a reliable change identity are read on every poll.
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

    /// Invokes the bounded byte reader only after the newly opened leaf changes identity.
    fn read_with(
        &mut self,
        path: &Path,
        limit: usize,
        read: impl FnOnce(&mut Opened) -> io::Result<Vec<u8>>,
    ) -> io::Result<Snapshot> {
        let key = (path.to_owned(), limit);
        let mut opened = open(path)?;
        let before = Identity::of(&opened.file)?;
        if let Some(cached) = self.files.get_mut(&key)
            && before.as_ref() == Some(&cached.identity)
        {
            cached.observed = true;
            return Ok(cached.snapshot.clone());
        }
        self.files.remove(&key);
        if opened.file.metadata()?.len() > limit as u64 {
            return Err(io::Error::other("source exceeds byte limit"));
        }
        let bytes = read(&mut opened)?;
        let after = Identity::of(&opened.file)?;
        if before != after {
            return Err(io::Error::other("source changed while being read"));
        }
        let snapshot = Snapshot {
            digest: Sha256::digest(&bytes).into(),
            bytes: bytes.into(),
        };
        if let Some(identity) = after {
            self.files.insert(
                key,
                Cached {
                    identity,
                    snapshot: snapshot.clone(),
                    observed: true,
                },
            );
        }
        Ok(snapshot)
    }
}

/// File identity plus content and metadata change times; access times are deliberately excluded.
#[derive(PartialEq, Eq)]
pub(crate) struct Identity {
    created: Option<std::time::SystemTime>,
    #[cfg(unix)]
    values: (u64, u64, u64, i64, i64, i64, i64),
    #[cfg(windows)]
    values: (u64, [u8; 16], u64, i64, i64),
}

impl Identity {
    /// Returns no reusable identity when the filesystem cannot supply a precise change stamp.
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
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows_sys::Win32::Storage::FileSystem::{
                FILE_BASIC_INFO, FILE_ID_INFO, FileBasicInfo, FileIdInfo,
                GetFileInformationByHandleEx,
            };
            const TICKS_PER_SECOND: i64 = 10_000_000;
            let mut basic = FILE_BASIC_INFO::default();
            let mut id = FILE_ID_INFO::default();
            // SAFETY: the live handle and correctly sized output buffers remain valid for both calls.
            let available = unsafe {
                GetFileInformationByHandleEx(
                    file.as_raw_handle(),
                    FileBasicInfo,
                    (&mut basic as *mut FILE_BASIC_INFO).cast(),
                    size_of::<FILE_BASIC_INFO>() as u32,
                ) != 0
                    && GetFileInformationByHandleEx(
                        file.as_raw_handle(),
                        FileIdInfo,
                        (&mut id as *mut FILE_ID_INFO).cast(),
                        size_of::<FILE_ID_INFO>() as u32,
                    ) != 0
            };
            if !available
                || basic.LastWriteTime % TICKS_PER_SECOND == 0
                || basic.ChangeTime % TICKS_PER_SECOND == 0
            {
                return Ok(None);
            }
            Ok(Some(Self {
                created: file.metadata()?.created().ok(),
                values: (
                    id.VolumeSerialNumber,
                    id.FileId.Identifier,
                    file.metadata()?.len(),
                    basic.LastWriteTime,
                    basic.ChangeTime,
                ),
            }))
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = file;
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests;
