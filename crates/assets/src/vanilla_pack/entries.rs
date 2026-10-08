//! Central-directory validation: entry names, collisions, links and declared byte bounds, all
//! checked before any byte is written.

use std::{
    collections::{HashMap, HashSet},
    io::{Read, Seek, SeekFrom},
    path::PathBuf,
};

use zip::ZipArchive;

use super::{UnpackError, UnpackLimits, rejected};

const CENTRAL_HEADER_SIGNATURE: u32 = 0x0201_4b50;
const CENTRAL_HEADER_LEN: usize = 46;
const S_IFMT: u32 = 0xF000;
const S_IFLNK: u32 = 0xA000;

/// A regular-file entry cleared for extraction.
pub(super) struct PlannedFile {
    pub index: usize,
    pub relative: PathBuf,
    pub raw: String,
}

/// Every directory (explicit or implied) and file the archive will create, relative to staging.
pub(super) struct Plan {
    pub directories: Vec<PathBuf>,
    pub files: Vec<PlannedFile>,
}

struct Node {
    path: String,
    file: bool,
    explicit: bool,
}

pub(super) fn zip_error(error: zip::result::ZipError) -> UnpackError {
    rejected(format!("unreadable ZIP archive: {error}"))
}

/// `central` reads the same archive as `zip`; it walks the central directory itself because the
/// ZIP reader silently keeps one entry per name.
pub(super) fn plan<R: Read + Seek>(
    zip: &mut ZipArchive<R>,
    central: &mut R,
    limits: &UnpackLimits,
) -> Result<Plan, UnpackError> {
    let count = central_entries(central, zip.central_directory_start(), limits.max_entries)?;
    if count > limits.max_entries {
        return Err(rejected(format!(
            "archive entry count {count} exceeds the maximum {}",
            limits.max_entries
        )));
    }
    if count != zip.len() as u64 {
        return Err(rejected("archive listing could not be parsed consistently"));
    }
    let mut nodes: HashMap<String, Node> = HashMap::new();
    let mut files = Vec::new();
    let (mut total_expanded, mut total_compressed) = (0u64, 0u64);
    for index in 0..zip.len() {
        let entry = zip.by_index_raw(index).map_err(zip_error)?;
        let raw = entry.name().to_owned();
        let (expanded, compressed) = (entry.size(), entry.compressed_size());
        let (parts, directory) = validate_name(&raw, expanded)?;
        record_nodes(&mut nodes, &raw, &parts, directory)?;
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & S_IFMT == S_IFLNK)
        {
            return Err(rejected(format!(
                "unsafe ZIP entry '{raw}': link entries are not allowed"
            )));
        }
        if !directory {
            if expanded > limits.max_file_bytes {
                return Err(rejected(format!(
                    "ZIP entry '{raw}' declared expanded size {expanded} exceeds the maximum {} bytes",
                    limits.max_file_bytes
                )));
            }
            if compressed >= limits.min_ratio_sample
                && expanded as f64 > limits.max_entry_ratio * compressed as f64
            {
                return Err(rejected(format!(
                    "ZIP entry '{raw}' compression ratio {expanded}:{compressed} exceeds the per-entry maximum {}",
                    limits.max_entry_ratio
                )));
            }
        }
        total_expanded = total_expanded.saturating_add(expanded);
        if total_expanded > limits.max_total_bytes {
            return Err(rejected(format!(
                "archive total declared expanded size {total_expanded} exceeds the maximum {} bytes",
                limits.max_total_bytes
            )));
        }
        total_compressed = total_compressed.saturating_add(compressed);
        if !directory {
            files.push(PlannedFile {
                index,
                relative: parts.iter().collect(),
                raw,
            });
        }
    }
    // A weighted average never exceeds the worst entry, so this only catches distributed bombs.
    if total_compressed >= limits.min_ratio_sample
        && total_expanded as f64 > limits.max_aggregate_ratio * total_compressed as f64
    {
        return Err(rejected(format!(
            "archive aggregate compression ratio {total_expanded}:{total_compressed} exceeds the aggregate maximum {}",
            limits.max_aggregate_ratio
        )));
    }
    let mut directories: Vec<PathBuf> = nodes
        .into_values()
        .filter(|node| !node.file)
        .map(|node| node.path.split('/').collect())
        .collect();
    directories.sort();
    Ok(Plan { directories, files })
}

