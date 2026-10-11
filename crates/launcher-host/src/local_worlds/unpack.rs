use std::{
    collections::{BTreeSet, HashSet},
    fs::{self, File},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use serde::Serialize;
use zip::ZipArchive;

use super::{
    bds::release_world_links,
    create_private_dir,
    download::remove_stale_parts,
    lock,
    provision::{Provisioner, unix_now},
    world::{WorldError, failed},
    write_private,
};

const UNPACK_WORKERS: usize = 8;
const SIZE_LIMIT: &str = "dedicated server archive exceeds size limit";

/// Bounds an archive by its listing before anything is written.
pub(super) struct UnpackLimits {
    pub max_entries: u64,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
    /// Entries (and archives) with fewer compressed bytes skip the ratio checks.
    pub min_ratio_sample: u64,
    pub max_entry_ratio: f64,
    pub max_archive_ratio: f64,
}

/// Headroom over the Linux 1.26.32.2 build: 9,761 entries, 317 MB expanded, largest file 233 MB,
/// worst entry ratio about 19.7, aggregate 3.6.
pub(super) const BDS_LIMITS: UnpackLimits = UnpackLimits {
    max_entries: 65_536,
    max_file_bytes: 1 << 30,
    max_total_bytes: 4 << 30,
    min_ratio_sample: 4096,
    max_entry_ratio: 500.0,
    max_archive_ratio: 100.0,
};

/// Where an installed build came from.
#[derive(Serialize)]
struct Manifest<'a> {
    version: &'a str,
    url: &'a str,
    zip_sha256: &'a str,
    zip_bytes: u64,
    platform: String,
    downloaded_unix: i64,
    client_version_prefix: String,
}

struct PlannedFile {
    index: usize,
    target: PathBuf,
    size: u64,
    /// The owner-execute bit the archive records.
    executable: u32,
}

impl Provisioner {
    /// Unpacks `archive` into `root/<version>` beside its provenance manifest; returns the binary.
    pub(super) fn unpack(
        &self,
        archive: &Path,
        version: &str,
        link: &str,
        digest: &str,
        size: u64,
    ) -> Result<PathBuf, WorldError> {
        let final_dir = self.root.join(version);
        let partial = self.root.join(format!("{version}.partial"));
        remove_dir(&partial)?;
        create_private_dir(&partial).map_err(|error| failed(error.to_string()))?;
        let installed = self.fill(&partial, archive, version, link, digest, size);
        if let Err(error) = installed {
            let _ = fs::remove_dir_all(&partial);
            return Err(error);
        }
        remove_dir(&final_dir)?;
        fs::rename(partial, &final_dir).map_err(|error| failed(error.to_string()))?;
        Ok(final_dir.join(self.binary_name()))
    }

    fn fill(
        &self,
        partial: &Path,
        archive: &Path,
        version: &str,
        link: &str,
        digest: &str,
        size: u64,
    ) -> Result<(), WorldError> {
        extract_archive(partial, archive, &BDS_LIMITS)?;
        let binary = partial.join(self.binary_name());
        if fs::metadata(&binary).is_err() {
            return Err(failed("dedicated server archive has no server binary"));
        }
        set_executable(&binary);
        let manifest = serde_json::to_vec_pretty(&Manifest {
            version,
            url: link,
            zip_sha256: digest,
            zip_bytes: size,
            platform: format!("{}/{}", self.platform.os, self.platform.arch),
            downloaded_unix: unix_now(),
            client_version_prefix: self.prefix(),
        })
        .map_err(|error| failed(error.to_string()))?;
        write_private(&partial.join("manifest.json"), &manifest)
            .map_err(|error| failed(error.to_string()))
    }

    /// Removes other versions' install folders and every leftover download.
    pub(super) fn prune_old_installs(&self, keep: &str) {
        remove_stale_parts(&self.downloads_dir(), "");
        let Ok(entries) = fs::read_dir(&self.root) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name == keep
                || !install_dir_name(&name)
                || !entry.file_type().is_ok_and(|kind| kind.is_dir())
            {
                continue;
            }
            let dir = entry.path();
            if !release_world_links(&dir) {
                tracing::warn!(
                    "kept the old dedicated server install {name}: its worlds folder is not empty"
                );
                continue;
            }
            if let Err(error) = fs::remove_dir_all(dir) {
                tracing::warn!("remove old dedicated server install {name}: {error}");
            }
        }
    }
}

/// Install folders are versions, `1.2.3.4`, or interrupted unpacks, `1.2.3.4.partial`.
fn install_dir_name(name: &str) -> bool {
    let version = name.strip_suffix(".partial").unwrap_or(name);
    version.contains('.')
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

fn remove_dir(path: &Path) -> Result<(), WorldError> {
    match fs::remove_dir_all(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(failed(error.to_string())),
        _ => Ok(()),
    }
}

fn set_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    }
    #[cfg(not(unix))]
    let _ = path;
}

fn open_archive(archive: &Path) -> Result<ZipArchive<File>, WorldError> {
    File::open(archive)
        .map_err(|error| failed(error.to_string()))
        .and_then(|file| {
            ZipArchive::new(file)
                .map_err(|error| failed(format!("dedicated server archive is corrupt: {error}")))
        })
}

