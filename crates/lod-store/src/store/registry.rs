//! Saving the material registry before any region frame that may name its ids.

use std::{
    io,
    path::PathBuf,
    sync::{Mutex, PoisonError},
};

use super::SharedRegistry;
use crate::{
    region_file::WriteBarrier,
    table::{self, Durability},
};

/// A store's registry and how much of it is on disk. As the [`WriteBarrier`] of every
/// writable region set, it saves the registry, synced, before region frames reach disk:
/// by an eviction's commit, a merge, a flush or a full write buffer alike.
#[derive(Debug)]
pub(super) struct RegistryFile {
    registry: SharedRegistry,
    path: PathBuf,
    /// Materials the file on disk holds.
    saved: Mutex<usize>,
}

impl RegistryFile {
    /// A registry stored at `path`, of which the first `saved` materials are on disk.
    pub(super) fn new(registry: SharedRegistry, path: PathBuf, saved: usize) -> Self {
        Self {
            registry,
            path,
            saved: Mutex::new(saved),
        }
    }

    /// The shared registry samplers assign ids in.
    pub(super) fn shared(&self) -> &SharedRegistry {
        &self.registry
    }

    /// The registry file.
    pub(super) fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Rewrites the file, synced, when the registry grew since the last save.
    pub(super) fn save(&self) -> io::Result<()> {
        let mut saved = self.saved.lock().unwrap_or_else(PoisonError::into_inner);
        let registry = self.registry.read().unwrap_or_else(PoisonError::into_inner);
        if registry.len() == *saved {
            return Ok(());
        }
        let (len, bytes) = (registry.len(), registry.encode());
        drop(registry);
        table::replace(&self.path, &bytes, Durability::Synced)?;
        *saved = len;
        Ok(())
    }
}

impl WriteBarrier for RegistryFile {
    fn before_write(&self) -> io::Result<()> {
        self.save().map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "saving {} before region writes: {error}",
                    self.path.display()
                ),
            )
        })
    }
}
