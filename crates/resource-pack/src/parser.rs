//! Bounded archive indexing and per-pack admission.
//!
//! Structural ZIP damage or a bad manifest drops one pack; odd entries inside an
//! otherwise sound archive are skipped and counted instead.

use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::Arc,
};

#[cfg(feature = "handoff")]
use protocol::ResourcePackArchive;
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive};

use crate::{
    AdmissionError, MAX_DECLARED_BYTES_PER_PACK, MAX_ENTRIES_PER_PACK, MAX_FILE_BYTES,
    MAX_MANIFEST_BYTES, MAX_PATH_BYTES,
    crypto::{ContentKey, contents_file_keys},
    manifest::read_manifest,
    pack::{ArchiveBytes, EntryIndex, ValidatedPack, read_entry},
    read_cache::ReadCache,
};
#[cfg(feature = "handoff")]
use crate::{
    MAX_DECLARED_BYTES_PER_STACK, MAX_ENTRIES_PER_STACK, PackRejection, ValidatedPackStack,
};

const MAX_CONTENTS_INDEX_BYTES: u64 = 4 * 1024 * 1024;

/// Validates one unencrypted archive under a nil identity; used by fuzzing.
pub fn validate_archive_bytes(bytes: &[u8]) -> Result<(), AdmissionError> {
    validate_archive_parts(Uuid::nil(), "0.0.0", "", bytes.to_vec(), None, None).map(|_| ())
}

/// Admits highest-priority-first server entries, then stores layers lowest priority first.
/// Structural checks retain rejection indexes in the server order.
#[cfg(feature = "handoff")]
pub(super) fn validate_stack(
    archives: Vec<ResourcePackArchive>,
    memory: Option<u32>,
) -> ValidatedPackStack {
    let mut packs = Vec::with_capacity(archives.len());
    let mut rejections = Vec::new();
    let (mut entry_count, mut declared_bytes) = (0usize, 0u64);
    let mut seen = HashSet::with_capacity(archives.len());
    let reads = Arc::new(ReadCache::default());
    for (stack_index, archive) in archives.into_iter().enumerate() {
        let admitted = admit_stack_entry(
            archive,
            &mut seen,
            memory,
            Arc::clone(&reads),
            |pack, declared| {
                let entries = entry_count.saturating_add(pack.entry_count());
                let bytes = declared_bytes.saturating_add(declared);
                if entries > MAX_ENTRIES_PER_STACK {
                    Err(AdmissionError::TooManyStackEntries)
                } else if bytes > MAX_DECLARED_BYTES_PER_STACK {
                    Err(AdmissionError::StackDeclaredSizeTooLarge)
                } else {
                    Ok((entries, bytes))
                }
            },
        );
        match admitted {
            Ok((pack, (entries, declared))) => {
                (entry_count, declared_bytes) = (entries, declared);
                packs.push(pack);
            }
            Err(reason) => rejections.push(PackRejection {
                stack_index,
                reason,
            }),
        }
    }
    packs.reverse();
    let stack = ValidatedPackStack {
        packs: packs.into_boxed_slice(),
        rejections: rejections.into_boxed_slice(),
        fingerprint: Default::default(),
    };
    let _ = stack.fingerprint();
    stack
}

#[cfg(feature = "handoff")]
fn admit_stack_entry<T>(
    archive: ResourcePackArchive,
    seen: &mut HashSet<Uuid>,
    memory: Option<u32>,
    reads: Arc<ReadCache>,
    stack_bounds: impl FnOnce(&ValidatedPack, u64) -> Result<T, AdmissionError>,
) -> Result<(ValidatedPack, T), AdmissionError> {
    if seen.contains(&archive.pack_id) {
        return Err(AdmissionError::DuplicatePack);
    }
    let key = archive.content_key.expose();
    let key = if key.is_empty() {
        None
    } else {
        Some(ContentKey::new(key).ok_or(AdmissionError::InvalidContentKey)?)
    };
    let (pack, declared) = validate_archive_parts_cached(
        archive.pack_id,
        &archive.version,
        &archive.sub_pack_name,
        archive.archive,
        key,
        memory,
        reads,
    )?;
    let bounds = stack_bounds(&pack, declared)?;
    seen.insert(pack.pack_id);
    Ok((pack, bounds))
}

/// Admits one archive with its own bounded read cache.
pub(crate) fn validate_archive_parts(
    pack_id: Uuid,
    version: &str,
    sub_pack_name: &str,
    archive_bytes: Vec<u8>,
    key: Option<ContentKey>,
    memory: Option<u32>,
) -> Result<(ValidatedPack, u64), AdmissionError> {
    validate_archive_parts_cached(
        pack_id,
        version,
        sub_pack_name,
        archive_bytes,
        key,
        memory,
        Arc::default(),
    )
}

