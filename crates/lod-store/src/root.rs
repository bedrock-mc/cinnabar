//! The directory of every store and the quotas across them.
//!
//! Quotas mirror the server pack cache: a per-store cap and a global cap. Over the global
//! cap, whole stores go first, least recently opened first; stores not opened within
//! [`STORE_EXPIRY_MINUTES`] go regardless. A store another client holds open, writing or
//! reading, is never removed, and a store whose last open is unreadable counts as opened
//! now. Stores of older layout versions are removed first. What remains over a cap is
//! taken from the open store's far regions. Enforcement walks every store directory, so
//! callers run it at a coarser cadence than flushes.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use crate::{
    access::{MAX_OPENED_BYTES, OPENED_FILE, decode_opened, encode_opened},
    error::StoreError,
    identity::{StoreKey, is_store_dir_name},
    lock::{StoreLock, is_lock_file},
    minute::Minute,
    store::{Focus, Store, StoreOptions},
    table::{self, Durability},
};

/// Version of the store directory layout, named in the root directory (`v1`). A layout
/// change moves to a new directory; quota enforcement removes the stores of older
/// layouts and clearing the cache removes every layout. Every layout keeps the store
/// directory names and lock files of [`crate::lock`], so a client of any version can tell
/// a store another client holds open.
pub const STORE_FORMAT_VERSION: u32 = 1;
/// Default per-store cap: 1 GiB, about 250,000 explored chunks at every level.
pub const DEFAULT_STORE_CAP_BYTES: u64 = 1 << 30;
/// Default cap across all stores: 8 GiB.
pub const DEFAULT_GLOBAL_CAP_BYTES: u64 = 8 << 30;
/// Stores not opened for this long are removed: 90 days.
pub const STORE_EXPIRY_MINUTES: u32 = 90 * 24 * 60;

/// Suffix of a store directory renamed away for removal.
const REMOVING_SUFFIX: &str = ".removing";
/// Prefix of a layout version directory name.
const VERSION_DIR_PREFIX: &str = "v";

/// Byte caps and expiry for the stores of one root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quotas {
    /// Bytes one store may hold before its far regions are evicted.
    pub store_bytes: u64,
    /// Bytes every store of the root may hold together.
    pub global_bytes: u64,
    /// Minutes since its last open after which a store is removed.
    pub expiry_minutes: u32,
}

impl Default for Quotas {
    fn default() -> Self {
        Self {
            store_bytes: DEFAULT_STORE_CAP_BYTES,
            global_bytes: DEFAULT_GLOBAL_CAP_BYTES,
            expiry_minutes: STORE_EXPIRY_MINUTES,
        }
    }
}

/// One store directory found under a root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreSummary {
    /// The store directory.
    pub path: PathBuf,
    /// Bytes of the regular files below it.
    pub bytes: u64,
    /// When the store was last opened; `None` when that record is missing or unreadable.
    pub opened: Option<Minute>,
}

/// Why a store directory could not be removed.
#[derive(Debug, thiserror::Error)]
pub enum RemoveError {
    #[error("the store is open in a client")]
    InUse,
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// What quota enforcement did.
#[derive(Debug, Default)]
pub struct QuotaReport {
    /// Stores removed, with the bytes each held.
    pub removed: Vec<(PathBuf, u64)>,
    /// Stores that should have gone but could not be removed.
    pub kept: Vec<(PathBuf, RemoveError)>,
    /// Bytes freed from the open store's far regions.
    pub evicted_region_bytes: u64,
}

/// What clearing the cache did.
#[derive(Debug, Default)]
pub struct ClearReport {
    /// Directories removed: stores, and caches of older layout versions.
    pub removed: Vec<PathBuf>,
    /// Stores left in place, such as those another client holds open.
    pub kept: Vec<(PathBuf, RemoveError)>,
}

/// The versioned directory holding every store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreRoot {
    cache_dir: PathBuf,
    dir: PathBuf,
}

impl StoreRoot {
    /// The root inside `lod_cache_dir`, the install layout's distant terrain directory.
    #[must_use]
    pub fn new(lod_cache_dir: &Path) -> Self {
        Self {
            cache_dir: lod_cache_dir.to_owned(),
            dir: lod_cache_dir.join(version_dir_name(STORE_FORMAT_VERSION)),
        }
    }

    /// The versioned root directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The directory of `key`'s store.
    #[must_use]
    pub fn store_dir(&self, key: &StoreKey) -> PathBuf {
        self.dir.join(key.dir_name())
    }

