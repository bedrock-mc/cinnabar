//! Persistent original archives; identity and SHA-256 are verified on every hit.
//!
//! Object names contain only digests. URLs and content keys never reach disk.

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt};
use cap_std::{
    ambient_authority,
    fs::{Dir, OpenOptions},
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{self, Read, Write},
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::SystemTime,
};
use uuid::Uuid;

/// Maximum original archive bytes retained by the native cache.
pub const DEFAULT_CACHE_QUOTA: u64 = 512 * 1024 * 1024;
const MAX_CACHE_OBJECTS: usize = 4096;
const OBJECT_SUFFIX: &str = ".mcpack";

/// Identifies the exact advertised archive; a size change creates a different entry.
#[derive(Clone, Debug)]
pub struct ArchiveCacheKey {
    id: Uuid,
    version: String,
    size: u64,
}

impl ArchiveCacheKey {
    /// Rejects identities that cannot represent a bounded original archive.
    pub fn new(id: Uuid, version: &str, size: u64) -> io::Result<Self> {
        if id.is_nil()
            || version.is_empty()
            || version.len() > bedrock_packs::MAX_PACK_VERSION_BYTES
            || size == 0
            || size > crate::MAX_LOCAL_ARCHIVE_BYTES as u64
        {
            return Err(invalid("invalid archive cache identity"));
        }
        Ok(Self {
            id,
            version: version.into(),
            size,
        })
    }

    /// Derives a path component without including server-controlled text.
    fn digest(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(self.id.as_bytes());
        hash.update((self.version.len() as u64).to_be_bytes());
        hash.update(self.size.to_be_bytes());
        hash.update(self.version.as_bytes());
        format!("{:x}", hash.finalize())
    }
}

/// A quota-managed directory lease. Call its filesystem methods on a blocking worker.
/// The `cancelled` flag lets dropped join futures stop a worker before publication.
pub struct ArchiveCache {
    dir: Dir,
    _lease: File,
    quota: u64,
    operations: Mutex<()>,
}

impl ArchiveCache {
    /// Creates an isolated native namespace under the configured pack-cache directory.
    /// An already leased namespace disables caching for the competing caller.
    pub fn open(root: &Path, quota: u64) -> io::Result<Self> {
        std::fs::create_dir_all(root)?;
        let parent = Dir::open_ambient_dir(root, ambient_authority())?;
        let builder = cap_std::fs::DirBuilder::new();
        #[cfg(unix)]
        let builder = {
            use cap_std::fs::DirBuilderExt;
            let mut builder = builder;
            builder.mode(0o700);
            builder
        };
        match parent.create_dir_with("native-v1", &builder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        if !parent.symlink_metadata("native-v1")?.is_dir() {
            return Err(invalid("pack cache namespace is not a directory"));
        }
        let dir = parent.open_dir("native-v1")?;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .create(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lease = dir.open_with(".packcache.lock", &options)?.into_std();
        if !lease.metadata()?.is_file() {
            return Err(invalid("pack cache lease is not a regular file"));
        }
        lease.try_lock().map_err(io::Error::other)?;
        let cache = Self {
            dir,
            _lease: lease,
            quota,
            operations: Mutex::new(()),
        };
        cache.remove_partial_files()?;
        cache.prune()?;
        Ok(cache)
    }

    /// Loads an exact-size, hash-checked archive or removes it and reports a cache miss.
    /// `now` is the caller-supplied modification time used to refresh eviction age.
    pub fn load(
        &self,
        key: &ArchiveCacheKey,
        cancelled: &AtomicBool,
        now: SystemTime,
    ) -> io::Result<Option<Vec<u8>>> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| io::Error::other("pack cache lock poisoned"))?;
        check_cancelled(cancelled)?;
        let prefix = format!("{}-", key.digest());
        for entry in self.objects()? {
            if !entry.name.starts_with(&prefix) {
                continue;
            }
            let result = self.read_object(&entry, key, cancelled, now);
            match result {
                Ok(Some(bytes)) => return Ok(Some(bytes)),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => return Err(error),
                _ => {
                    let _ = self.dir.remove_file(&entry.name);
                }
            }
        }
        Ok(None)
    }

