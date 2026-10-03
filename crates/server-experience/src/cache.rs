//! Immutable, digest-addressed bundle bytes; URLs and keys are never persisted.

use crate::{
    crypto,
    policy::{CACHE_QUOTA, MAX_BUNDLE_BYTES},
};
use anyhow::{Result, ensure};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};

/// The bundle cache under a client's per-user data root; cache seeding uses the same path.
pub fn objects_dir(user_data_root: &Path) -> PathBuf {
    user_data_root.join("server-experiences/v1/objects")
}

pub struct BundleCache {
    root: PathBuf,
    _lease: File,
    mutation: Mutex<()>,
}

impl BundleCache {
    /// Opens an owner-only objects directory with an exclusive process lease.
    pub fn open(root: &Path) -> Result<Self> {
        fs::create_dir_all(root)?;
        ensure!(
            !fs::symlink_metadata(root)?.file_type().is_symlink(),
            "cache root is a symlink"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
        }
        let root = root.canonicalize()?;
        let lease_path = root.join(".lock");
        reject_link(&lease_path)?;
        let lease = open_private(&lease_path, false)?;
        lease.try_lock()?;
        let cache = Self {
            root,
            _lease: lease,
            mutation: Mutex::new(()),
        };
        cache.evict(0)?;
        Ok(cache)
    }

