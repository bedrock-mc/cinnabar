//! Installed global packs and an editable, highest-priority-first selection.

use crate::{
    AdmissionError, MAX_ARCHIVE_BYTES, MAX_PACKS, ValidatedPackStack,
    parser::validate_archive_parts, subpacks as tiers,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

mod storage;
pub use storage::{DirectoryStorage, PackStorage};

const CATALOG_FILE: &str = "global_packs.json";
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;

/// One manifest-declared resolution or memory option.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Subpack {
    pub folder: String,
    pub name: String,
    pub memory_tier: u32,
}

/// Display metadata for an installed, unencrypted resource pack.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct InstalledPack {
    pub id: Uuid,
    pub version: [u32; 3],
    pub name: String,
    pub description: String,
    pub min_engine_version: Option<[u32; 3]>,
    pub subpacks: Vec<Subpack>,
    #[serde(default)]
    pub revision: u64,
}

impl InstalledPack {
    /// Formats the manifest version for archive admission.
    pub(crate) fn version_text(&self) -> String {
        self.version.map(|part| part.to_string()).join(".")
    }

    /// Derives a safe archive filename exclusively from parsed identity fields.
    pub(crate) fn filename(&self) -> String {
        if self.revision == 0 {
            format!("{}-{}.mcpack", self.id, self.version_text())
        } else {
            format!(
                "{}-{}-{}.mcpack",
                self.id,
                self.version_text(),
                self.revision
            )
        }
    }
}

/// A selected pack; list index zero has the highest priority.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ActivePack {
    pub id: Uuid,
    #[serde(default)]
    pub subpack: String,
    #[serde(default)]
    pub revision: u64,
}

