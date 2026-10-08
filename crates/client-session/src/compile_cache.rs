//! Join-time compile results and server-pack archives persisted across launches, under one bound.

use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};

use sha2::{Digest, Sha256};

const MAGIC: [u8; 8] = *b"CNBRCMP1";
const HEADER: usize = MAGIC.len() + 32 + 8;
const HASH: usize = 32;
const EXTENSION: &str = "bin";
/// A writer that died mid-entry leaves its temporary file; later writers sweep it after this.
const STALE_TEMPORARY: Duration = Duration::from_secs(3600);

/// A size-bounded on-disk store of compiled subscriber payloads, least recently used first out.
#[derive(Clone, Debug)]
pub struct CompileCache {
    dir: PathBuf,
    max_bytes: u64,
}

/// Starts a cache key that also names the build, so a new client never reads an older layout.
pub fn cache_key(subscriber: &str) -> Sha256 {
    let mut key = Sha256::new();
    part(&mut key, env!("CARGO_PKG_VERSION").as_bytes());
    part(&mut key, &build_identity());
    part(&mut key, subscriber.as_bytes());
    key
}

/// Length framing keeps adjacent key parts from aliasing.
pub fn part(key: &mut Sha256, bytes: &[u8]) {
    key.update((bytes.len() as u64).to_le_bytes());
    key.update(bytes);
}

/// Size and modification time of the running executable; any rebuild changes it.
fn build_identity() -> [u8; 16] {
    static IDENTITY: std::sync::OnceLock<[u8; 16]> = std::sync::OnceLock::new();
    *IDENTITY.get_or_init(|| {
        let metadata = std::env::current_exe().and_then(fs::metadata).ok();
        let modified = metadata
            .as_ref()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(|time| time.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |since| since.as_nanos() as u64);
        let length = metadata.map_or(0, |metadata| metadata.len());
        let mut identity = [0; 16];
        identity[..8].copy_from_slice(&length.to_le_bytes());
        identity[8..].copy_from_slice(&modified.to_le_bytes());
        identity
    })
}

/// Keys an archive by its offer identity alone: archives outlive client builds, unlike compiles.
fn archive_key(identity: protocol::ResourcePackIdentity<'_>) -> [u8; 32] {
    let mut key = Sha256::new();
    part(&mut key, b"server-pack-archive");
    part(&mut key, identity.pack_id.as_bytes());
    part(&mut key, identity.version.as_bytes());
    part(&mut key, &identity.size.to_le_bytes());
    key.finalize().into()
}

impl protocol::ResourcePackStore for CompileCache {
    fn load(&self, identity: protocol::ResourcePackIdentity<'_>) -> Option<Vec<u8>> {
        CompileCache::load(self, &archive_key(identity))
    }

    fn store(&self, identity: protocol::ResourcePackIdentity<'_>, archive: &[u8]) {
        CompileCache::store(self, &archive_key(identity), archive);
    }
}

impl CompileCache {
    #[must_use]
    pub fn new(dir: PathBuf, max_bytes: u64) -> Self {
        Self { dir, max_bytes }
    }

    /// Returns the stored payload, or `None` for a miss; a damaged entry is deleted.
    pub fn load(&self, key: &[u8; 32]) -> Option<Vec<u8>> {
        let path = self.entry(key);
        let mut bytes = fs::read(&path).ok()?;
        if !valid(&bytes, key) {
            tracing::warn!(path = %path.display(), "discarding a damaged compile cache entry");
            let _ = fs::remove_file(&path);
            return None;
        }
        // Recency for eviction; a read-only cache still serves the hit.
        let _ = fs::File::options()
            .append(true)
            .open(&path)
            .and_then(|file| file.set_modified(SystemTime::now()));
        bytes.truncate(bytes.len() - HASH);
        bytes.drain(..HEADER);
        Some(bytes)
    }