/// Admits a pack whose plaintext storage shares the caller's byte budget.
pub(crate) fn validate_archive_parts_cached(
    pack_id: Uuid,
    version: &str,
    sub_pack_name: &str,
    mut archive_bytes: Vec<u8>,
    key: Option<ContentKey>,
    memory: Option<u32>,
    reads: Arc<ReadCache>,
) -> Result<(ValidatedPack, u64), AdmissionError> {
    let expected_entries = prepare_zip_bytes(&mut archive_bytes)?;
    let bytes = Arc::new(archive_bytes);
    let mut zip = ZipArchive::new(Cursor::new(ArchiveBytes(Arc::clone(&bytes))))
        .map_err(|_| AdmissionError::MalformedZip)?;
    if zip.len() != expected_entries || zip.len() > MAX_ENTRIES_PER_PACK {
        return Err(AdmissionError::TooManyEntries);
    }
    let mut physical = HashMap::with_capacity(zip.len());
    let mut folded_seen = HashSet::with_capacity(zip.len());
    // An unreadable index must retain its rejection instead of enabling plaintext fallback.
    let mut contents_paths = HashSet::new();
    let (mut declared, mut skipped) = (0u64, 0usize);
    for archive_index in 0..zip.len() {
        let file = zip
            .by_index_raw(archive_index)
            .map_err(|_| AdmissionError::MalformedZip)?;
        declared = declared
            .checked_add(file.size())
            .filter(|total| *total <= MAX_DECLARED_BYTES_PER_PACK)
            .ok_or(AdmissionError::DeclaredSizeTooLarge)?;
        let path = canonical_path(file.name_raw());
        if let Some(path) = &path
            && path.rsplit('/').next() == Some("contents.json")
        {
            contents_paths.insert(path.clone());
        }
        let usable = !file.is_dir()
            && is_regular_file(file.unix_mode())
            && !file.encrypted()
            && matches!(
                file.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            )
            && file.size() <= MAX_FILE_BYTES;
        match path {
            Some(path) if usable && folded_seen.insert(path.to_ascii_lowercase()) => {
                let entry = EntryIndex {
                    archive_index,
                    uncompressed_size: file.size(),
                    key: None,
                    cached: reads.entry(),
                };
                physical.insert(path, entry);
            }
            _ => skipped += usize::from(!file.is_dir()),
        }
    }
    let root = pack_root(&physical);
    let mut rooted: HashMap<Box<str>, EntryIndex> = physical
        .into_iter()
        .filter_map(|(path, entry)| Some((path.strip_prefix(root.as_str())?.into(), entry)))
        .collect();
    let archive_bytes = bytes.len();
    let keys = match key {
        Some(pack_key) if contents_paths.contains(format!("{root}contents.json").as_str()) => {
            attach_file_keys(&zip, &mut rooted, &pack_key)?
        }
        _ => Box::default(),
    };
    let manifest_path = MANIFEST_NAMES
        .into_iter()
        .find(|path| rooted.contains_key(*path))
        .ok_or(AdmissionError::MissingManifest)?;
    let mut pack = ValidatedPack {
        pack_id,
        version: version.into(),
        sub_pack_name: sub_pack_name.into(),
        archive_bytes,
        declared_bytes: declared,
        zip,
        content_hash: Arc::default(),
        reads,
        files: rooted,
        folded: HashMap::new(),
        file_order: Box::default(),
        keys: keys.into(),
        physical_entry_count: expected_entries,
        skipped_entries: skipped,
    };
    let manifest_bytes = pack
        .read_file_with_limit(manifest_path, MAX_MANIFEST_BYTES as u64)
        .map_err(|error| match error {
            AdmissionError::FileTooLarge => AdmissionError::ManifestTooLarge,
            other => other,
        })?
        .ok_or(AdmissionError::MissingManifest)?;
    let manifest = read_manifest(&manifest_bytes, pack_id, version)?;
    let selected = if let Some(memory) = memory {
        crate::subpacks::supported(&manifest.subpacks, sub_pack_name, memory)
    } else {
        manifest
            .subpacks
            .iter()
            .find(|pack| pack.folder == sub_pack_name)
            .map_or("", |pack| pack.folder.as_str())
    };
    pack.files = logical_files(&pack.files, selected)?;
    if memory.is_some() {
        pack.sub_pack_name = selected.into();
    }
    pack.folded = pack
        .files
        .keys()
        .map(|path| (path.to_ascii_lowercase().into_boxed_str(), path.clone()))
        .collect();
    let mut file_order = pack.files.keys().cloned().collect::<Vec<_>>();
    file_order.sort_unstable();
    pack.file_order = file_order.into_boxed_slice();
    Ok((pack, declared))
}

/// Returns the directory prefix holding the manifest when an archive wraps the
/// pack in one top-level folder.
fn pack_root(physical: &HashMap<Box<str>, EntryIndex>) -> String {
    if MANIFEST_NAMES
        .iter()
        .any(|name| physical.contains_key(*name))
    {
        return String::new();
    }
    let mut roots = physical
        .keys()
        .filter_map(|path| {
            let (folder, rest) = path.split_once('/')?;
            MANIFEST_NAMES.contains(&rest).then(|| format!("{folder}/"))
        })
        .collect::<Vec<_>>();
    roots.sort_unstable();
    roots.into_iter().next().unwrap_or_default()
}