/// Checks the whole listing against `limits`, creates every folder once, then writes the files
/// in parallel, largest first.
pub(super) fn extract_archive(
    root: &Path,
    archive: &Path,
    limits: &UnpackLimits,
) -> Result<(), WorldError> {
    let (files, dirs) = plan_extraction(root, &mut open_archive(archive)?, limits)?;
    for dir in &dirs {
        create_private_dir(dir).map_err(|error| failed(error.to_string()))?;
    }
    let next = AtomicUsize::new(0);
    let failure: Mutex<Option<WorldError>> = Mutex::new(None);
    let record = |error: WorldError| {
        let mut first = lock(&failure);
        if first.is_none() {
            *first = Some(error);
        }
    };
    std::thread::scope(|scope| {
        for _ in 0..UNPACK_WORKERS.min(files.len()) {
            scope.spawn(|| {
                let mut zip = match open_archive(archive) {
                    Ok(zip) => zip,
                    Err(error) => return record(error),
                };
                let mut buffer = vec![0u8; 256 << 10];
                while lock(&failure).is_none() {
                    let Some(job) = files.get(next.fetch_add(1, Ordering::Relaxed)) else {
                        return;
                    };
                    if let Err(error) = extract_file(&mut zip, job, &mut buffer) {
                        return record(error);
                    }
                }
            });
        }
    });
    match failure
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
    {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

/// Validates every entry and returns the files (largest first) and the folders they need.
/// Symlinks and other special entries are skipped.
fn plan_extraction(
    root: &Path,
    zip: &mut ZipArchive<File>,
    limits: &UnpackLimits,
) -> Result<(Vec<PlannedFile>, BTreeSet<PathBuf>), WorldError> {
    if zip.len() as u64 > limits.max_entries {
        return Err(failed(format!(
            "dedicated server archive has more than {} entries",
            limits.max_entries
        )));
    }
    let mut files = Vec::new();
    let mut dirs = BTreeSet::new();
    let mut seen = HashSet::new();
    let (mut total, mut compressed) = (0u64, 0u64);
    for index in 0..zip.len() {
        let entry = zip
            .by_index_raw(index)
            .map_err(|error| failed(format!("dedicated server archive is corrupt: {error}")))?;
        let name = entry.name().to_owned();
        let target = entry_target(root, &name)?;
        if entry.is_dir() {
            dirs.insert(target);
            continue;
        }
        let mode = entry.unix_mode().unwrap_or(0);
        if !matches!(mode & 0o170_000, 0 | 0o100_000) {
            continue;
        }
        // Case-folded, so parallel writers never share a file on case-insensitive disks.
        if !seen.insert(target.to_string_lossy().to_lowercase()) {
            return Err(failed(format!("archive entry {name:?} is duplicated")));
        }
        let (size, packed) = (entry.size(), entry.compressed_size());
        if size > limits.max_file_bytes {
            return Err(failed(SIZE_LIMIT));
        }
        if packed >= limits.min_ratio_sample && size as f64 > limits.max_entry_ratio * packed as f64
        {
            return Err(failed(format!(
                "archive entry {name:?} is compressed implausibly well"
            )));
        }
        total += size;
        compressed += packed;
        if total > limits.max_total_bytes {
            return Err(failed(SIZE_LIMIT));
        }
        if let Some(parent) = target.parent() {
            dirs.insert(parent.to_path_buf());
        }
        files.push(PlannedFile {
            index,
            target,
            size,
            executable: mode & 0o100,
        });
    }
    if compressed >= limits.min_ratio_sample
        && total as f64 > limits.max_archive_ratio * compressed as f64
    {
        return Err(failed(
            "dedicated server archive is compressed implausibly well",
        ));
    }
    // Largest first, so the server binary never trails the small files.
    files.sort_by(|a, b| b.size.cmp(&a.size));
    Ok((files, dirs))
}

/// The path of an archive entry under `root`, rejecting names that escape it.
fn entry_target(root: &Path, name: &str) -> Result<PathBuf, WorldError> {
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    let absolute = name.starts_with('/')
        || (cfg!(windows) && (name.starts_with('\\') || name.as_bytes().get(1) == Some(&b':')));
    if absolute {
        return Err(failed(format!("archive entry {name:?} is absolute")));
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split(separators) {
        match part {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() {
                    return Err(failed(format!(
                        "archive entry {name:?} escapes the install directory"
                    )));
                }
            }
            part if cfg!(windows) && part.contains(':') => {
                return Err(failed(format!("archive entry {name:?} names a stream")));
            }
            part => parts.push(part),
        }
    }
    Ok(parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

/// Writes one planned entry, refusing more bytes than its listing declared.
fn extract_file(
    zip: &mut ZipArchive<File>,
    job: &PlannedFile,
    buffer: &mut [u8],
) -> Result<(), WorldError> {
    let mut entry = zip
        .by_index(job.index)
        .map_err(|error| failed(error.to_string()))?;
    let mut options = fs::OpenOptions::new();
    options.create(true).write(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600 | job.executable);
    #[cfg(not(unix))]
    let _ = job.executable;
    let mut out = options
        .open(&job.target)
        .map_err(|error| failed(error.to_string()))?;
    let mut written = 0u64;
    loop {
        let read = entry
            .read(buffer)
            .map_err(|error| failed(error.to_string()))?;
        if read == 0 {
            return Ok(());
        }
        written += read as u64;
        if written > job.size {
            return Err(failed(SIZE_LIMIT));
        }
        out.write_all(&buffer[..read])
            .map_err(|error| failed(error.to_string()))?;
    }
}

#[cfg(test)]
#[path = "unpack_tests.rs"]
mod tests;