/// Counts central-directory headers (at most `limit + 1`), rejecting exact duplicate names.
fn central_entries<R: Read + Seek>(
    reader: &mut R,
    mut position: u64,
    limit: u64,
) -> Result<u64, UnpackError> {
    let unreadable = |error| rejected(format!("unreadable ZIP central directory: {error}"));
    let mut names = HashSet::new();
    let mut count = 0;
    while count <= limit {
        reader.seek(SeekFrom::Start(position)).map_err(unreadable)?;
        let mut header = [0u8; CENTRAL_HEADER_LEN];
        if reader.read_exact(&mut header).is_err()
            || header[..4] != CENTRAL_HEADER_SIGNATURE.to_le_bytes()
        {
            break;
        }
        let field = |at: usize| u64::from(u16::from_le_bytes([header[at], header[at + 1]]));
        let (name_len, extra_len, comment_len) = (field(28), field(30), field(32));
        let mut name = vec![0; name_len as usize];
        reader.read_exact(&mut name).map_err(unreadable)?;
        if names.contains(&name) {
            let name = String::from_utf8_lossy(&name);
            return Err(rejected(format!(
                "unsafe ZIP entry '{name}': duplicate ZIP entry path '{name}'"
            )));
        }
        names.insert(name);
        position += CENTRAL_HEADER_LEN as u64 + name_len + extra_len + comment_len;
        count += 1;
    }
    Ok(count)
}

/// Applies the per-entry name rules; returns the path components and whether it is a directory.
fn validate_name(raw: &str, expanded: u64) -> Result<(Vec<String>, bool), UnpackError> {
    let unsafe_entry = |reason: &str| rejected(format!("unsafe ZIP entry '{raw}': {reason}"));
    if raw.trim().is_empty() || raw.contains('\0') {
        return Err(unsafe_entry("path is empty or contains a null character"));
    }
    if raw.starts_with(['/', '\\']) {
        return Err(unsafe_entry("absolute and UNC paths are not allowed"));
    }
    let normalized = raw.replace('\\', "/");
    if normalized.contains("//") {
        return Err(unsafe_entry("empty path components are not allowed"));
    }
    let directory = normalized.ends_with('/');
    if directory && expanded != 0 {
        return Err(unsafe_entry("directory entries must be empty"));
    }
    let trimmed = normalized.trim_end_matches('/');
    if trimmed.trim().is_empty() {
        return Err(unsafe_entry("path is empty"));
    }
    let mut parts = Vec::new();
    for part in trimmed.split('/') {
        if part.is_empty() {
            return Err(unsafe_entry("empty path components are not allowed"));
        }
        if part == "." || part == ".." {
            return Err(unsafe_entry("traversal components are not allowed"));
        }
        if part.contains(':') {
            return Err(unsafe_entry(
                "drive and alternate-stream paths are not allowed",
            ));
        }
        if part.contains(['"', '<', '>', '|', '*', '?'])
            || part.chars().any(char::is_control)
            || part.ends_with([' ', '.'])
        {
            return Err(unsafe_entry(&format!(
                "invalid filename component '{part}'"
            )));
        }
        if is_reserved_device(part) {
            return Err(unsafe_entry(&format!(
                "reserved filename component '{part}'"
            )));
        }
        parts.push(part.to_owned());
    }
    Ok((parts, directory))
}

/// Windows device names, matched on the text before the first `.`.
fn is_reserved_device(part: &str) -> bool {
    let base = part.split('.').next().unwrap_or(part).to_ascii_lowercase();
    match base.as_bytes() {
        b"con" | b"prn" | b"aux" | b"nul" => true,
        [b'c', b'o', b'm', digit] | [b'l', b'p', b't', digit] => (b'1'..=b'9').contains(digit),
        _ => false,
    }
}

/// Registers the entry and its implied ancestors; names that collide case-insensitively, change
/// shape, or repeat a file or explicit directory are rejected.
fn record_nodes(
    nodes: &mut HashMap<String, Node>,
    raw: &str,
    parts: &[String],
    directory: bool,
) -> Result<(), UnpackError> {
    let mut current = String::new();
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            current.push('/');
        }
        current.push_str(part);
        let leaf = index + 1 == parts.len();
        let file = leaf && !directory;
        match nodes.get_mut(&current.to_lowercase()) {
            Some(node) => {
                if node.path != current || node.file != file {
                    return Err(rejected(format!(
                        "unsafe ZIP entry '{raw}': ZIP entry path collision at '{current}'"
                    )));
                }
                if leaf {
                    if file || node.explicit {
                        return Err(rejected(format!(
                            "unsafe ZIP entry '{raw}': duplicate ZIP entry path '{current}'"
                        )));
                    }
                    node.explicit = true;
                }
            }
            None => {
                nodes.insert(
                    current.to_lowercase(),
                    Node {
                        path: current.clone(),
                        file,
                        explicit: leaf,
                    },
                );
            }
        }
    }
    Ok(())
}