    /// Opens `key`'s store; see [`Store::open`].
    pub fn open(
        &self,
        key: &StoreKey,
        now: Minute,
        options: StoreOptions,
    ) -> Result<Store, StoreError> {
        Store::open(&self.store_dir(key), now, options)
    }

    /// Every store directory, with its size and last open. Links are never followed.
    pub fn stores(&self) -> io::Result<Vec<StoreSummary>> {
        stores_in(&self.dir)
    }

    /// Applies `quotas` at `now`: removes stores of older layouts and expired stores,
    /// then whole stores, least recently opened first, while the cache is over its global
    /// cap, then evicts far regions of `current` (never removed itself) until it fits its
    /// per-store cap and the global cap. Stores of newer layouts count toward the global
    /// cap but are left to their own clients. `focus` orders the eviction. A store whose
    /// last open is unreadable is stamped with `now` when no client holds it, so it is
    /// neither lost at once nor kept forever.
    pub fn enforce(
        &self,
        quotas: Quotas,
        now: Minute,
        current: Option<&mut Store>,
        focus: Option<Focus>,
    ) -> Result<QuotaReport, StoreError> {
        remove_leftovers(&self.dir);
        let mut report = QuotaReport::default();
        let mut other_layout_bytes = 0;
        for (version, dir) in self.other_layouts()? {
            if version < STORE_FORMAT_VERSION {
                clear_layout(&dir, &mut report.removed, &mut report.kept)?;
            }
            other_layout_bytes += dir_bytes(&dir)?;
        }
        let current_dir = current.as_ref().map(|store| store.dir().to_owned());
        let mut others = self.stores()?;
        others.retain(|store| Some(&store.path) != current_dir.as_ref());
        let mut others: Vec<_> = others
            .into_iter()
            .map(|store| {
                let opened = store.opened.unwrap_or_else(|| {
                    stamp_opened(&store.path, now);
                    now
                });
                (opened, store)
            })
            .collect();
        others.sort_by_key(|(opened, store)| (*opened, store.path.clone()));
        let current_bytes = match &current {
            Some(store) => store.disk_bytes()?,
            None => 0,
        };
        let mut total = other_layout_bytes
            + current_bytes
            + others.iter().map(|(_, store)| store.bytes).sum::<u64>();
        for (opened, store) in others {
            let expired = now.minutes_since(opened) > quotas.expiry_minutes;
            if !expired && total <= quotas.global_bytes {
                continue;
            }
            match remove_store(&store.path) {
                Ok(()) => {
                    total -= store.bytes;
                    report.removed.push((store.path, store.bytes));
                }
                Err(error) => report.kept.push((store.path, error)),
            }
        }
        if let Some(store) = current {
            let over_global = total.saturating_sub(quotas.global_bytes);
            let cap = quotas
                .store_bytes
                .min(current_bytes.saturating_sub(over_global));
            report.evicted_region_bytes = store.evict_regions_from(current_bytes, cap, focus)?;
        }
        Ok(report)
    }

    /// Removes every store no client holds open, in this layout and every other one,
    /// reporting the rest.
    pub fn clear(&self) -> io::Result<ClearReport> {
        remove_leftovers(&self.dir);
        let mut removed = Vec::new();
        let mut kept = Vec::new();
        for store in self.stores()? {
            match remove_store(&store.path) {
                Ok(()) => removed.push((store.path, store.bytes)),
                Err(error) => kept.push((store.path, error)),
            }
        }
        for (_, dir) in self.other_layouts()? {
            clear_layout(&dir, &mut removed, &mut kept)?;
        }
        Ok(ClearReport {
            removed: removed.into_iter().map(|(path, _)| path).collect(),
            kept,
        })
    }

