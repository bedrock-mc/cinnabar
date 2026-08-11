//! Internal bounded archive and manifest parser.
//!
//! Admission indexes each archive and validates its manifest atomically. It does
//! not extract archives, merge pack namespaces, or apply assets.

use std::{
    collections::{HashMap, HashSet},
    io::Cursor,
    sync::Arc,
};

#[cfg(feature = "handoff")]
use protocol::ResourcePackArchive;
use serde::Deserialize;
use uuid::Uuid;
use zip::{CompressionMethod, ZipArchive};

use crate::{
    AdmissionError, EntryIndex, MAX_ARCHIVE_BYTES, MAX_DECLARED_BYTES_PER_PACK, MAX_DEPENDENCIES,
    MAX_ENTRIES_PER_PACK, MAX_FILE_BYTES, MAX_MANIFEST_BYTES, MAX_MANIFEST_STRING_BYTES,
    MAX_MODULES, MAX_PATH_BYTES, MAX_SUBPACKS, ValidatedPack,
};
#[cfg(feature = "handoff")]
use crate::{
    MAX_DECLARED_BYTES_PER_STACK, MAX_ENTRIES_PER_STACK, MAX_PACKS, MAX_STACK_ARCHIVE_BYTES,
    ValidatedPackStack,
};

const EOCD_MIN_BYTES: usize = 22;
const EOCD_MAX_SEARCH_BYTES: usize = EOCD_MIN_BYTES + u16::MAX as usize;

pub fn validate_archive_bytes(bytes: &[u8]) -> Result<(), AdmissionError> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(AdmissionError::ArchiveTooLarge);
    }
    validate_archive_parts(Uuid::nil(), "0.0.0", "", bytes.to_vec()).map(|_| ())
}

#[cfg(feature = "handoff")]
pub(super) fn validate_archives(
    archives: Vec<ResourcePackArchive>,
) -> Result<ValidatedPackStack, AdmissionError> {
    if archives.len() > MAX_PACKS {
        return Err(AdmissionError::TooManyPacks);
    }
    // Never copy or format content keys. Reject before ZIP or manifest parsing.
    if archives
        .iter()
        .any(|archive| !archive.content_key.expose().is_empty())
    {
        return Err(AdmissionError::UnsupportedContentEncryption);
    }
    let mut archive_bytes = 0usize;
    let mut entry_count = 0usize;
    let mut declared_bytes = 0u64;
    let mut packs = Vec::with_capacity(archives.len());
    for archive in archives {
        if archive.archive.len() > MAX_ARCHIVE_BYTES {
            return Err(AdmissionError::ArchiveTooLarge);
        }
        archive_bytes = archive_bytes
            .checked_add(archive.archive.len())
            .ok_or(AdmissionError::StackArchiveTooLarge)?;
        if archive_bytes > MAX_STACK_ARCHIVE_BYTES {
            return Err(AdmissionError::StackArchiveTooLarge);
        }
        let (pack, pack_declared) = validate_archive(archive)?;
        entry_count = entry_count
            .checked_add(pack.entry_count())
            .ok_or(AdmissionError::TooManyStackEntries)?;
        if entry_count > MAX_ENTRIES_PER_STACK {
            return Err(AdmissionError::TooManyStackEntries);
        }
        declared_bytes = declared_bytes
            .checked_add(pack_declared)
            .ok_or(AdmissionError::StackDeclaredSizeTooLarge)?;
        if declared_bytes > MAX_DECLARED_BYTES_PER_STACK {
            return Err(AdmissionError::StackDeclaredSizeTooLarge);
        }
        packs.push(pack);
    }
    validate_dependency_graph(&packs)?;
    Ok(ValidatedPackStack {
        packs: packs.into_boxed_slice(),
    })
}

#[cfg(feature = "handoff")]
fn validate_archive(archive: ResourcePackArchive) -> Result<(ValidatedPack, u64), AdmissionError> {
    validate_archive_parts(
        archive.pack_id,
        &archive.version,
        &archive.sub_pack_name,
        archive.archive,
    )
}