/// Independent outcomes from a resource pack or add-on bundle import.
#[derive(Debug, Default)]
pub struct ImportReport {
    pub imported: Vec<InstalledPack>,
    pub skipped_behavior: usize,
    pub rejected: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("resource-pack storage: {0}")]
    Io(#[from] std::io::Error),
    #[error("resource-pack catalog is malformed: {0}")]
    Catalog(#[from] serde_json::Error),
    #[error(transparent)]
    Admission(#[from] AdmissionError),
    #[error("encrypted marketplace or password-protected packs are unsupported")]
    Encrypted,
    #[error("open a supported resource-pack archive")]
    UnsupportedExtension,
    #[error("pack is not installed or not active")]
    UnknownPack,
    #[error("pack requires a newer game version")]
    NewerEngine,
    #[error("installed resource-pack catalog exceeds its storage limit")]
    CatalogTooLarge,
}

#[derive(Clone, Default, Deserialize, Serialize)]
struct Catalog {
    #[serde(default)]
    available: Vec<InstalledPack>,
    #[serde(default)]
    active: Vec<ActivePack>,
    #[serde(default)]
    retained: Vec<InstalledPack>,
    #[serde(default)]
    next_revision: u64,
}

/// Installed packs in a [`PackStorage`]. Selection edits are staged until `apply` succeeds.
pub struct GlobalPackLibrary {
    storage: Box<dyn PackStorage>,
    engine_version: [u32; 3],
    catalog: Catalog,
    active: Vec<ActivePack>,
    memory_tier: u32,
}

impl GlobalPackLibrary {
    /// Opens install-owned pack storage in the directory `root` without touching the base
    /// carriers.
    pub fn open(root: impl Into<PathBuf>, engine_version: [u32; 3]) -> Result<Self, LibraryError> {
        Self::open_in(Box::new(DirectoryStorage::new(root)), engine_version)
    }

    /// Opens the packs kept in `storage`.
    pub fn open_in(
        storage: Box<dyn PackStorage>,
        engine_version: [u32; 3],
    ) -> Result<Self, LibraryError> {
        let catalog: Catalog = match storage.read(CATALOG_FILE, MAX_CATALOG_BYTES) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if storage::not_found(&error) => Catalog::default(),
            Err(error) => return Err(error),
        };
        if catalog.active.len() > MAX_PACKS {
            return Err(AdmissionError::TooManyPacks.into());
        }
        let active = catalog.active.clone();
        Ok(Self {
            storage,
            engine_version,
            catalog,
            active,
            memory_tier: 0,
        })
    }

    /// Sets vanilla's memory tier for newly activated packs; manual selections remain intact.
    pub fn set_device_memory(&mut self, bytes: u64) {
        self.memory_tier = tiers::device_memory_tier(bytes);
    }

    /// Reports the tier used to warn about unsupported manual choices.
    pub fn device_memory_tier(&self) -> u32 {
        self.memory_tier
    }

    /// Resolves the immutable archive version referenced by a selection.
    pub fn metadata(&self, active: &ActivePack) -> Option<&InstalledPack> {
        self.catalog
            .available
            .iter()
            .chain(&self.catalog.retained)
            .find(|pack| pack.id == active.id && pack.revision == active.revision)
    }

    /// Lists installed resource packs, including active packs.
    pub fn available(&self) -> &[InstalledPack] {
        &self.catalog.available
    }

    /// Lists the staged selection, highest priority first.
    pub fn active(&self) -> &[ActivePack] {
        &self.active
    }

    /// Where the library keeps its packs, for callers that store pack artwork beside them.
    pub fn storage(&self) -> &dyn PackStorage {
        self.storage.as_ref()
    }

    /// [`Self::storage`], to store pack artwork beside the packs.
    pub fn storage_mut(&mut self) -> &mut dyn PackStorage {
        self.storage.as_mut()
    }

    /// The stored archives a [`Self::preview`] of the staged selection reads.
    pub fn staged_archives(&self) -> Vec<String> {
        self.active
            .iter()
            .filter_map(|active| self.metadata(active))
            .map(InstalledPack::filename)
            .collect()
    }

    /// Imports independently valid resource halves; behavior halves are skipped.
    pub fn import(&mut self, path: &Path) -> Result<ImportReport, LibraryError> {
        if !crate::is_pack_import_path(path) {
            return Err(LibraryError::UnsupportedExtension);
        }
        self.import_archive(storage::read_file(path, MAX_ARCHIVE_BYTES)?)
    }

    /// Imports the archive `bytes` of a file the player picked, as [`Self::import`] does.
    pub fn import_archive(&mut self, bytes: Vec<u8>) -> Result<ImportReport, LibraryError> {
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(AdmissionError::ArchiveTooLarge.into());
        }
        let (mut report, archives) = crate::import::read_import(bytes)?;
        let mut candidate = self.catalog.clone();
        let mut accepted = Vec::new();
        for (mut metadata, bytes) in archives {
            candidate.next_revision = candidate
                .next_revision
                .checked_add(1)
                .ok_or(LibraryError::CatalogTooLarge)?;
            metadata.revision = candidate.next_revision;
            if let Some(previous) = candidate
                .available
                .iter()
                .find(|pack| pack.id == metadata.id)
            {
                candidate.retained.push(previous.clone());
            }
            candidate.available.retain(|pack| pack.id != metadata.id);
            candidate.available.push(metadata.clone());
            accepted.push((metadata, bytes));
        }
        let catalog_bytes = encode_catalog(&candidate)?;
        for (metadata, bytes) in accepted {
            self.storage.write(&metadata.filename(), &bytes)?;
            report.imported.push(metadata);
        }
        self.storage.write(CATALOG_FILE, &catalog_bytes)?;
        self.catalog = candidate;
        for active in &mut self.active {
            if let Some(pack) = self
                .catalog
                .available
                .iter()
                .find(|pack| pack.id == active.id)
            {
                active.revision = pack.revision;
                if !pack
                    .subpacks
                    .iter()
                    .any(|pack| pack.folder == active.subpack)
                {
                    active.subpack = tiers::select(&pack.subpacks, self.memory_tier).to_owned();
                }
            }
        }
        Ok(report)
    }

    /// Reads one installed pack's own icon without activating it or consulting overlays.
    pub fn pack_icon(&self, metadata: &InstalledPack) -> Result<Option<Box<[u8]>>, LibraryError> {
        let bytes = self.storage.read(&metadata.filename(), MAX_ARCHIVE_BYTES)?;
        let (pack, _) =
            validate_archive_parts(metadata.id, &metadata.version_text(), "", bytes, None, None)?;
        Ok(pack.read_file_with_limit("pack_icon.png", 4 * 1024 * 1024)?)
    }

    /// Adds a pack at the top of the staged stack, without duplicating it.
    pub fn activate(&mut self, id: Uuid) -> Result<(), LibraryError> {
        let pack = self
            .catalog
            .available
            .iter()
            .find(|pack| pack.id == id)
            .ok_or(LibraryError::UnknownPack)?;
        if pack
            .min_engine_version
            .is_some_and(|minimum| minimum > self.engine_version)
        {
            return Err(LibraryError::NewerEngine);
        }
        if self.active.iter().any(|pack| pack.id == id) {
            return Ok(());
        }
        if self.active.len() >= MAX_PACKS {
            return Err(AdmissionError::TooManyPacks.into());
        }
        self.active.insert(
            0,
            ActivePack {
                id,
                subpack: tiers::select(&pack.subpacks, self.memory_tier).to_owned(),
                revision: pack.revision,
            },
        );
        Ok(())
    }

    /// Removes a pack from the staged stack, retaining its installed archive.
    pub fn deactivate(&mut self, id: Uuid) {
        self.active.retain(|pack| pack.id != id);
    }

    /// Moves an active pack to a clamped, highest-priority-first list index.
    pub fn move_pack(&mut self, id: Uuid, index: usize) -> Result<(), LibraryError> {
        let old = self
            .active
            .iter()
            .position(|pack| pack.id == id)
            .ok_or(LibraryError::UnknownPack)?;
        let pack = self.active.remove(old);
        self.active.insert(index.min(self.active.len()), pack);
        Ok(())
    }

    /// Selects a declared subpack; an empty folder selects the root resources.
    pub fn select_subpack(&mut self, id: Uuid, folder: &str) -> Result<(), LibraryError> {
        let active = self
            .active
            .iter()
            .find(|pack| pack.id == id)
            .ok_or(LibraryError::UnknownPack)?;
        let pack = self.metadata(active).ok_or(LibraryError::UnknownPack)?;
        if !folder.is_empty() && !pack.subpacks.iter().any(|subpack| subpack.folder == folder) {
            return Err(AdmissionError::InvalidSubpack.into());
        }
        let selection = self
            .active
            .iter_mut()
            .find(|pack| pack.id == id)
            .ok_or(LibraryError::UnknownPack)?;
        selection.subpack = folder.to_owned();
        Ok(())
    }

    /// Validates and persists a selection when no later runtime publication is required.
    pub fn apply(&mut self) -> Result<Arc<ValidatedPackStack>, LibraryError> {
        let stack = self.preview()?;
        self.commit_selection(&self.active.clone())?;
        Ok(stack)
    }

    /// Validates staged archives on the caller's worker without persisting pending changes.
    pub fn preview(&mut self) -> Result<Arc<ValidatedPackStack>, LibraryError> {
        let mut stack = ValidatedPackStack {
            packs: Box::default(),
            rejections: Box::default(),
        };
        for active in self.active.iter().rev() {
            let metadata = self.metadata(active).ok_or(LibraryError::UnknownPack)?;
            if metadata
                .min_engine_version
                .is_some_and(|minimum| minimum > self.engine_version)
            {
                return Err(LibraryError::NewerEngine);
            }
            let bytes = self.storage.read(&metadata.filename(), MAX_ARCHIVE_BYTES)?;
            let (pack, _) = validate_archive_parts(
                active.id,
                &metadata.version_text(),
                &active.subpack,
                bytes,
                None,
                None,
            )?;
            let next = ValidatedPackStack {
                packs: vec![pack].into_boxed_slice(),
                rejections: Box::default(),
            };
            stack = ValidatedPackStack::compose(&stack, &next)?;
        }
        Ok(Arc::new(stack))
    }

    /// Persists an acknowledged runtime selection without changing newer staged edits.
    pub fn commit_selection(&mut self, selection: &[ActivePack]) -> Result<(), LibraryError> {
        if selection.len() > MAX_PACKS {
            return Err(AdmissionError::TooManyPacks.into());
        }
        let mut selection = selection.to_vec();
        let mut seen = std::collections::HashSet::new();
        for active in &mut selection {
            if !seen.insert(active.id) {
                return Err(AdmissionError::DuplicatePack.into());
            }
            let pack = self.metadata(active).ok_or(LibraryError::UnknownPack)?;
            if !active.subpack.is_empty()
                && !pack
                    .subpacks
                    .iter()
                    .any(|subpack| subpack.folder == active.subpack)
            {
                active.subpack.clear();
            }
        }
        let previous = std::mem::replace(&mut self.catalog.active, selection.to_vec());
        if let Err(error) = self.persist() {
            self.catalog.active = previous;
            return Err(error);
        }
        if selection == self.active {
            storage::prune(self.storage.as_mut(), &mut self.catalog)?;
        }
        Ok(())
    }

    /// Atomically publishes catalog changes while retaining the last applied selection.
    fn persist(&mut self) -> Result<(), LibraryError> {
        let bytes = encode_catalog(&self.catalog)?;
        self.storage.write(CATALOG_FILE, &bytes)
    }
}

/// Applies the same catalog bound before publication and on the next startup read.
fn encode_catalog(catalog: &Catalog) -> Result<Vec<u8>, LibraryError> {
    let bytes = serde_json::to_vec_pretty(catalog)?;
    if bytes.len() > MAX_CATALOG_BYTES {
        return Err(LibraryError::CatalogTooLarge);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
