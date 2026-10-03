//! Installed global packs and an editable, highest-priority-first selection.

use crate::{
    AdmissionError, MAX_ARCHIVE_BYTES, MAX_PACKS, ValidatedPackStack,
    parser::validate_archive_parts,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
use uuid::Uuid;

mod storage;
mod tiers;

const CATALOG_FILE: &str = "global_packs.json";
const MAX_CATALOG_BYTES: usize = 16 * 1024 * 1024;

/// One manifest-declared resolution or memory option.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Subpack {
    pub folder: String,
    pub name: String,
    pub memory_tier: u32,
}

/// Display metadata for an installed, unencrypted resource pack.
#[derive(Clone, Debug, Deserialize, Serialize)]
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

/// Disk-backed library. Selection edits are staged until `apply` succeeds.
pub struct GlobalPackLibrary {
    root: PathBuf,
    engine_version: [u32; 3],
    catalog: Catalog,
    active: Vec<ActivePack>,
    memory_tier: u32,
}

impl GlobalPackLibrary {
    /// Opens install-owned pack storage without touching the base carriers.
    pub fn open(root: impl Into<PathBuf>, engine_version: [u32; 3]) -> Result<Self, LibraryError> {
        let root = root.into();
        let path = root.join(CATALOG_FILE);
        let catalog: Catalog = if path.exists() {
            serde_json::from_slice(&read_bounded(&path, MAX_CATALOG_BYTES)?)?
        } else {
            Catalog::default()
        };
        if catalog.active.len() > MAX_PACKS {
            return Err(AdmissionError::TooManyPacks.into());
        }
        let active = catalog.active.clone();
        Ok(Self {
            root,
            engine_version,
            catalog,
            active,
            memory_tier: 0,
        })
    }

    /// Sets vanilla's memory tier for newly activated packs; manual selections remain intact.
    pub fn set_device_memory(&mut self, bytes: u64) {
        self.memory_tier = tiers::memory_tier(bytes);
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

    /// Imports independently valid resource halves; behavior halves are skipped.
    pub fn import(&mut self, path: &Path) -> Result<ImportReport, LibraryError> {
        if !crate::is_pack_import_path(path) {
            return Err(LibraryError::UnsupportedExtension);
        }
        let bytes = read_bounded(path, MAX_ARCHIVE_BYTES)?;
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
            atomic_write(&self.root.join(metadata.filename()), &bytes)?;
            report.imported.push(metadata);
        }
        atomic_write(&self.root.join(CATALOG_FILE), &catalog_bytes)?;
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
        let bytes = read_bounded(&self.root.join(metadata.filename()), MAX_ARCHIVE_BYTES)?;
        let (pack, _) =
            validate_archive_parts(metadata.id, &metadata.version_text(), "", bytes, None)?;
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
            let bytes = read_bounded(&self.root.join(metadata.filename()), MAX_ARCHIVE_BYTES)?;
            let (pack, _) = validate_archive_parts(
                active.id,
                &metadata.version_text(),
                &active.subpack,
                bytes,
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
            storage::prune(&self.root, &mut self.catalog)?;
        }
        Ok(())
    }

    /// Atomically publishes catalog changes while retaining the last applied selection.
    fn persist(&self) -> Result<(), LibraryError> {
        atomic_write(
            &self.root.join(CATALOG_FILE),
            &encode_catalog(&self.catalog)?,
        )
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

/// Caps reads before allocating, including files that grow during the read.
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, LibraryError> {
    let file = fs::File::open(path)?;
    if file.metadata()?.len() > limit as u64 {
        return Err(AdmissionError::ArchiveTooLarge.into());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(AdmissionError::ArchiveTooLarge.into());
    }
    Ok(bytes)
}

/// Uses a sibling temporary file so readers see either complete version.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), LibraryError> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = fs::File::create(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)?;
    Ok(())
}

#[cfg(test)]
mod tests;