fn validate_archive_parts(
    pack_id: Uuid,
    version: &str,
    sub_pack_name: &str,
    archive_bytes: Vec<u8>,
) -> Result<(ValidatedPack, u64), AdmissionError> {
    if archive_bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(AdmissionError::ArchiveTooLarge);
    }
    if version.len() > MAX_MANIFEST_STRING_BYTES || sub_pack_name.len() > MAX_MANIFEST_STRING_BYTES
    {
        return Err(AdmissionError::ManifestStringTooLong);
    }
    let expected_entries = preflight_eocd(&archive_bytes)?;
    let bytes: Arc<[u8]> = archive_bytes.into();
    let mut zip = ZipArchive::new(Cursor::new(Arc::clone(&bytes)))
        .map_err(|_| AdmissionError::MalformedZip)?;
    if zip.len() != expected_entries || zip.len() > MAX_ENTRIES_PER_PACK {
        return Err(AdmissionError::TooManyEntries);
    }
    let mut physical_entries = HashMap::with_capacity(zip.len());
    let mut folded_paths = HashSet::with_capacity(zip.len());
    let mut declared = 0u64;
    for archive_index in 0..zip.len() {
        let file = zip
            .by_index_raw(archive_index)
            .map_err(|_| AdmissionError::MalformedZip)?;
        if file.encrypted() {
            return Err(AdmissionError::UnsupportedZipEncryption);
        }
        if !matches!(
            file.compression(),
            CompressionMethod::Stored | CompressionMethod::Deflated
        ) {
            return Err(AdmissionError::UnsupportedCompression);
        }
        if file.is_dir() || !is_regular_file(file.unix_mode()) {
            return Err(AdmissionError::NonFileEntry);
        }
        if file.size() > MAX_FILE_BYTES {
            return Err(AdmissionError::FileTooLarge);
        }
        declared = declared
            .checked_add(file.size())
            .ok_or(AdmissionError::DeclaredSizeTooLarge)?;
        if declared > MAX_DECLARED_BYTES_PER_PACK {
            return Err(AdmissionError::DeclaredSizeTooLarge);
        }
        let path = canonical_path(file.name_raw())?;
        let folded = path.to_lowercase();
        if !folded_paths.insert(folded) || physical_entries.contains_key(path.as_ref()) {
            return Err(AdmissionError::DuplicatePath);
        }
        physical_entries.insert(
            path,
            EntryIndex {
                archive_index,
                uncompressed_size: file.size(),
            },
        );
    }
    let manifest_path: Box<str> = "manifest.json".into();
    let manifest_entry = physical_entries
        .get(manifest_path.as_ref())
        .cloned()
        .ok_or(AdmissionError::MissingManifest)?;
    if manifest_entry.uncompressed_size > MAX_MANIFEST_BYTES as u64 {
        return Err(AdmissionError::ManifestTooLarge);
    }
    let mut files = HashMap::with_capacity(physical_entries.len());
    let mut logical_keys = HashMap::with_capacity(physical_entries.len());
    for (path, entry) in &physical_entries {
        if !is_physical_subpack_path(path) {
            files.insert(path.clone(), entry.clone());
            logical_keys.insert(path.to_lowercase(), path.clone());
        }
    }
    if !sub_pack_name.is_empty() {
        for (path, entry) in &physical_entries {
            if let Some(logical) = selected_subpack_logical_path(path, sub_pack_name) {
                if logical.eq_ignore_ascii_case("manifest.json") || logical.is_empty() {
                    return Err(AdmissionError::InvalidSubpack);
                }
                let logical: Box<str> = logical.into();
                if let Some(root_key) = logical_keys.insert(logical.to_lowercase(), logical.clone())
                {
                    files.remove(root_key.as_ref());
                }
                files.insert(logical, entry.clone());
            }
        }
    }
    // The root manifest is identity authority and cannot be shadowed by a subpack.
    files.insert(manifest_path.clone(), manifest_entry);
    let mut file_order = files.keys().cloned().collect::<Vec<_>>();
    file_order.sort_unstable();
    let mut pack = ValidatedPack {
        pack_id,
        version: version.into(),
        sub_pack_name: sub_pack_name.into(),
        archive: bytes,
        files,
        file_order: file_order.into_boxed_slice(),
        dependencies: Box::new([]),
        physical_entry_count: expected_entries,
    };
    let manifest_bytes = pack
        .read_file_with_limit(&manifest_path, MAX_MANIFEST_BYTES as u64)?
        .ok_or(AdmissionError::MissingManifest)?;
    if manifest_bytes.len() > MAX_MANIFEST_BYTES {
        return Err(AdmissionError::ManifestTooLarge);
    }
    pack.dependencies = validate_manifest(&pack, &manifest_bytes)?.into_boxed_slice();
    Ok((pack, declared))
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