    /// Waits for a retiring worker's lease without blocking cancellation or waiting forever.
    pub(crate) async fn open_wait(
        root: &Path,
        cancelled: &mut tokio::sync::watch::Receiver<bool>,
        deadline: tokio::time::Instant,
    ) -> Result<Self> {
        loop {
            ensure!(!*cancelled.borrow(), "download cancelled");
            match Self::open(root) {
                Ok(cache) => return Ok(cache),
                Err(error)
                    if matches!(
                        error.downcast_ref::<std::fs::TryLockError>(),
                        Some(std::fs::TryLockError::WouldBlock)
                    ) => {}
                Err(error) => return Err(error),
            }
            tokio::select! {
                biased;
                _ = cancelled.changed() => anyhow::bail!("download cancelled"),
                _ = tokio::time::sleep_until(deadline) => anyhow::bail!("cache lease deadline exceeded"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
            }
        }
    }

    /// Rehashes cache hits so corrupt local data never reaches the loader.
    pub fn read(&self, digest: &str) -> Result<Option<Vec<u8>>> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow::anyhow!("cache mutation lock poisoned"))?;
        self.read_locked(digest)
    }

    /// Reads an object while publication and eviction share the caller's lock.
    fn read_locked(&self, digest: &str) -> Result<Option<Vec<u8>>> {
        let path = self.object_path(digest)?;
        reject_link(&path)?;
        let file = match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        ensure!(file.metadata()?.is_file(), "cache object is not a file");
        let mut bytes = Vec::new();
        (&file)
            .take((MAX_BUNDLE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BUNDLE_BYTES || crypto::digest(&bytes) != digest {
            fs::remove_file(path)?;
            return Ok(None);
        }
        file.set_times(std::fs::FileTimes::new().set_modified(SystemTime::now()))?;
        Ok(Some(bytes))
    }

    /// Verifies first, then atomically publishes a private file under its digest.
    pub fn publish(&self, digest: &str, bytes: &[u8]) -> Result<()> {
        self.publish_with(digest, bytes, |_| {})
    }

    /// Publishes one object while an observer can witness its temporary-file lifetime.
    fn publish_with(&self, digest: &str, bytes: &[u8], created: impl FnOnce(&Path)) -> Result<()> {
        let _guard = self
            .mutation
            .lock()
            .map_err(|_| anyhow::anyhow!("cache mutation lock poisoned"))?;
        ensure!(
            bytes.len() <= MAX_BUNDLE_BYTES && crypto::digest(bytes) == digest,
            "bundle digest mismatch"
        );
        let destination = self.object_path(digest)?;
        if self.read_locked(digest)?.is_some() {
            return Ok(());
        }
        self.evict(bytes.len() as u64)?;
        let temporary = Temporary(
            self.root
                .join(format!(".download-{}", crypto::challenge()?)),
        );
        let mut file = open_private(&temporary.0, true)?;
        created(&temporary.0);
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary.0, destination)?;
        Ok(())
    }

    /// Maps only validated digests into filenames.
    fn object_path(&self, digest: &str) -> Result<PathBuf> {
        crypto::fixed_hex::<32>(digest)?;
        Ok(self.root.join(format!("{digest}.cxb")))
    }

    /// Evicts least-recently-read objects; live runtimes retain their own bytes.
    fn evict(&self, incoming: u64) -> Result<()> {
        let mut entries = Vec::new();
        let mut used = incoming;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let name = entry.file_name();
            if let Some(partial) = name
                .to_str()
                .and_then(|name| name.strip_prefix(".download-"))
            {
                crypto::fixed_hex::<32>(partial)?;
                ensure!(entry.file_type()?.is_file(), "unsafe partial cache object");
                fs::remove_file(entry.path())?;
                continue;
            }
            let Some(name) = name.to_str().and_then(|name| name.strip_suffix(".cxb")) else {
                continue;
            };
            crypto::fixed_hex::<32>(name)?;
            ensure!(entry.file_type()?.is_file(), "unsafe cache object");
            let metadata = entry.metadata()?;
            used = used
                .checked_add(metadata.len())
                .ok_or_else(|| anyhow::anyhow!("cache size overflow"))?;
            entries.push((metadata.modified()?, entry.path(), metadata.len()));
        }
        entries.sort_by_key(|entry| entry.0);
        for (_, path, size) in entries {
            if used <= CACHE_QUOTA {
                break;
            }
            fs::remove_file(path)?;
            used -= size;
        }
        ensure!(used <= CACHE_QUOTA, "bundle exceeds cache quota");
        Ok(())
    }
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    /// Removes an unpublished partial file on every error path.
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Rejects existing links before touching a private cache entry.
fn reject_link(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "unsafe cache file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

/// Creates private files without truncating a concurrently published object.
fn open_private(path: &Path, exclusive: bool) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if exclusive {
        options.create_new(true);
    } else {
        options.create(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_concurrent_publications_preserve_active_temporary_files() {
        let root = tempfile::tempdir().unwrap();
        let cache = BundleCache::open(root.path()).unwrap();
        let (entered, waiting) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        let (started, attempting) = std::sync::mpsc::channel();
        let (finished, completed) = std::sync::mpsc::channel();
        let (first, second) = std::thread::scope(|scope| {
            let cache_ref = &cache;
            let first = scope.spawn(move || {
                cache_ref.publish_with(&crypto::digest(b"first"), b"first", |_| {
                    entered.send(()).unwrap();
                    resume.recv().unwrap();
                })
            });
            waiting.recv().unwrap();
            let second = scope.spawn(|| {
                started.send(()).unwrap();
                let result = cache.publish(&crypto::digest(b"second"), b"second");
                finished.send(()).unwrap();
                result
            });
            attempting.recv().unwrap();
            let _ = completed.recv_timeout(std::time::Duration::from_millis(50));
            release.send(()).unwrap();
            (first.join().unwrap(), second.join().unwrap())
        });
        assert!(
            first.is_ok(),
            "active publication lost its temporary path: {first:?}"
        );
        assert!(second.is_ok());
        assert_eq!(
            cache.read(&crypto::digest(b"first")).unwrap().unwrap(),
            b"first"
        );
        assert_eq!(
            cache.read(&crypto::digest(b"second")).unwrap().unwrap(),
            b"second"
        );
    }

    #[test]
    fn cache_hit_refreshes_timestamp_with_writable_attributes() {
        let root = tempfile::tempdir().unwrap();
        let cache = BundleCache::open(root.path()).unwrap();
        let bytes = b"cached bundle";
        let digest = crypto::digest(bytes);
        cache.publish(&digest, bytes).unwrap();
        assert_eq!(
            cache.read(&digest).unwrap().as_deref(),
            Some(bytes.as_slice())
        );
    }

    #[test]
    fn replacement_waits_for_retiring_lease_and_can_be_cancelled() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let root = tempfile::tempdir().unwrap();
            let old = BundleCache::open(root.path()).unwrap();
            let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
            let wait = BundleCache::open_wait(
                root.path(),
                &mut cancelled,
                tokio::time::Instant::now() + std::time::Duration::from_secs(1),
            );
            let release = async {
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                drop(old);
            };
            let (replacement, ()) = tokio::join!(wait, release);
            let replacement = replacement.unwrap();
            let timeout = BundleCache::open_wait(
                root.path(),
                &mut cancelled,
                tokio::time::Instant::now() + std::time::Duration::from_millis(20),
            )
            .await;
            assert!(timeout.err().unwrap().to_string().contains("deadline"));
            let wait = BundleCache::open_wait(
                root.path(),
                &mut cancelled,
                tokio::time::Instant::now() + std::time::Duration::from_secs(1),
            );
            let revoke = async {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                cancel.send(true).unwrap();
            };
            let (result, ()) = tokio::join!(wait, revoke);
            assert!(result.err().unwrap().to_string().contains("cancelled"));
            drop(replacement);
        });
    }
}