    /// Writes the entry atomically, then evicts the least recently used past the size bound.
    pub fn store(&self, key: &[u8; 32], payload: &[u8]) {
        if (HEADER + payload.len() + HASH) as u64 > self.max_bytes {
            return;
        }
        if let Err(error) = self.write(key, payload) {
            tracing::warn!(%error, dir = %self.dir.display(), "compile cache entry was not stored");
            return;
        }
        self.evict(&self.entry(key));
    }

    fn entry(&self, key: &[u8; 32]) -> PathBuf {
        let name: String = key.iter().map(|byte| format!("{byte:02x}")).collect();
        self.dir.join(name).with_extension(EXTENSION)
    }

    /// No fsync: a torn entry fails validation on load and is treated as a miss.
    fn write(&self, key: &[u8; 32], payload: &[u8]) -> std::io::Result<()> {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        fs::create_dir_all(&self.dir)?;
        let path = self.entry(key);
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let temporary = path.with_extension(format!("{}.{sequence}.tmp", std::process::id()));
        let mut hash = Sha256::new();
        let mut file = fs::File::create(&temporary)?;
        let length = (payload.len() as u64).to_le_bytes();
        for chunk in [&MAGIC[..], key, &length, payload] {
            hash.update(chunk);
            file.write_all(chunk)?;
        }
        file.write_all(&hash.finalize())?;
        drop(file);
        fs::rename(&temporary, &path).inspect_err(|_| {
            let _ = fs::remove_file(&temporary);
        })
    }

    fn evict(&self, keep: &Path) {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return;
        };
        let now = SystemTime::now();
        let mut entries: Vec<_> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let path = entry.path();
                let metadata = entry.metadata().ok()?;
                let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
                if path.extension().is_some_and(|extension| extension == "tmp") {
                    if now.duration_since(modified).unwrap_or_default() > STALE_TEMPORARY {
                        let _ = fs::remove_file(&path);
                    }
                    return None;
                }
                (metadata.is_file() && path.extension().is_some_and(|ext| ext == EXTENSION))
                    .then_some((modified, metadata.len(), path))
            })
            .collect();
        let mut total: u64 = entries.iter().map(|(_, length, _)| length).sum();
        entries.sort();
        for (_, length, path) in entries {
            if total <= self.max_bytes {
                break;
            }
            if path != keep && fs::remove_file(&path).is_ok() {
                total -= length;
            }
        }
    }
}