fn preflight_eocd(bytes: &[u8]) -> Result<usize, AdmissionError> {
    if bytes.len() < EOCD_MIN_BYTES {
        return Err(AdmissionError::InvalidZipFooter);
    }
    let start = bytes.len().saturating_sub(EOCD_MAX_SEARCH_BYTES);
    let signature = b"PK\x05\x06";
    let eocd = (start..=bytes.len() - EOCD_MIN_BYTES)
        .rev()
        .find(|offset| bytes[*offset..].starts_with(signature))
        .ok_or(AdmissionError::InvalidZipFooter)?;
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

fn is_regular_file(mode: Option<u32>) -> bool {
    mode.is_none_or(|mode| {
        let kind = mode & 0o170000;
        kind == 0 || kind == 0o100000
    })
}

fn canonical_path(raw: &[u8]) -> Result<Box<str>, AdmissionError> {
    if raw.is_empty() || raw.len() > MAX_PATH_BYTES || raw.contains(&0) {
        return Err(AdmissionError::UnsafePath);
    }
    let path = std::str::from_utf8(raw).map_err(|_| AdmissionError::UnsafePath)?;
    if path.starts_with('/')
        || path.starts_with('\\')
        || path.contains('\\')
        || path.contains(':')
        || path.ends_with('/')
        || path.bytes().any(|byte| byte.is_ascii_control())
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(AdmissionError::UnsafePath);
    }
    Ok(path.into())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format_version: u32,
    header: Header,
    modules: Vec<Module>,
    #[serde(default)]
    dependencies: Vec<Dependency>,
    #[serde(default)]
    subpacks: Vec<Subpack>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    name: String,
    description: String,
    uuid: Uuid,
    version: Version,
    #[serde(default)]
    min_engine_version: Option<Version>,
    #[serde(default)]
    pack_scope: Option<String>,
    #[serde(default)]
    lock_template_options: Option<bool>,
    #[serde(default)]
    allow_random_seed: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Module {
    #[serde(default)]
    description: String,
    #[serde(rename = "type")]
    module_type: String,
    uuid: Uuid,
    version: Version,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Dependency {
    uuid: Uuid,
    version: Version,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Subpack {
    folder_name: String,
    name: String,
    memory_tier: u32,
}

#[derive(Clone, Copy, Deserialize, Eq, PartialEq)]
pub(super) struct Version([u32; 3]);

impl Version {
    fn parse_canonical(value: &str) -> Result<Self, AdmissionError> {
        let mut parts = value.split('.');
        let mut parsed = [0; 3];
        for part in &mut parsed {
            let text = parts.next().ok_or(AdmissionError::InvalidVersion)?;
            if text.is_empty()
                || (text.len() > 1 && text.starts_with('0'))
                || !text.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(AdmissionError::InvalidVersion);
            }
            *part = text.parse().map_err(|_| AdmissionError::InvalidVersion)?;
        }
        if parts.next().is_some() {
            return Err(AdmissionError::InvalidVersion);
        }
        Ok(Self(parsed))
    }
}

fn validate_manifest(
    pack: &ValidatedPack,
    bytes: &[u8],
) -> Result<Vec<(Uuid, Version)>, AdmissionError> {
    let stripped = strip_jsonc(bytes)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&stripped);
    let manifest =
        Manifest::deserialize(&mut deserializer).map_err(|_| AdmissionError::MalformedManifest)?;
    deserializer
        .end()
        .map_err(|_| AdmissionError::MalformedManifest)?;
    if manifest.format_version != 2 {
        return Err(AdmissionError::UnsupportedManifestFormat);
    }
    validate_manifest_strings(&manifest)?;
    if manifest.header.uuid != pack.pack_id
        || manifest.header.version != Version::parse_canonical(&pack.version)?
    {
        return Err(AdmissionError::ManifestIdentityMismatch);
    }
    if manifest.modules.is_empty() || manifest.modules.len() > MAX_MODULES {
        return Err(AdmissionError::InvalidModules);
    }
    let mut module_ids = HashSet::with_capacity(manifest.modules.len());
    let mut has_resources = false;
    for module in &manifest.modules {
        if module.module_type != "resources" || !module_ids.insert(module.uuid) {
            return Err(AdmissionError::InvalidModules);
        }
        let _ = module.version;
        has_resources = true;
    }
    if !has_resources {
        return Err(AdmissionError::InvalidModules);
    }
    if manifest.dependencies.len() > MAX_DEPENDENCIES {
        return Err(AdmissionError::InvalidDependencies);
    }
    let mut dependencies = HashSet::with_capacity(manifest.dependencies.len());
    if manifest
        .dependencies
        .iter()
        .any(|dependency| !dependencies.insert(dependency.uuid) || dependency.uuid == pack.pack_id)
    {
        return Err(AdmissionError::InvalidDependencies);
    }
    if manifest.subpacks.len() > MAX_SUBPACKS {
        return Err(AdmissionError::InvalidSubpack);
    }
    let mut subpack_names = HashSet::with_capacity(manifest.subpacks.len());
    if manifest.subpacks.iter().any(|subpack| {
        let _ = subpack.memory_tier;
        subpack.folder_name.is_empty()
            || canonical_path(subpack.folder_name.as_bytes()).is_err()
            || subpack.folder_name.contains('/')
            || !subpack_names.insert(subpack.folder_name.to_lowercase())
    }) {
        return Err(AdmissionError::InvalidSubpack);
    }
    if !pack.sub_pack_name.is_empty()
        && !manifest
            .subpacks
            .iter()
            .any(|subpack| subpack.folder_name == pack.sub_pack_name.as_ref())
    {
        return Err(AdmissionError::InvalidSubpack);
    }
    Ok(manifest
        .dependencies
        .into_iter()
        .map(|dependency| (dependency.uuid, dependency.version))
        .collect())
}

fn validate_manifest_strings(manifest: &Manifest) -> Result<(), AdmissionError> {
    let strings = std::iter::once(manifest.header.name.as_str())
        .chain(std::iter::once(manifest.header.description.as_str()))
        .chain(manifest.header.pack_scope.iter().map(String::as_str))
        .chain(manifest.capabilities.iter().map(String::as_str))
        .chain(
            manifest
                .modules
                .iter()
                .flat_map(|module| [module.description.as_str(), module.module_type.as_str()]),
        )
        .chain(
            manifest
                .subpacks
                .iter()
                .flat_map(|subpack| [subpack.folder_name.as_str(), subpack.name.as_str()]),
        );
    if strings
        .into_iter()
        .any(|value| value.len() > MAX_MANIFEST_STRING_BYTES)
    {
        return Err(AdmissionError::ManifestStringTooLong);
    }
    if let Some(metadata) = &manifest.metadata {
        validate_json_strings(metadata)?;
    }
    // Consume optional fields so they remain intentionally admitted and visible to review.
    let _ = (
        manifest.header.min_engine_version,
        manifest.header.lock_template_options,
        manifest.header.allow_random_seed,
        &manifest.metadata,
    );
    Ok(())
}

fn validate_json_strings(value: &serde_json::Value) -> Result<(), AdmissionError> {
    match value {
        serde_json::Value::String(value) => {
            if value.len() > MAX_MANIFEST_STRING_BYTES {
                return Err(AdmissionError::ManifestStringTooLong);
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                validate_json_strings(value)?;
            }
        }
        serde_json::Value::Object(values) => {
            for (key, value) in values {
                if key.len() > MAX_MANIFEST_STRING_BYTES {
                    return Err(AdmissionError::ManifestStringTooLong);
                }
                validate_json_strings(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(any(feature = "handoff", test))]
fn validate_dependency_graph(packs: &[ValidatedPack]) -> Result<(), AdmissionError> {
    let positions: HashMap<Uuid, usize> = packs
        .iter()
        .enumerate()
        .map(|(index, pack)| (pack.pack_id, index))
        .collect();
    if positions.len() != packs.len() {
        return Err(AdmissionError::InvalidDependencies);
    }
    let mut edges = vec![Vec::new(); packs.len()];
    for (index, pack) in packs.iter().enumerate() {
        for &(dependency_id, dependency_version) in &pack.dependencies {
            let Some(&target) = positions.get(&dependency_id) else {
                return Err(AdmissionError::InvalidDependencies);
            };
            if packs[target].version.as_ref()
                != canonical_version_string(dependency_version).as_str()
            {
                return Err(AdmissionError::InvalidDependencies);
            }
            edges[index].push(target);
        }
    }
    let mut states = vec![0u8; packs.len()];
    for root in 0..packs.len() {
        visit(root, &edges, &mut states)?;
    }
    Ok(())
}

#[cfg(any(feature = "handoff", test))]
fn visit(index: usize, edges: &[Vec<usize>], states: &mut [u8]) -> Result<(), AdmissionError> {
    match states[index] {
        1 => return Err(AdmissionError::DependencyCycle),
        2 => return Ok(()),
        _ => {}
    }
    states[index] = 1;
    for &next in &edges[index] {
        visit(next, edges, states)?;
    }
    states[index] = 2;
    Ok(())
}

#[cfg(any(feature = "handoff", test))]
fn canonical_version_string(version: Version) -> String {
    format!("{}.{}.{}", version.0[0], version.0[1], version.0[2])
}

fn strip_jsonc(bytes: &[u8]) -> Result<Vec<u8>, AdmissionError> {
    std::str::from_utf8(bytes).map_err(|_| AdmissionError::MalformedManifest)?;
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            output.push(byte);
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
            continue;
        }
        if byte == b'"' {
            in_string = true;
            output.push(byte);
            index += 1;
        } else if bytes.get(index..index + 2) == Some(b"//") {
            output.extend_from_slice(b"  ");
            index += 2;
            while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                output.push(b' ');
                index += 1;
            }
        } else if bytes.get(index..index + 2) == Some(b"/*") {
            output.extend_from_slice(b"  ");
            index += 2;
            let mut terminated = false;
            while index < bytes.len() {
                if bytes.get(index..index + 2) == Some(b"*/") {
                    output.extend_from_slice(b"  ");
                    index += 2;
                    terminated = true;
                    break;
                }
                output.push(if matches!(bytes[index], b'\n' | b'\r') {
                    bytes[index]
                } else {
                    b' '
                });
                index += 1;
            }
            if !terminated {
                return Err(AdmissionError::MalformedManifest);
            }
        } else {
            output.push(byte);
            index += 1;
        }
    }
    if in_string {
        return Err(AdmissionError::MalformedManifest);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use zip::{ZipWriter, write::SimpleFileOptions};

    use super::*;

    const PACK_ID: Uuid = Uuid::from_u128(0x11111111_2222_3333_4444_555555555555);
    const MODULE_ID: Uuid = Uuid::from_u128(0xaaaaaaaa_bbbb_cccc_dddd_eeeeeeeeeeee);

    fn manifest(extra: &str) -> String {
        format!(
            r#"{{
                // numeric v2 manifests are admitted
                "format_version": 2,
                "header": {{
                    "name": "test // literal",
                    "description": "bounded",
                    "uuid": "{PACK_ID}",
                    "version": [1, 2, 3]
                }},
                "modules": [{{
                    "type": "resources",
                    "uuid": "{MODULE_ID}",
                    "version": [1, 2, 3]
                }}]
                {extra}
            }}"#
        )
    }

    fn zip_files(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, bytes) in files {
            writer
                .start_file(*path, SimpleFileOptions::default())
                .expect("start fixture file");
            writer.write_all(bytes).expect("write fixture file");
        }
        writer.finish().expect("finish fixture ZIP").into_inner()
    }

    fn deflated_zip_file(path: &str, bytes: &[u8]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file(
                path,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .expect("start deflated fixture file");
        writer
            .write_all(bytes)
            .expect("write deflated fixture file");
        writer.finish().expect("finish fixture ZIP").into_inner()
    }

    fn validate_fixture(bytes: Vec<u8>, selected: &str) -> Result<ValidatedPack, AdmissionError> {
        validate_archive_parts(PACK_ID, "1.2.3", selected, bytes).map(|(pack, _)| pack)
    }
    #[test]
    fn admits_jsonc_manifest_and_exposes_only_selected_logical_namespace() {
        let manifest = manifest(
            r#", "subpacks": [
                {"folder_name":"low", "name":"Low", "memory_tier":1},
                {"folder_name":"high", "name":"High", "memory_tier":2}
            ] /* a block comment */"#,
        );
        let archive = zip_files(&[
            ("manifest.json", manifest.as_bytes()),
            ("textures/root.txt", b"root"),
            ("subpacks/low/textures/root.txt", b"low"),
            ("subpacks/high/textures/root.txt", b"high"),
            ("subpacks/high/textures/only.txt", b"only"),
        ]);
        let pack = validate_fixture(archive, "high").expect("valid selected subpack");

        assert_eq!(
            pack.read_file("textures/root.txt")
                .unwrap()
                .unwrap()
                .as_ref(),
            b"high"
        );
        assert_eq!(
            pack.read_file("textures/only.txt")
                .unwrap()
                .unwrap()
                .as_ref(),
            b"only"
        );
        assert!(
            pack.read_file("subpacks/low/textures/root.txt")
                .unwrap()
                .is_none()
        );
        assert_eq!(
            pack.files_under("textures/").as_ref(),
            ["textures/only.txt", "textures/root.txt"]
        );
    }
    #[test]
    fn root_selection_excludes_all_physical_subpacks() {
        let manifest =
            manifest(r#", "subpacks": [{"folder_name":"high", "name":"High", "memory_tier":2}]"#);
        let archive = zip_files(&[
            ("manifest.json", manifest.as_bytes()),
            ("base.txt", b"root"),
            ("subpacks/high/base.txt", b"high"),
        ]);
        let pack = validate_fixture(archive, "").expect("root selection");
        assert_eq!(
            pack.read_file("base.txt").unwrap().unwrap().as_ref(),
            b"root"
        );
        assert_eq!(pack.files_under("").as_ref(), ["base.txt", "manifest.json"]);
    }
    #[test]
    fn rejects_unsafe_duplicate_and_nonfile_entries() {
        let manifest = manifest("");
        let traversal = zip_files(&[("manifest.json", manifest.as_bytes()), ("../x", b"x")]);
        assert_eq!(
            validate_fixture(traversal, "").unwrap_err(),
            AdmissionError::UnsafePath
        );

        let collision = zip_files(&[
            ("manifest.json", manifest.as_bytes()),
            ("Textures/x", b"a"),
            ("textures/X", b"b"),
        ]);
        assert_eq!(
            validate_fixture(collision, "").unwrap_err(),
            AdmissionError::DuplicatePath
        );

        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_directory("directory/", SimpleFileOptions::default())
            .unwrap();
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(manifest.as_bytes()).unwrap();
        let directory = writer.finish().unwrap().into_inner();
        assert_eq!(
            validate_fixture(directory, "").unwrap_err(),
            AdmissionError::NonFileEntry
        );

        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink("link", "target", SimpleFileOptions::default())
            .unwrap();
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(manifest.as_bytes()).unwrap();
        let symlink = writer.finish().unwrap().into_inner();
        assert_eq!(
            validate_fixture(symlink, "").unwrap_err(),
            AdmissionError::NonFileEntry
        );
    }
    #[test]
    fn rejects_zip64_sentinel_before_zip_parser_allocation() {
        let manifest = manifest("");
        let mut archive = zip_files(&[("manifest.json", manifest.as_bytes())]);
        let eocd = archive
            .windows(4)
            .rposition(|window| window == b"PK\x05\x06")
            .unwrap();
        archive[eocd + 10..eocd + 12].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(
            preflight_eocd(&archive),
            Err(AdmissionError::UnsupportedZip64)
        );
    }
    #[test]
    fn rejects_zip_encryption_and_unsupported_compression() {
        let manifest = manifest("");
        let original = zip_files(&[("manifest.json", manifest.as_bytes())]);

        let mut encrypted = original.clone();
        let local = encrypted
            .windows(4)
            .position(|window| window == b"PK\x03\x04")
            .unwrap();
        let central = encrypted
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .unwrap();
        encrypted[local + 6..local + 8].copy_from_slice(&1u16.to_le_bytes());
        encrypted[central + 8..central + 10].copy_from_slice(&1u16.to_le_bytes());
        assert_eq!(
            validate_fixture(encrypted, "").unwrap_err(),
            AdmissionError::UnsupportedZipEncryption
        );

        let mut unsupported = original;
        unsupported[local + 8..local + 10].copy_from_slice(&12u16.to_le_bytes());
        unsupported[central + 10..central + 12].copy_from_slice(&12u16.to_le_bytes());
        assert_eq!(
            validate_fixture(unsupported, "").unwrap_err(),
            AdmissionError::UnsupportedCompression
        );
    }
    #[test]
    fn rejects_malformed_jsonc_and_multiple_json_values() {
        for body in [
            br#"{"format_version":2 /* unterminated"#.as_slice(),
            br#"{} {}"#.as_slice(),
        ] {
            let archive = zip_files(&[("manifest.json", body)]);
            assert_eq!(
                validate_fixture(archive, "").unwrap_err(),
                AdmissionError::MalformedManifest
            );
        }

        let invalid_utf8_comment = b"// \xff\n{}";
        let archive = zip_files(&[("manifest.json", invalid_utf8_comment)]);
        assert_eq!(
            validate_fixture(archive, "").unwrap_err(),
            AdmissionError::MalformedManifest
        );
    }

    #[test]
    fn manifest_read_is_bounded_by_its_forged_declared_size() {
        let body = manifest("");
        let mut archive = deflated_zip_file("manifest.json", body.as_bytes());
        let local = archive
            .windows(4)
            .position(|window| window == b"PK\x03\x04")
            .unwrap();
        let central = archive
            .windows(4)
            .position(|window| window == b"PK\x01\x02")
            .unwrap();
        archive[local + 22..local + 26].copy_from_slice(&1u32.to_le_bytes());
        archive[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());

        assert_eq!(
            validate_fixture(archive, "").unwrap_err(),
            AdmissionError::InvalidFileData
        );
    }

    #[test]
    fn rejects_cycles_in_the_exact_selected_dependency_graph() {
        let other = Uuid::from_u128(0x99999999_8888_7777_6666_555555555555);
        let first_manifest = manifest(&format!(
            r#", "dependencies": [{{"uuid":"{other}", "version":[1,2,3]}}]"#
        ));
        let second_manifest = manifest(&format!(
            r#", "dependencies": [{{"uuid":"{PACK_ID}", "version":[1,2,3]}}]"#
        ))
        .replacen(&PACK_ID.to_string(), &other.to_string(), 1);
        let first = validate_archive_parts(
            PACK_ID,
            "1.2.3",
            "",
            zip_files(&[("manifest.json", first_manifest.as_bytes())]),
        )
        .unwrap()
        .0;
        let second = validate_archive_parts(
            other,
            "1.2.3",
            "",
            zip_files(&[("manifest.json", second_manifest.as_bytes())]),
        )
        .unwrap()
        .0;
        assert_eq!(
            validate_dependency_graph(&[first, second]),
            Err(AdmissionError::DependencyCycle)
        );
    }

    #[test]
    fn admits_public_reference_scale_entry_count() {
        const REFERENCE_ENTRY_COUNT: usize = 17_805;
        let manifest = manifest("");
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(manifest.as_bytes()).unwrap();
        for index in 1..REFERENCE_ENTRY_COUNT {
            writer
                .start_file(
                    format!("textures/generated/{index:05}.txt"),
                    SimpleFileOptions::default(),
                )
                .unwrap();
        }
        let archive = writer.finish().unwrap().into_inner();
        let pack = validate_fixture(archive, "").expect("reference-scale central directory");
        assert_eq!(pack.entry_count(), REFERENCE_ENTRY_COUNT);
    }

    #[test]
    fn maximum_entry_subpack_overlay_uses_bounded_key_index() {
        let manifest =
            manifest(r#", "subpacks": [{"folder_name":"high", "name":"High", "memory_tier":2}]"#);
        let common = format!("assets/{}/", "a".repeat(420));
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("manifest.json", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(manifest.as_bytes()).unwrap();
        for index in 0..16_383 {
            writer
                .start_file(
                    format!("{common}{index:05}.txt"),
                    SimpleFileOptions::default(),
                )
                .unwrap();
        }
        for index in 0..16_384 {
            writer
                .start_file(
                    format!("subpacks/high/{common}{index:05}.txt"),
                    SimpleFileOptions::default(),
                )
                .unwrap();
        }
        let archive = writer.finish().unwrap().into_inner();
        let pack = validate_fixture(archive, "high").expect("maximum-entry selected subpack");
        assert_eq!(pack.entry_count(), MAX_ENTRIES_PER_PACK);
        assert_eq!(pack.files_under("assets/").len(), 16_384);
    }

    #[test]
    fn errors_never_include_manifest_or_path_data() {
        let secret = "attacker-secret-marker";
        let archive = zip_files(&[("manifest.json", secret.as_bytes())]);
        let error = validate_fixture(archive, "").unwrap_err();
        assert!(!error.to_string().contains(secret));
        assert!(!format!("{error:?}").contains(secret));
    }
}
