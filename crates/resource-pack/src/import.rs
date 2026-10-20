//! Bounded import of ordinary pack archives and resource halves of add-on bundles.

use bedrock_packs::jsonc::parse_jsonc;

use crate::{
    AdmissionError, MAX_DECLARED_BYTES_PER_PACK, MAX_FILE_BYTES, MAX_LOCAL_ARCHIVE_BYTES,
    MAX_LOCAL_PACKS, MAX_MANIFEST_BYTES,
    library::{ImportReport, InstalledPack, LibraryError},
    manifest::{Purpose, Version, read_manifest},
    parser::{
        MANIFEST_NAMES, canonical_path, is_regular_file, prepare_zip_bytes, validate_archive_parts,
    },
};
use serde_json::Value;
use std::{
    collections::HashSet,
    io::{Cursor, Read},
    path::Path,
};
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

mod directory;
mod selection;
pub use selection::{
    GlobalSelectionEntry, ParsedGlobalSelection, merge_global_selection, parse_global_selection,
};

type ImportedArchives = Vec<(InstalledPack, Vec<u8>)>;

/// Validated bytes held between preview and confirmation, without installing anything.
#[derive(Debug)]
pub struct PreparedImport {
    pub report: ImportReport,
    pub(crate) archives: ImportedArchives,
}

impl PreparedImport {
    /// The archive memory retained by this preview, for the host's aggregate bound.
    pub fn archive_bytes(&self) -> usize {
        self.archives.iter().map(|(_, bytes)| bytes.len()).sum()
    }

    /// Splits validated resource halves after the caller has recorded the bundle diagnostics.
    pub fn into_resource_packs(self) -> impl Iterator<Item = Self> {
        self.archives.into_iter().map(|(metadata, bytes)| Self {
            report: ImportReport {
                imported: vec![metadata.clone()],
                ..Default::default()
            },
            archives: vec![(metadata, bytes)],
        })
    }
}

/// Reads an explicit user pack path and validates it through the ordinary import pipeline.
pub fn prepare_import(path: &Path) -> Result<PreparedImport, LibraryError> {
    let metadata = std::fs::symlink_metadata(path)?;
    let bytes = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        directory::archive(path)?
    } else if metadata.is_file() && !metadata.file_type().is_symlink() && is_pack_import_path(path)
    {
        crate::library::read_bounded(path, MAX_LOCAL_ARCHIVE_BYTES)?
    } else {
        return Err(LibraryError::UnsupportedExtension);
    };
    let (mut report, archives) = read_import(bytes, path)?;
    report.imported = archives
        .iter()
        .map(|(metadata, _)| metadata.clone())
        .collect();
    Ok(PreparedImport { report, archives })
}

/// Pack archive extensions accepted by file-open, drag-and-drop, and native pickers.
pub const PACK_IMPORT_EXTENSIONS: [&str; 3] = ["mcpack", "mcaddon", "zip"];

/// Recognizes file-open and drop targets without attempting archive reads.
pub fn is_pack_import_path(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| {
            PACK_IMPORT_EXTENSIONS
                .iter()
                .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        })
}

/// Reads each pack separately so a behavior or malformed half cannot hide a valid resource half.
pub(crate) fn read_import(
    bytes: Vec<u8>,
    source: &Path,
) -> Result<(ImportReport, ImportedArchives), LibraryError> {
    let mut report = ImportReport::default();
    let mut imported = Vec::new();
    let source = source
        .file_name()
        .unwrap_or(source.as_os_str())
        .to_string_lossy();
    read_bundle(bytes, &source, 0, &mut report, &mut imported)
        .map_err(|error| file_error(&source, error))?;
    if imported.iter().map(|(_, bytes)| bytes.len()).sum::<usize>()
        > crate::MAX_LOCAL_STACK_ARCHIVE_BYTES
    {
        return Err(AdmissionError::StackArchiveTooLarge.into());
    }
    Ok((report, imported))
}