fn valid(bytes: &[u8], key: &[u8; 32]) -> bool {
    let Some(body) = bytes.len().checked_sub(HASH) else {
        return false;
    };
    body >= HEADER
        && bytes[..8] == MAGIC
        && bytes[8..40] == key[..]
        && u64::from_le_bytes(bytes[40..48].try_into().unwrap()) == (body - HEADER) as u64
        && Sha256::digest(&bytes[..body])[..] == bytes[body..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(byte: u8) -> [u8; 32] {
        [byte; 32]
    }

    fn age(cache: &CompileCache, key: &[u8; 32], seconds: u64) {
        fs::File::options()
            .append(true)
            .open(cache.entry(key))
            .unwrap()
            .set_modified(SystemTime::now() - Duration::from_secs(seconds))
            .unwrap();
    }

    #[test]
    fn a_stored_entry_loads_back_and_other_keys_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().join("compiled"), 1 << 20);
        cache.store(&key(1), b"payload");
        assert_eq!(cache.load(&key(1)).as_deref(), Some(&b"payload"[..]));
        assert_eq!(cache.load(&key(2)), None);
    }

    #[test]
    fn a_damaged_entry_is_a_miss_and_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().to_owned(), 1 << 20);
        for damage in [
            (|bytes: &mut Vec<u8>| bytes[50] ^= 1) as fn(&mut Vec<u8>),
            |bytes| bytes.truncate(bytes.len() - 1),
            |bytes| bytes.truncate(3),
        ] {
            cache.store(&key(1), b"payload bytes");
            let path = cache.entry(&key(1));
            let mut bytes = fs::read(&path).unwrap();
            damage(&mut bytes);
            fs::write(&path, bytes).unwrap();
            assert_eq!(cache.load(&key(1)), None);
            assert!(!path.exists());
        }
    }

    #[test]
    fn an_entry_renamed_to_another_key_is_a_miss() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().to_owned(), 1 << 20);
        cache.store(&key(1), b"payload");
        fs::rename(cache.entry(&key(1)), cache.entry(&key(2))).unwrap();
        assert_eq!(cache.load(&key(2)), None);
    }

    #[test]
    fn eviction_drops_the_least_recently_used_entries_first() {
        let dir = tempfile::tempdir().unwrap();
        let entry = (HEADER + 100 + HASH) as u64;
        let cache = CompileCache::new(dir.path().to_owned(), entry * 2);
        cache.store(&key(1), &[1; 100]);
        cache.store(&key(2), &[2; 100]);
        age(&cache, &key(1), 20);
        age(&cache, &key(2), 10);
        // A hit refreshes the older entry, so the other one is evicted.
        assert!(cache.load(&key(1)).is_some());
        cache.store(&key(3), &[3; 100]);
        assert!(cache.load(&key(1)).is_some());
        assert_eq!(cache.load(&key(2)), None);
        assert!(cache.load(&key(3)).is_some());
    }

    #[test]
    fn an_entry_larger_than_the_bound_is_not_stored() {
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().to_owned(), 64);
        cache.store(&key(1), &[0; 64]);
        assert_eq!(cache.load(&key(1)), None);
    }

    #[test]
    fn a_stored_archive_answers_only_its_exact_offer_identity() {
        use protocol::{ResourcePackIdentity, ResourcePackStore};
        let dir = tempfile::tempdir().unwrap();
        let store: &dyn ResourcePackStore = &CompileCache::new(dir.path().to_owned(), 1 << 20);
        let id = uuid::Uuid::from_u128(7);
        let identity = ResourcePackIdentity {
            pack_id: id,
            version: "1.0.0",
            size: 7,
        };
        store.store(identity, b"archive");
        assert_eq!(store.load(identity).as_deref(), Some(&b"archive"[..]));
        for other in [
            ResourcePackIdentity {
                version: "1.0.1",
                ..identity
            },
            ResourcePackIdentity {
                size: 8,
                ..identity
            },
            ResourcePackIdentity {
                pack_id: uuid::Uuid::from_u128(8),
                ..identity
            },
        ] {
            assert_eq!(store.load(other), None);
        }
    }

    #[test]
    fn a_corrupted_archive_is_a_miss() {
        use protocol::{ResourcePackIdentity, ResourcePackStore};
        let dir = tempfile::tempdir().unwrap();
        let cache = CompileCache::new(dir.path().to_owned(), 1 << 20);
        let store: &dyn ResourcePackStore = &cache;
        let identity = ResourcePackIdentity {
            pack_id: uuid::Uuid::from_u128(7),
            version: "1.0.0",
            size: 7,
        };
        store.store(identity, b"archive");
        let path = cache.entry(&archive_key(identity));
        let mut bytes = fs::read(&path).unwrap();
        bytes[HEADER] ^= 1;
        fs::write(&path, bytes).unwrap();
        assert_eq!(store.load(identity), None);
    }

    #[test]
    fn keys_change_with_each_framed_part() {
        let digest = |parts: &[&[u8]]| {
            let mut key = cache_key("entities");
            for bytes in parts {
                part(&mut key, bytes);
            }
            key.finalize()
        };
        assert_ne!(digest(&[b"ab", b"c"]), digest(&[b"a", b"bc"]));
        assert_ne!(
            cache_key("entities").finalize(),
            cache_key("glyphs").finalize()
        );
    }
}