/// Reads a present, bounded contents index and attaches keys only to listed files.
fn attach_file_keys(
    zip: &crate::pack::PackZip,
    rooted: &mut HashMap<Box<str>, EntryIndex>,
    pack_key: &ContentKey,
) -> Result<Box<[ContentKey]>, AdmissionError> {
    let contents = rooted
        .get("contents.json")
        .ok_or(AdmissionError::MissingContentsIndex)?;
    let raw = read_entry(zip, contents, MAX_CONTENTS_INDEX_BYTES)
        .map_err(|_| AdmissionError::MalformedContentsIndex)?;
    let mut file_keys = contents_file_keys(&raw, pack_key)?;
    let mut keys = Vec::with_capacity(file_keys.len());
    for (path, entry) in rooted.iter_mut() {
        if path.as_ref() == "contents.json" {
            continue;
        }
        if let Some(key) = file_keys.remove(path.as_ref()) {
            entry.key = Some(keys.len());
            keys.push(key);
        }
    }
    Ok(keys.into_boxed_slice())
}

/// Maps physical paths to logical ones: the selected subpack overlays the root
/// and every other subpack is hidden. The root manifest cannot be shadowed.
fn logical_files(
    rooted: &HashMap<Box<str>, EntryIndex>,
    sub_pack_name: &str,
) -> Result<HashMap<Box<str>, EntryIndex>, AdmissionError> {
    let mut files = HashMap::with_capacity(rooted.len());
    let mut logical_keys = HashMap::with_capacity(rooted.len());
    for (path, entry) in rooted {
        if !is_physical_subpack_path(path) {
            files.insert(path.clone(), entry.clone());
            logical_keys.insert(path.to_ascii_lowercase(), path.clone());
        }
    }
    if sub_pack_name.is_empty() {
        return Ok(files);
    }
    for (path, entry) in rooted {
        let Some(logical) = selected_subpack_logical_path(path, sub_pack_name) else {
            continue;
        };
        if MANIFEST_NAMES
            .iter()
            .any(|name| logical.eq_ignore_ascii_case(name))
            || logical.is_empty()
        {
            return Err(AdmissionError::InvalidSubpack);
        }
        let logical: Box<str> = logical.into();
        if let Some(root_key) = logical_keys.insert(logical.to_ascii_lowercase(), logical.clone()) {
            files.remove(root_key.as_ref());
        }
        files.insert(logical, entry.clone());
    }
    Ok(files)
}

fn is_physical_subpack_path(path: &str) -> bool {
    path.split('/')
        .next()
        .is_some_and(|part| part.eq_ignore_ascii_case("subpacks"))
}

fn selected_subpack_logical_path<'a>(path: &'a str, selected: &str) -> Option<&'a str> {
    let mut parts = path.splitn(3, '/');
    let root = parts.next()?;
    let name = parts.next()?;
    let logical = parts.next()?;
    (root.eq_ignore_ascii_case("subpacks") && name == selected).then_some(logical)
}

pub(crate) const MANIFEST_NAMES: [&str; 2] = ["manifest.json", "pack_manifest.json"];

/// Checks fixture footer bounds without changing its comment bytes.
#[cfg(test)]
fn preflight_eocd(bytes: &[u8]) -> Result<usize, AdmissionError> {
    zip_footer::find_eocd(bytes, MAX_ENTRIES_PER_PACK)
        .map(|(_, entries)| entries)
        .map_err(AdmissionError::from)
}

/// Removes only the validated ZIP comment so downstream footer searches cannot select its bytes.
pub(crate) fn prepare_zip_bytes(bytes: &mut Vec<u8>) -> Result<usize, AdmissionError> {
    Ok(zip_footer::prepare_zip_bytes(bytes, MAX_ENTRIES_PER_PACK)?)
}

pub(crate) fn is_regular_file(mode: Option<u32>) -> bool {
    mode.is_none_or(|mode| {
        let kind = mode & 0o170000;
        kind == 0 || kind == 0o100000
    })
}

/// Normalizes separators and a leading `./`; paths that could escape the pack
/// root or carry control characters are refused.
pub(crate) fn canonical_path(raw: &[u8]) -> Option<Box<str>> {
    if raw.is_empty() || raw.len() > MAX_PATH_BYTES || raw.contains(&0) {
        return None;
    }
    let path = std::str::from_utf8(raw).ok()?.replace('\\', "/");
    let path = path.trim_start_matches("./");
    let unsafe_path = path.is_empty()
        || path.starts_with('/')
        || path.contains(':')
        || path.ends_with('/')
        || path.bytes().any(|byte| byte.is_ascii_control())
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    (!unsafe_path).then(|| path.into())
}

#[cfg(test)]
mod tests;