/// Splits directory bundles or one level of nested pack archives without filesystem extraction.
fn read_bundle(
    mut bytes: Vec<u8>,
    source: &str,
    depth: usize,
    report: &mut ImportReport,
    imported: &mut ImportedArchives,
) -> Result<(), LibraryError> {
    if bytes.len() > MAX_LOCAL_ARCHIVE_BYTES {
        return Err(AdmissionError::ArchiveTooLarge.into());
    }
    prepare_zip_bytes(&mut bytes)?;
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| AdmissionError::MalformedZip)?;
    let paths = index_archive(&mut archive)?;
    let roots = paths
        .iter()
        .filter_map(|(_, path)| {
            let (root, name) = path
                .rsplit_once('/')
                .map_or(("", path.as_str()), |(root, name)| {
                    (&path[..root.len() + 1], name)
                });
            MANIFEST_NAMES.contains(&name).then(|| root.to_owned())
        })
        .collect::<HashSet<_>>();
    let mut roots = roots.into_iter().collect::<Vec<_>>();
    roots.sort();
    if roots.len() > MAX_LOCAL_PACKS {
        return Err(AdmissionError::TooManyPacks.into());
    }
    for root in &roots {
        if imported.len() >= MAX_LOCAL_PACKS {
            return Err(AdmissionError::TooManyPacks.into());
        }
        let result = read_candidate(&mut archive, &paths, root, &roots);
        match result {
            Ok(Some(pack)) => imported.push(pack),
            Ok(None) => report.skipped_behavior += 1,
            Err(error) => report.rejected.push(format!("{source}: {error}")),
        }
    }
    if depth == 0 {
        let mut nested_bytes = 0usize;
        for (index, path) in &paths {
            if !roots.iter().any(|root| path.starts_with(root))
                && is_pack_import_path(Path::new(path))
            {
                let bytes = match read_entry(&mut archive, *index, MAX_LOCAL_ARCHIVE_BYTES as u64) {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        report.rejected.push(format!("{source}/{path}: {error}"));
                        continue;
                    }
                };
                nested_bytes = nested_bytes.saturating_add(bytes.len());
                if nested_bytes > crate::MAX_LOCAL_STACK_ARCHIVE_BYTES {
                    return Err(AdmissionError::StackArchiveTooLarge.into());
                }
                let nested_source = format!("{source}/{path}");
                if let Err(error) = read_bundle(bytes, &nested_source, depth + 1, report, imported)
                {
                    report.rejected.push(format!("{nested_source}: {error}"));
                }
            }
        }
    }
    if roots.is_empty() && depth > 0 {
        return Err(AdmissionError::MissingManifest.into());
    }
    if depth == 0
        && imported.is_empty()
        && report.skipped_behavior == 0
        && report.rejected.is_empty()
    {
        return Err(AdmissionError::MissingManifest.into());
    }
    Ok(())
}

/// Rejects encrypted ZIPs and bounds every declared entry before reading payloads.
fn index_archive(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
) -> Result<Vec<(usize, String)>, LibraryError> {
    let mut paths = Vec::new();
    let mut seen = HashSet::new();
    let mut declared = 0u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index_raw(index)
            .map_err(|_| AdmissionError::MalformedZip)?;
        if file.encrypted() {
            return Err(LibraryError::Encrypted);
        }
        declared = declared
            .checked_add(file.size())
            .filter(|size| *size <= MAX_DECLARED_BYTES_PER_PACK)
            .ok_or(AdmissionError::DeclaredSizeTooLarge)?;
        let Some(path) = canonical_path(file.name_raw()) else {
            continue;
        };
        if file.is_dir()
            || !is_regular_file(file.unix_mode())
            || file.size() > MAX_FILE_BYTES
            || !matches!(
                file.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            || !seen.insert(path.to_ascii_lowercase())
        {
            continue;
        }
        paths.push((index, path.into()));
    }
    Ok(paths)
}

/// Re-roots a candidate archive by copying compressed entries, then reuses server admission.
fn read_candidate(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    paths: &[(usize, String)],
    root: &str,
    roots: &[String],
) -> Result<Option<(InstalledPack, Vec<u8>)>, LibraryError> {
    let (manifest_name, manifest_index) = MANIFEST_NAMES
        .into_iter()
        .find_map(|name| {
            paths
                .iter()
                .find(|(_, path)| path == &format!("{root}{name}"))
                .map(|(index, _)| (name, *index))
        })
        .ok_or(AdmissionError::MissingManifest)?;
    let manifest_path = format!("{root}{manifest_name}");
    let manifest = read_entry(archive, manifest_index, MAX_MANIFEST_BYTES as u64)
        .map_err(|error| file_error(&manifest_path, error))?;
    let Some(mut metadata) =
        read_metadata(&manifest).map_err(|error| file_error(&manifest_path, error))?
    else {
        return Ok(None);
    };
    if let Some((index, path)) = paths
        .iter()
        .find(|(_, path)| path == &format!("{root}contents.json"))
    {
        let result = read_entry(archive, *index, MAX_MANIFEST_BYTES as u64)
            .and_then(|bytes| reject_encrypted_index(&bytes));
        result.map_err(|error| file_error(path, error))?;
    }
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (index, path) in paths {
        let Some(relative) = path.strip_prefix(root) else {
            continue;
        };
        if roots
            .iter()
            .any(|other| other != root && other.starts_with(root) && path.starts_with(other))
        {
            continue;
        }
        let file = archive
            .by_index_raw(*index)
            .map_err(|_| AdmissionError::MalformedZip)?;
        writer
            .raw_copy_file_rename(file, relative)
            .map_err(|_| AdmissionError::MalformedZip)?;
    }
    let bytes = writer
        .finish()
        .map_err(|_| AdmissionError::MalformedZip)?
        .into_inner();
    let (validated, _, _) = validate_archive_parts(
        metadata.id,
        &metadata.version_text(),
        "",
        bytes.clone(),
        None,
        None,
        Purpose::Rendering,
    )
    .map_err(|error| file_error(&manifest_path, error.into()))?;
    if let Ok(Some(language)) =
        validated.read_file_with_limit("texts/en_US.lang", MAX_MANIFEST_BYTES as u64)
    {
        localize_metadata(&mut metadata, &language);
    }
    Ok(Some((metadata, bytes)))
}

