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
    AdmissionError, MAX_ARCHIVE_BYTES, MAX_DECLARED_BYTES_PER_PACK, MAX_ENTRIES_PER_PACK,
    MAX_FILE_BYTES, MAX_MANIFEST_BYTES, MAX_PATH_BYTES,
    crypto::{ContentKey, contents_file_keys},
    manifest::read_manifest,
    pack::{EntryIndex, ValidatedPack, read_entry},
};
#[cfg(feature = "handoff")]
use crate::{
    MAX_DECLARED_BYTES_PER_STACK, MAX_ENTRIES_PER_STACK, MAX_PACKS, MAX_STACK_ARCHIVE_BYTES,
    PackRejection, ValidatedPackStack,
};

const EOCD_MIN_BYTES: usize = 22;
const EOCD_MAX_SEARCH_BYTES: usize = EOCD_MIN_BYTES + u16::MAX as usize;
const MAX_CONTENTS_INDEX_BYTES: u64 = 4 * 1024 * 1024;

/// Validates one unencrypted archive under a nil identity; used by fuzzing.
pub fn validate_archive_bytes(bytes: &[u8]) -> Result<(), AdmissionError> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(AdmissionError::ArchiveTooLarge);
    }
    validate_archive_parts(Uuid::nil(), "0.0.0", "", bytes.to_vec(), None, None).map(|_| ())
}

/// Admits each archive independently in stack order. Stack-wide bounds drop
/// the packs that would exceed them, not the packs already admitted.
#[cfg(feature = "handoff")]
pub(super) fn validate_stack(
    archives: Vec<ResourcePackArchive>,
    memory: Option<u32>,
) -> ValidatedPackStack {
    let mut packs = Vec::with_capacity(archives.len().min(MAX_PACKS));
    let mut rejections = Vec::new();
    let (mut archive_bytes, mut entry_count, mut declared_bytes) = (0usize, 0usize, 0u64);
    let mut seen = HashSet::with_capacity(archives.len());
    for (stack_index, archive) in archives.into_iter().enumerate() {
        let admitted = admit_stack_entry(
            archive,
            &mut seen,
            packs.len(),
            archive_bytes,
            memory,
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
            Ok((pack, size, (entries, declared))) => {
                archive_bytes += size;
                (entry_count, declared_bytes) = (entries, declared);
                packs.push(pack);
            }
            Err(reason) => rejections.push(PackRejection {
                stack_index,
                reason,
            }),
        }
    }
    ValidatedPackStack {
        packs: packs.into_boxed_slice(),
        rejections: rejections.into_boxed_slice(),
    }
}

#[cfg(feature = "handoff")]
fn admit_stack_entry<T>(
    archive: ResourcePackArchive,
    seen: &mut HashSet<Uuid>,
    admitted: usize,
    archive_bytes: usize,
    memory: Option<u32>,
    stack_bounds: impl FnOnce(&ValidatedPack, u64) -> Result<T, AdmissionError>,
) -> Result<(ValidatedPack, usize, T), AdmissionError> {
    if admitted >= MAX_PACKS {
        return Err(AdmissionError::TooManyPacks);
    }
    let size = archive.archive.len();
    if size > MAX_ARCHIVE_BYTES {
        return Err(AdmissionError::ArchiveTooLarge);
    }
    if archive_bytes.saturating_add(size) > MAX_STACK_ARCHIVE_BYTES {
        return Err(AdmissionError::StackArchiveTooLarge);
    }
    if seen.contains(&archive.pack_id) {
        return Err(AdmissionError::DuplicatePack);
    }
    let key = archive.content_key.expose();
    let key = if key.is_empty() {
        None
    } else {
        Some(ContentKey::new(key).ok_or(AdmissionError::InvalidContentKey)?)
    };
    let (pack, declared) = validate_archive_parts(
        archive.pack_id,
        &archive.version,
        &archive.sub_pack_name,
        archive.archive,
        key,
        memory,
    )?;
    let bounds = stack_bounds(&pack, declared)?;
    seen.insert(pack.pack_id);
    Ok((pack, size, bounds))
}

