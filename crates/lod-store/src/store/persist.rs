//! Writing a store's files: flushes, close, and its event log.

use std::path::Path;

use super::{Store, StoreEvent};
use crate::{
    access::ACCESS_FILE,
    epochs::CATALOG_FILE,
    error::TableErrorAt,
    region_file::Access,
    table::{self, Durability, TableError},
};

impl Store {
    /// Persists the registry (synced) first, then commits region files, compacting
    /// within [`super::StoreOptions::compaction_budget_bytes`], and writes the catalog and
    /// the access table. Nothing else is synced; see [`Self::sync`]. Signatures of epochs no session uses are written and
    /// their files closed; those in use stay in memory, where votes read them, until
    /// their epoch goes idle, their cache evicts them or the store closes, so a crash
    /// loses at most one session's new signatures. Failures become events; true when
    /// everything was written.
    pub fn flush(&mut self) -> bool {
        self.write_all(false)
    }

    /// [`Self::flush`], then flushes the open region files to disk, for owners that sync
    /// when the client goes idle; true when everything was written and synced.
    pub fn sync(&mut self) -> bool {
        self.write_all(true)
    }

    /// Ends every session, flushes and syncs every file, and returns the events not yet
    /// drained.
    pub fn close(mut self) -> Vec<StoreEvent> {
        let dimensions: Vec<_> = self.sessions.keys().copied().collect();
        for dimension in dimensions {
            self.end_session(dimension);
        }
        self.write_all(true);
        self.collect_file_events();
        self.events
    }

    /// [`Self::flush`], also flushing region files to disk when `sync` is set.
    fn write_all(&mut self, sync: bool) -> bool {
        if self.access == Access::ReadOnly {
            self.collect_file_events();
            return true;
        }
        let mut ok = self.save_registry();
        // Region sets save the registry themselves too; stopping here keeps the failure
        // to one event.
        let mut idle = Vec::new();
        if ok {
            let in_use = self.epochs_in_use();
            for (&epoch, files) in &mut self.epochs {
                let committed = if sync {
                    files.regions.sync_all()
                } else {
                    files.regions.commit_all()
                };
                let idle_epoch = !in_use.contains(&epoch);
                let signed = !idle_epoch || files.signatures.flush();
                if committed && signed && idle_epoch {
                    idle.push(epoch);
                }
                ok &= committed && signed;
            }
        }
        ok &= self.save_tables();
        self.collect_file_events();
        // Written epochs no session uses give back their handles.
        for epoch in idle {
            self.epochs.remove(&epoch);
        }
        ok
    }

    /// Takes the events recorded since the last call.
    pub fn drain_events(&mut self) -> Vec<StoreEvent> {
        self.collect_file_events();
        std::mem::take(&mut self.events)
    }

    /// Bytes the store directory holds on disk.
    pub fn disk_bytes(&self) -> std::io::Result<u64> {
        crate::root::dir_bytes(&self.dir)
    }

    /// Writes the registry when it grew since the last save; true unless that failed.
    pub(super) fn save_registry(&mut self) -> bool {
        match self.registry.save() {
            Ok(()) => true,
            Err(error) => {
                self.events.push(StoreEvent::Io {
                    path: self.registry.path().clone(),
                    error,
                });
                false
            }
        }
    }

    /// Writes the catalog and access table when they changed; true unless that failed.
    pub(super) fn save_tables(&mut self) -> bool {
        let mut ok = true;
        if self.catalog_dirty {
            let saved = self.write_table(CATALOG_FILE, &self.catalog.encode());
            self.catalog_dirty = !saved;
            ok &= saved;
        }
        if self.table_dirty {
            let saved = self.write_table(ACCESS_FILE, &self.table.encode());
            self.table_dirty = !saved;
            ok &= saved;
        }
        ok
    }

    /// Replaces the table file `name` with `bytes`, without syncing; a failure becomes an
    /// event and returns false.
    pub(super) fn write_table(&mut self, name: &str, bytes: &[u8]) -> bool {
        let path = self.dir.join(name);
        match table::replace(&path, bytes, Durability::Cached) {
            Ok(()) => true,
            Err(error) => {
                self.events.push(StoreEvent::Io { path, error });
                false
            }
        }
    }

    /// Moves region and signature events of every open epoch into the store's events.
    pub(super) fn collect_file_events(&mut self) {
        for (&epoch, files) in &mut self.epochs {
            self.events.extend(
                files
                    .regions
                    .drain_events()
                    .into_iter()
                    .map(|event| StoreEvent::Region { epoch, event }),
            );
            self.events.extend(
                files
                    .signatures
                    .drain_events()
                    .into_iter()
                    .map(|event| StoreEvent::Signature { epoch, event }),
            );
        }
    }
}

/// A table error with the path of the file it came from.
pub(crate) fn table_error(path: &Path, error: TableError) -> TableErrorAt {
    TableErrorAt {
        path: path.to_owned(),
        error,
    }
}