    /// Publishes only complete matching archives, keeping the original ZIP bytes unchanged.
    /// Cancellation and failed writes remove the private temporary object.
    pub fn store(
        &self,
        key: &ArchiveCacheKey,
        bytes: &[u8],
        cancelled: &AtomicBool,
    ) -> io::Result<()> {
        let _guard = self
            .operations
            .lock()
            .map_err(|_| io::Error::other("pack cache lock poisoned"))?;
        check_cancelled(cancelled)?;
        if bytes.len() as u64 != key.size || key.size > self.quota {
            return Err(invalid("archive exceeds cache quota or advertised size"));
        }
        crate::verify_archive_identity(bytes, key.id, &key.version).map_err(io::Error::other)?;
        let identity = key.digest();
        let digest = Sha256::digest(bytes);
        let name = format!("{identity}-{digest:x}{OBJECT_SUFFIX}");
        let mut objects = self.objects()?;
        objects.sort_unstable_by_key(|entry| entry.modified);
        let mut used = objects
            .iter()
            .try_fold(0_u64, |sum, entry| sum.checked_add(entry.size))
            .ok_or_else(|| invalid("pack cache sizes overflow"))?;
        for entry in &objects {
            if entry.name == name
                && self
                    .read_object(entry, key, cancelled, entry.modified)?
                    .is_some()
            {
                return Ok(());
            }
        }
        let mut count = objects.len();
        for entry in objects {
            if used
                .checked_add(key.size)
                .is_some_and(|total| total <= self.quota)
                && count < MAX_CACHE_OBJECTS
                && !entry.name.starts_with(&format!("{identity}-"))
            {
                continue;
            }
            count -= 1;
            self.dir.remove_file(&entry.name)?;
            used = used.saturating_sub(entry.size);
        }
        check_cancelled(cancelled)?;
        let temporary = format!(".partial-{}", Uuid::new_v4());
        let cleanup = Temporary {
            dir: &self.dir,
            name: temporary.clone(),
        };
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        #[cfg(unix)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = self.dir.open_with(&temporary, &options)?;
        for chunk in bytes.chunks(64 * 1024) {
            check_cancelled(cancelled)?;
            file.write_all(chunk)?;
        }
        file.flush()?;
        drop(file);
        check_cancelled(cancelled)?;
        self.dir.rename(&temporary, &self.dir, &name)?;
        drop(cleanup);
        Ok(())
    }

    /// Reads through a no-follow directory capability and validates content before use.
    fn read_object(
        &self,
        entry: &Object,
        key: &ArchiveCacheKey,
        cancelled: &AtomicBool,
        now: SystemTime,
    ) -> io::Result<Option<Vec<u8>>> {
        if entry.size != key.size {
            return Ok(None);
        }
        let mut options = OpenOptions::new();
        options.read(true).follow(FollowSymlinks::No);
        let mut file = self.dir.open_with(&entry.name, &options)?;
        if !file.metadata()?.is_file() || file.metadata()?.len() != key.size {
            return Ok(None);
        }
        let mut bytes = Vec::new();
        let mut scratch = [0_u8; 64 * 1024];
        loop {
            check_cancelled(cancelled)?;
            let length = file.read(&mut scratch)?;
            if length == 0 {
                break;
            }
            if bytes.len() as u64 + length as u64 > key.size {
                return Ok(None);
            }
            bytes.try_reserve(length).map_err(io::Error::other)?;
            bytes.extend_from_slice(&scratch[..length]);
        }
        if bytes.len() as u64 != key.size || format!("{:x}", Sha256::digest(&bytes)) != entry.digest
        {
            return Ok(None);
        }
        if crate::verify_archive_identity(&bytes, key.id, &key.version).is_err() {
            return Ok(None);
        }
        let _ = file.into_std().set_modified(now);
        Ok(Some(bytes))
    }

    /// Lists only owned regular objects; unrelated files and symlinks are untouched.
    fn objects(&self) -> io::Result<Vec<Object>> {
        let mut objects = Vec::new();
        for entry in self.dir.entries()? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Some((identity, digest)) = name
                .strip_suffix(OBJECT_SUFFIX)
                .and_then(|name| name.split_once('-'))
            else {
                continue;
            };
            if identity.len() != 64
                || digest.len() != 64
                || !identity
                    .bytes()
                    .chain(digest.bytes())
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                continue;
            }
            let metadata = self.dir.symlink_metadata(name)?;
            if !metadata.is_file() {
                continue;
            }
            if objects.len() >= MAX_CACHE_OBJECTS {
                return Err(invalid("pack cache object limit exceeded"));
            }
            objects.push(Object {
                name: name.into(),
                digest: digest.into(),
                size: metadata.len(),
                modified: metadata.modified()?.into_std(),
            });
        }
        Ok(objects)
    }

    /// Enforces the current quota when reopening a cache created with a larger budget.
    fn prune(&self) -> io::Result<()> {
        let mut objects = self.objects()?;
        objects.sort_unstable_by_key(|entry| entry.modified);
        let mut used = objects
            .iter()
            .try_fold(0_u64, |sum, entry| sum.checked_add(entry.size))
            .ok_or_else(|| invalid("pack cache sizes overflow"))?;
        for entry in objects {
            if used <= self.quota {
                break;
            }
            self.dir.remove_file(entry.name)?;
            used = used.saturating_sub(entry.size);
        }
        Ok(())
    }

    /// Removes abandoned temporaries while holding the exclusive namespace lease.
    fn remove_partial_files(&self) -> io::Result<()> {
        for entry in self.dir.entries()? {
            let entry = entry?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(".partial-"))
                && entry.file_type()?.is_file()
            {
                self.dir.remove_file(entry.file_name())?;
            }
        }
        Ok(())
    }
}

struct Object {
    name: String,
    digest: String,
    size: u64,
    modified: SystemTime,
}
struct Temporary<'a> {
    dir: &'a Dir,
    name: String,
}
impl Drop for Temporary<'_> {
    fn drop(&mut self) {
        let _ = self.dir.remove_file(&self.name);
    }
}

/// Interrupts bounded filesystem loops when their owning join is dropped.
fn check_cancelled(cancelled: &AtomicBool) -> io::Result<()> {
    if cancelled.load(Ordering::Relaxed) {
        Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "pack cache operation cancelled",
        ))
    } else {
        Ok(())
    }
}

/// Reports a stable error without including server metadata or paths.
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

#[cfg(test)]
mod tests;
