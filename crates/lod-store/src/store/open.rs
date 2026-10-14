//! Opening a store: lock, registry, catalog and access table, with recovery from
//! unreadable tables and from candidates a crash left behind.

use std::{
    fs, io,
    path::Path,
    sync::{Arc, RwLock},
};

use lod::registry::MaterialRegistry;

use super::{
    MATERIALS_FILE, MAX_MATERIALS_BYTES, ResetReason, Store, StoreEvent, StoreOptions,
    registry::RegistryFile, table_error,
};
use crate::{
    access::{
        ACCESS_FILE, AccessTable, MAX_ACCESS_BYTES, MAX_OPENED_BYTES, OPENED_FILE, decode_opened,
        encode_opened,
    },
    epochs::{CATALOG_FILE, EpochCatalog, EpochId, EpochRef, EpochState, MAX_CATALOG_BYTES},
    error::StoreError,
    identity::Dimension,
    lock::{StoreLock, is_lock_file},
    minute::Minute,
    region_file::Access,
    table::{self, TableError},
};

impl Store {
    /// Opens or creates the store in `dir`. The first opener takes its lock and may
    /// write; later openers get a read-only store that never writes and holds a reader's
    /// lock, so the store is not removed under it. A writer resets an
    /// unreadable registry by deleting every epoch, rebuilds an unreadable catalog from
    /// the epoch directories, and deletes candidates and stray epoch directories a crash
    /// left behind. `now` is recorded as the store's last open.
    pub fn open(dir: &Path, now: Minute, options: StoreOptions) -> Result<Self, StoreError> {
        fs::create_dir_all(dir)?;
        let (lock, access) = match StoreLock::try_acquire(dir)? {
            Some(lock) => (Some(lock), Access::ReadWrite),
            // Without a reader's lock the store is being removed, and simply reads as it
            // disappears.
            None => (StoreLock::try_acquire_reader(dir)?, Access::ReadOnly),
        };
        let mut events = Vec::new();

        let materials = dir.join(MATERIALS_FILE);
        let registry_table = match load(&materials, MAX_MATERIALS_BYTES)? {
            None => MaterialRegistry::new(),
            Some(Ok(bytes)) => match MaterialRegistry::decode(&bytes) {
                Ok(registry) => registry,
                Err(error) if access == Access::ReadOnly => return Err(error.into()),
                Err(error) => {
                    reset(dir)?;
                    events.push(StoreEvent::Reset(ResetReason::Registry(error)));
                    MaterialRegistry::new()
                }
            },
            Some(Err(error)) if access == Access::ReadOnly => {
                return Err(table_error(&materials, error).into());
            }
            Some(Err(error)) => {
                reset(dir)?;
                events.push(StoreEvent::Reset(ResetReason::File(table_error(
                    &materials, error,
                ))));
                MaterialRegistry::new()
            }
        };

        let saved = registry_table.len();
        let catalog_path = dir.join(CATALOG_FILE);
        let (mut catalog, mut catalog_dirty) = match load(&catalog_path, MAX_CATALOG_BYTES)? {
            Some(Ok(bytes)) => match EpochCatalog::decode(&bytes) {
                Ok(catalog) => (catalog, false),
                Err(error) => {
                    events.push(StoreEvent::CatalogRebuilt(table_error(
                        &catalog_path,
                        error,
                    )));
                    (EpochCatalog::rebuilt(epoch_dirs(dir)?, now), true)
                }
            },
            Some(Err(error)) => {
                events.push(StoreEvent::CatalogRebuilt(table_error(
                    &catalog_path,
                    error,
                )));
                (EpochCatalog::rebuilt(epoch_dirs(dir)?, now), true)
            }
            None => {
                let found = epoch_dirs(dir)?;
                let dirty = !found.is_empty();
                (EpochCatalog::rebuilt(found, now), dirty)
            }
        };

        let access_path = dir.join(ACCESS_FILE);
        let mut table = match load(&access_path, MAX_ACCESS_BYTES)? {
            Some(Ok(bytes)) => AccessTable::decode(&bytes).unwrap_or_else(|error| {
                events.push(StoreEvent::AccessReset(table_error(&access_path, error)));
                AccessTable::new()
            }),
            Some(Err(error)) => {
                events.push(StoreEvent::AccessReset(table_error(&access_path, error)));
                AccessTable::new()
            }
            None => AccessTable::new(),
        };
        let opened = match load(&dir.join(OPENED_FILE), MAX_OPENED_BYTES)? {
            Some(Ok(bytes)) => decode_opened(&bytes).map_or(now, |opened| opened.max(now)),
            _ => now,
        };

        if access == Access::ReadWrite {
            catalog_dirty |= drop_leftovers(dir, &mut catalog, &mut table, &mut events)?;
        }
        let mut store = Self {
            dir: dir.to_owned(),
            _lock: lock,
            access,
            options,
            registry: Arc::new(RegistryFile::new(
                Arc::new(RwLock::new(registry_table)),
                materials,
                saved,
            )),
            opened,
            catalog,
            catalog_dirty,
            table,
            table_dirty: false,
            epochs: Default::default(),
            detached: Default::default(),
            sessions: Default::default(),
            hidden_regions: 0,
            newly_hidden: Vec::new(),
            leading_changes: 0,
            events,
        };
        if access == Access::ReadWrite {
            // Recorded at once, so quota ranking sees this open even after a crash.
            store.write_table(OPENED_FILE, &encode_opened(opened));
            store.save_tables();
        }
        Ok(store)
    }
}