/// Accepts content-only indexes but never requests or derives marketplace keys.
fn reject_encrypted_index(bytes: &[u8]) -> Result<(), LibraryError> {
    let root: Value = parse_jsonc(bytes).map_err(|_| LibraryError::Encrypted)?;
    if root["content"].as_array().is_some_and(|entries| {
        entries
            .iter()
            .any(|entry| entry["key"].as_str().is_some_and(|key| !key.is_empty()))
    }) {
        return Err(LibraryError::Encrypted);
    }
    Ok(())
}

/// Reads manifest display fields after the shared identity/module validation.
fn read_metadata(bytes: &[u8]) -> Result<Option<InstalledPack>, LibraryError> {
    let root: Value = parse_jsonc(bytes).map_err(LibraryError::PackJson)?;
    if root["modules"].as_array().is_some_and(|modules| {
        !modules.iter().any(|module| module["type"] == "resources")
            && modules
                .iter()
                .any(|module| module["type"] == "data" || module["type"] == "script")
    }) {
        return Ok(None);
    }
    let header = &root["header"];
    let id = header["uuid"]
        .as_str()
        .and_then(|value| Uuid::parse_str(value.trim()).ok())
        .ok_or(AdmissionError::MalformedManifest)?;
    let version = Version::from_value(&header["version"])
        .ok_or(AdmissionError::InvalidVersion)?
        .0;
    let version_text = version.map(|part| part.to_string()).join(".");
    let manifest = read_manifest(bytes, id, &version_text, Purpose::Rendering)?;
    let min_engine_version = if header["min_engine_version"].is_null() {
        None
    } else {
        Some(
            Version::from_value(&header["min_engine_version"])
                .ok_or(AdmissionError::InvalidVersion)?
                .0,
        )
    };
    Ok(Some(InstalledPack {
        id,
        version,
        name: header["name"].as_str().unwrap_or("Resource pack").into(),
        description: header["description"].as_str().unwrap_or_default().into(),
        min_engine_version,
        subpacks: manifest.subpacks,
        revision: 0,
    }))
}

/// Adds the source file to an import failure without changing its underlying reason.
fn file_error(path: &str, source: LibraryError) -> LibraryError {
    LibraryError::PackFile {
        path: path.to_owned(),
        source: Box::new(source),
    }
}

/// Enforces the actual inflated size as well as the ZIP directory's size.
fn read_entry(
    archive: &mut ZipArchive<Cursor<Vec<u8>>>,
    index: usize,
    limit: u64,
) -> Result<Vec<u8>, LibraryError> {
    let mut file = archive
        .by_index(index)
        .map_err(|_| AdmissionError::InvalidFileData)?;
    let declared = file.size();
    if declared > limit {
        return Err(AdmissionError::FileTooLarge.into());
    }
    let mut bytes = Vec::new();
    file.by_ref().take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != declared {
        return Err(AdmissionError::InvalidFileData.into());
    }
    Ok(bytes)
}

/// Resolves known English fallback keys; PackManifest and Pack load pack localization.
fn localize_metadata(metadata: &mut InstalledPack, bytes: &[u8]) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    let (name_key, description_key) = (metadata.name.clone(), metadata.description.clone());
    for line in text.trim_start_matches('\u{feff}').lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = value
            .split_once("##")
            .map_or(value, |(text, _)| text)
            .trim();
        if key.trim() == name_key {
            metadata.name = value.to_owned();
        }
        if key.trim() == description_key {
            metadata.description = value.to_owned();
        }
    }
}