pub(crate) fn validate_archive_parts(
    pack_id: Uuid,
    version: &str,
    sub_pack_name: &str,
    mut archive_bytes: Vec<u8>,
    key: Option<ContentKey>,
    memory: Option<u32>,
) -> Result<(ValidatedPack, u64), AdmissionError> {
    if archive_bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(AdmissionError::ArchiveTooLarge);
    }
    let expected_entries = prepare_zip_bytes(&mut archive_bytes)?;
    let bytes: Arc<[u8]> = archive_bytes.into();
    let mut zip = ZipArchive::new(Cursor::new(Arc::clone(&bytes)))
        .map_err(|_| AdmissionError::MalformedZip)?;
    if zip.len() != expected_entries || zip.len() > MAX_ENTRIES_PER_PACK {
        return Err(AdmissionError::TooManyEntries);
    }
    let mut physical = HashMap::with_capacity(zip.len());
    let mut folded_seen = HashSet::with_capacity(zip.len());
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
        Some(pack_key) => attach_file_keys(&zip, &mut rooted, &pack_key)?,
        None => Box::default(),
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
    find_eocd(bytes).map(|(_, entries)| entries)
}

/// Removes only the validated ZIP comment so downstream footer searches cannot select its bytes.
pub(crate) fn prepare_zip_bytes(bytes: &mut Vec<u8>) -> Result<usize, AdmissionError> {
    let (footer, entries) = find_eocd(bytes)?;
    bytes[footer + 20..footer + 22].fill(0);
    bytes.truncate(footer + EOCD_MIN_BYTES);
    Ok(entries)
}

/// Finds the last structurally valid footer rather than an arbitrary signature in its comment.
fn find_eocd(bytes: &[u8]) -> Result<(usize, usize), AdmissionError> {
    if bytes.len() < EOCD_MIN_BYTES {
        return Err(AdmissionError::InvalidZipFooter);
    }
    let start = bytes.len().saturating_sub(EOCD_MAX_SEARCH_BYTES);
    let signature = b"PK\x05\x06";
    let mut failure = AdmissionError::InvalidZipFooter;
    for eocd in (start..=bytes.len() - EOCD_MIN_BYTES)
        .rev()
        .filter(|offset| bytes[*offset..].starts_with(signature))
    {
        match validate_eocd_at(bytes, eocd) {
            Ok(entries) => return Ok((eocd, entries)),
            Err(error) => failure = error,
        }
    }
    Err(failure)
}

/// Validates one footer candidate, including its central directory and exact comment extent.
fn validate_eocd_at(bytes: &[u8], eocd: usize) -> Result<usize, AdmissionError> {
    let tail = &bytes[eocd..];
    let disk = le_u16(tail, 4)?;
    let central_disk = le_u16(tail, 6)?;
    let disk_entries = le_u16(tail, 8)?;
    let total_entries = le_u16(tail, 10)?;
    let central_size = le_u32(tail, 12)?;
    let central_offset = le_u32(tail, 16)?;
    let comment_len = usize::from(le_u16(tail, 20)?);
    if eocd + EOCD_MIN_BYTES + comment_len != bytes.len() || disk != 0 || central_disk != 0 {
        return Err(AdmissionError::InvalidZipFooter);
    }
    if disk_entries == u16::MAX
        || total_entries == u16::MAX
        || central_size == u32::MAX
        || central_offset == u32::MAX
        || eocd >= 20 && bytes[eocd - 20..eocd].starts_with(b"PK\x06\x07")
    {
        return Err(AdmissionError::UnsupportedZip64);
    }
    if disk_entries != total_entries || usize::from(total_entries) > MAX_ENTRIES_PER_PACK {
        return Err(AdmissionError::TooManyEntries);
    }
    let central_end = usize::try_from(central_offset)
        .ok()
        .and_then(|offset| {
            usize::try_from(central_size)
                .ok()
                .and_then(|size| offset.checked_add(size))
        })
        .ok_or(AdmissionError::InvalidZipFooter)?;
    if central_end > eocd {
        return Err(AdmissionError::InvalidZipFooter);
    }
    if central_end != eocd {
        let gap = &bytes[central_end..eocd];
        if gap
            .windows(4)
            .any(|window| window == b"PK\x06\x06" || window == b"PK\x06\x07")
        {
            return Err(AdmissionError::UnsupportedZip64);
        }
        return Err(AdmissionError::InvalidZipFooter);
    }
    Ok(usize::from(total_entries))
}

fn le_u16(bytes: &[u8], offset: usize) -> Result<u16, AdmissionError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(AdmissionError::InvalidZipFooter)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

fn le_u32(bytes: &[u8], offset: usize) -> Result<u32, AdmissionError> {
    let value = bytes
        .get(offset..offset + 4)
        .ok_or(AdmissionError::InvalidZipFooter)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
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