/// A table file's bytes; `None` when missing.
fn load(path: &Path, max: usize) -> io::Result<Option<Result<Vec<u8>, TableError>>> {
    table::read_bounded(path, max)
}

/// Deletes everything in the store directory `dir` but its lock files: every dimension
/// directory and every table, including any a later table adds.
fn reset(dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if is_lock_file(&entry.file_name()) {
            continue;
        }
        if entry.file_type()?.is_dir() {
            remove_dir(&entry.path())?;
        } else {
            table::remove_if_present(&entry.path())?;
        }
    }
    Ok(())
}

/// Every epoch directory below the store directory `dir`.
pub(super) fn epoch_dirs(dir: &Path) -> io::Result<Vec<EpochRef>> {
    let mut epochs = Vec::new();
    for dimension in read_dir_names(dir)? {
        let Some(dimension) = Dimension::from_dir_name(&dimension) else {
            continue;
        };
        for epoch in read_dir_names(&dir.join(dimension.dir_name()))? {
            if let Some(id) = EpochId::from_dir_name(&epoch) {
                epochs.push(EpochRef { dimension, id });
            }
        }
    }
    epochs.sort_unstable();
    Ok(epochs)
}

/// Names of the real directories (never links) in `dir`; none when it is missing.
fn read_dir_names(dir: &Path) -> io::Result<Vec<String>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry?;
        if entry.file_type()?.is_dir()
            && let Ok(name) = entry.file_name().into_string()
        {
            names.push(name);
        }
    }
    Ok(names)
}

/// Deletes candidates a crash left in the catalog and epoch directories it does not
/// list; true when the catalog changed.
fn drop_leftovers(
    dir: &Path,
    catalog: &mut EpochCatalog,
    table: &mut AccessTable,
    events: &mut Vec<StoreEvent>,
) -> io::Result<bool> {
    let candidates: Vec<_> = catalog
        .entries()
        .filter(|entry| entry.state == EpochState::Candidate)
        .map(|entry| entry.epoch)
        .collect();
    let changed = !candidates.is_empty();
    for epoch in candidates {
        catalog.remove(epoch);
        table.forget_epoch(epoch);
        remove_dir(&epoch.dir(dir))?;
        events.push(StoreEvent::CandidateDropped(epoch));
    }
    for epoch in epoch_dirs(dir)? {
        if catalog.get(epoch).is_none() {
            remove_dir(&epoch.dir(dir))?;
        }
    }
    Ok(changed)
}

/// Removes the directory at `path` and everything in it; a missing one is not an error.
/// Links inside are removed, never followed.
pub(super) fn remove_dir(path: &Path) -> io::Result<()> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}