    /// Layout directories of other versions beside this root, by version.
    fn other_layouts(&self) -> io::Result<Vec<(u32, PathBuf)>> {
        let entries = match fs::read_dir(&self.cache_dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut layouts = Vec::new();
        for entry in entries {
            let entry = entry?;
            let version = entry.file_name().to_str().and_then(parse_version_dir_name);
            if let Some(version) = version
                && version != STORE_FORMAT_VERSION
                && entry.file_type()?.is_dir()
            {
                layouts.push((version, entry.path()));
            }
        }
        layouts.sort_unstable();
        Ok(layouts)
    }
}

/// The name of layout `version`'s directory.
fn version_dir_name(version: u32) -> String {
    format!("{VERSION_DIR_PREFIX}{version}")
}

/// Parses a name written by [`version_dir_name`]; `None` for any other name.
fn parse_version_dir_name(name: &str) -> Option<u32> {
    let version = name.strip_prefix(VERSION_DIR_PREFIX)?.parse().ok()?;
    (version_dir_name(version) == name).then_some(version)
}

/// Every store directory in `dir`, with its size and last open. Links are never followed.
fn stores_in(dir: &Path) -> io::Result<Vec<StoreSummary>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut stores = Vec::new();
    for entry in entries {
        let entry = entry?;
        let is_store = entry.file_type()?.is_dir()
            && entry.file_name().to_str().is_some_and(is_store_dir_name);
        if !is_store {
            continue;
        }
        let path = entry.path();
        let opened = match table::read_bounded(&path.join(OPENED_FILE), MAX_OPENED_BYTES)? {
            Some(Ok(bytes)) => decode_opened(&bytes).ok(),
            _ => None,
        };
        stores.push(StoreSummary {
            bytes: dir_bytes(&path)?,
            path,
            opened,
        });
    }
    stores.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    Ok(stores)
}

/// Removes the stores no client holds open in `dir`, the directory of another layout
/// version, then the directory itself once no store is left in it. Removed stores and
/// the directory go to `removed` with their bytes, stores left in place to `kept`.
fn clear_layout(
    dir: &Path,
    removed: &mut Vec<(PathBuf, u64)>,
    kept: &mut Vec<(PathBuf, RemoveError)>,
) -> io::Result<()> {
    remove_leftovers(dir);
    let kept_before = kept.len();
    for store in stores_in(dir)? {
        match remove_store(&store.path) {
            Ok(()) => removed.push((store.path, store.bytes)),
            Err(error) => kept.push((store.path, error)),
        }
    }
    if kept.len() == kept_before {
        let bytes = dir_bytes(dir)?;
        match fs::remove_dir_all(dir) {
            Ok(()) => removed.push((dir.to_owned(), bytes)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => kept.push((dir.to_owned(), error.into())),
        }
    }
    Ok(())
}

/// Removes directories in `dir` an interrupted removal renamed away; failures wait for
/// the next call.
fn remove_leftovers(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let leftover = entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(REMOVING_SUFFIX));
        if leftover {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

/// Records `now` as the last open of the store at `path` when no client holds it; a
/// failure leaves it unknown until the next enforcement.
fn stamp_opened(path: &Path, now: Minute) {
    if let Ok(Some(_lock)) = StoreLock::try_acquire(path) {
        let _ = table::replace(
            &path.join(OPENED_FILE),
            &encode_opened(now),
            Durability::Cached,
        );
    }
}

/// Removes the store directory at `path` unless a client holds it open, writing or
/// reading. The directory is renamed away under both locks first, so a client opening
/// the store meanwhile starts a fresh directory; where renaming fails (open files on
/// Windows), it is emptied in place.
pub fn remove_store(path: &Path) -> Result<(), RemoveError> {
    let lock = StoreLock::try_acquire_for_removal(path)?.ok_or(RemoveError::InUse)?;
    let mut removing = path.as_os_str().to_owned();
    removing.push(REMOVING_SUFFIX);
    let removing = PathBuf::from(removing);
    if fs::rename(path, &removing).is_ok() {
        drop(lock);
        fs::remove_dir_all(&removing)?;
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if is_lock_file(&entry.file_name()) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        } else {
            fs::remove_file(entry.path())?;
        }
    }
    drop(lock);
    fs::remove_dir_all(path)?;
    Ok(())
}

/// Bytes of the regular files below `dir`; links are counted as themselves and never
/// followed. A missing directory holds none.
pub fn dir_bytes(dir: &Path) -> io::Result<u64> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(error),
    };
    let mut bytes = 0;
    for entry in entries {
        let entry = entry?;
        let kind = entry.file_type()?;
        bytes += if kind.is_dir() {
            dir_bytes(&entry.path())?
        } else if kind.is_file() {
            // A file replaced or removed during the walk no longer counts.
            match entry.metadata() {
                Ok(metadata) => metadata.len(),
                Err(error) if error.kind() == io::ErrorKind::NotFound => 0,
                Err(error) => return Err(error),
            }
        } else {
            0
        };
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
