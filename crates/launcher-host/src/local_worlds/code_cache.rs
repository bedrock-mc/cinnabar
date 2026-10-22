//! The translator's reusable code cache for the APK-bundled server.
//!
//! Each exact server build and packaged runtime gets one generation folder. The first launch of a
//! generation lets the translator write it when the server exits; later launches only read it, so
//! stops never rewrite it. The translator validates every file it loads and deletes any it
//! rejects. This module alone creates, seals and removes generations, and never touches world
//! data.

use std::{
    collections::BTreeSet,
    fs, io,
    io::Write as _,
    path::{Path, PathBuf},
    process::Command,
    sync::Mutex,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{create_private_dir, lock, process::Ending, write_private};

/// The largest generation kept, in MiB, passed as the translator's folder limit. The packaged
/// translator skips writing a cache file that would take its folder past this limit or leave less
/// than this much free space, so its own pruning never drops part of a generation this module
/// then seals.
pub(super) const LIMIT_MIB: u64 = 512;
/// The free space, in MiB, a producer launch leaves for world saves beyond a full generation; the
/// translator applies its limit as this reserve.
pub(super) const RESERVE_MIB: u64 = LIMIT_MIB;
/// How many producer launches a generation gets before its cache stays off.
pub(super) const PRODUCER_ATTEMPTS: u32 = 2;
/// The translator's setting for uncompressed blocks. The packaged native runtime ships no
/// compressor, so it would write them uncompressed anyway.
pub(super) const UNCOMPRESSED: &str = "0";

const MODE_VAR: &str = "BOX64_DYNACACHE";
const FOLDER_VAR: &str = "BOX64_DYNACACHE_FOLDER";
const COMPRESS_VAR: &str = "BOX64_DYNACACHE_COMPRESS";
/// The translator's folder limit in MiB, also its write-time free-space reserve.
const LIMIT_VAR: &str = "BOX64_DYNACACHE_LIMIT";
/// Left to the translator's default; an inherited value is cleared.
const MIN_VAR: &str = "BOX64_DYNACACHE_MIN";

/// Suffix of the translator's published cache files.
const CACHE_SUFFIX: &str = ".box64";
/// Marks the translator's interrupted temporary writes.
const PARTIAL_MARK: &str = ".box64.tmp.";
const SEALED: &str = "sealed.json";
const ATTEMPTS: &str = "attempts";
const DISABLED: &str = "disabled";

/// Cache roots with a launch in flight. A root is leased by at most one [`Use`] at a time, so a
/// second launch never reads or writes a generation another server still holds.
static LEASED: Mutex<BTreeSet<PathBuf>> = Mutex::new(BTreeSet::new());

/// What a launch does with its generation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Mode {
    /// Neither reads nor writes translated code.
    Off,
    /// Reads nothing, then writes the generation when the server exits.
    Produce,
    /// Reads the sealed generation and writes nothing.
    Load,
}

impl Mode {
    /// The translator's value for its cache-mode setting.
    fn setting(self) -> &'static str {
        match self {
            Self::Off => "0",
            Self::Produce => "1",
            Self::Load => "2",
        }
    }
}

/// Why a launch runs with its cache off.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Off {
    /// The server install or packaged runtime has no known identity.
    Unidentified,
    /// Another server holds the cache root.
    InUse,
    /// A failed launch that loaded it, or an oversized generation, turned this generation off.
    Disabled,
    /// Every producer attempt of this generation ended without a seal.
    Exhausted(u32),
    /// The file system has `free` bytes, below the `needed` generation plus its reserve.
    LowSpace { free: u64, needed: u64 },
    /// Reading or writing the cache folder failed.
    Io(String),
}

impl std::fmt::Display for Off {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unidentified => write!(f, "the server or translator identity is unknown"),
            Self::InUse => write!(f, "another server is using it"),
            Self::Disabled => write!(f, "it was turned off for this server build"),
            Self::Exhausted(attempts) => {
                write!(f, "{attempts} attempts ended without a usable cache")
            }
            Self::LowSpace { free, needed } => {
                write!(f, "{free} bytes free, {needed} needed")
            }
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

/// The files of a sealed generation, as sorted names and byte lengths.
#[derive(Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
struct Sealed {
    files: Vec<(String, u64)>,
}

/// One launch's use of a generation. [`Use::finish`] records how that launch ended; dropping it
/// releases the cache root for the next launch.
#[derive(Debug)]
pub(crate) struct Use {
    mode: Mode,
    /// Why `mode` is [`Mode::Off`]; `None` otherwise.
    off: Option<Off>,
    dir: PathBuf,
    /// Begins the name of the server executable's own cache file.
    server_prefix: String,
    /// The root this use leased, released when it drops.
    leased: Option<PathBuf>,
}

/// The code cache's root folder and the generation the current runtime selects.
pub(crate) struct CodeCache {
    root: PathBuf,
    key: String,
    /// Begins the name of the server executable's own cache file.
    server_prefix: String,
}

impl CodeCache {
    /// A cache under `root`, which it owns exclusively, for the server executable named
    /// `executable`. `parts` identify the exact translated code: server bytes, translator bytes,
    /// runtime pins and code-generation settings. A generation is usable only once it holds the
    /// executable's own cache file, which the translator names after the executable and a `-`;
    /// files for its libraries alone do not cover the server's startup.
    pub(crate) fn new(root: PathBuf, executable: &str, parts: &[&str]) -> Self {
        let mut hash = Sha256::new();
        for part in parts {
            hash.update(part.as_bytes());
            hash.update([0]);
        }
        Self {
            root,
            key: format!("{:x}", hash.finalize()),
            server_prefix: format!("{executable}-"),
        }
    }

    /// Chooses the next launch's mode after removing other generations and interrupted writes.
    /// `available` reports free bytes on the folder's file system. Callers launch servers that use
    /// this cache one at a time; a launch while another still holds the root runs with it off.
    pub(crate) fn prepare(&self, available: impl FnOnce(&Path) -> io::Result<u64>) -> Use {
        let dir = self.root.join(&self.key);
        if !lock(&LEASED).insert(self.root.clone()) {
            tracing::warn!("translated code cache in use by another server; off for this launch");
            return Self::off(Off::InUse);
        }
        let leased = Some(self.root.clone());
        let chosen = choose(self, &dir, available).unwrap_or_else(|error| {
            tracing::warn!("translated code cache unavailable: {error}");
            Err(Off::Io(error.to_string()))
        });
        let (mode, off) = match chosen {
            Ok(mode) => (mode, None),
            Err(off) => (Mode::Off, Some(off)),
        };
        tracing::info!("translated code cache: {mode:?}");
        Use {
            mode,
            off,
            dir,
            server_prefix: self.server_prefix.clone(),
            leased,
        }
    }

    /// Whether the generation this cache selects is sealed with the server executable's own
    /// file and every sealed file still present at its sealed length, so [`Self::prepare`] would
    /// load it. Only reads: it takes no lease, uses no producer attempt and removes nothing. The
    /// translator still validates each file against the mapped server binary when it loads it.
    pub(crate) fn ready(&self) -> bool {
        let dir = self.root.join(&self.key);
        !dir.join(DISABLED).exists()
            && read_sealed(&dir).is_some_and(|sealed| {
                holds_server(&sealed.files, &self.server_prefix)
                    && published_files(&dir).is_ok_and(|files| files == sealed.files)
            })
    }

    /// Lets a new user-initiated setup attempt rebuild a generation disabled by an earlier
    /// failure. Normal world starts never call this, and no launch is retried automatically.
    /// Refuses to remove code while any process in this client still leases the cache root.
    pub(crate) fn retry_disabled(&self) -> io::Result<()> {
        let leased = lock(&LEASED);
        if leased.contains(&self.root) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "startup code is in use",
            ));
        }
        let dir = self.root.join(&self.key);
        if dir.join(DISABLED).is_file() || read_attempts(&dir) >= PRODUCER_ATTEMPTS {
            match fs::remove_dir_all(&dir) {
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                result => result?,
            }
        }
        Ok(())
    }

    /// The folder of the generation this cache selects.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> PathBuf {
        self.root.join(&self.key)
    }

    /// A launch that neither reads nor writes, for `reason`.
    pub(crate) fn off(reason: Off) -> Use {
        Use {
            mode: Mode::Off,
            off: Some(reason),
            dir: PathBuf::new(),
            server_prefix: String::new(),
            leased: None,
        }
    }
}

impl Use {
    /// Whether this launch produces, loads or bypasses translated startup code.
    pub(crate) fn mode(&self) -> Mode {
        self.mode
    }

    /// Why this launch runs with its cache off; `None` when it reads or writes it.
    pub(crate) fn off_reason(&self) -> Option<&Off> {
        self.off.as_ref()
    }

    /// Sets this launch's translator cache settings explicitly, clearing inherited ones, so no
    /// inherited, home or cache folder is used.
    pub(crate) fn apply(&self, command: &mut Command) {
        for var in [FOLDER_VAR, COMPRESS_VAR, LIMIT_VAR, MIN_VAR] {
            command.env_remove(var);
        }
        command.env(MODE_VAR, self.mode.setting());
        if self.mode != Mode::Off {
            command
                .env(FOLDER_VAR, &self.dir)
                .env(COMPRESS_VAR, UNCOMPRESSED)
                .env(LIMIT_VAR, LIMIT_MIB.to_string());
        }
    }

    /// Records how the launch ended. The launch itself is never retried or changed:
    ///
    /// - A clean producer seals its generation, unless it wrote nothing or too much.
    /// - A producer that never ran or was cancelled before it was ready returns its attempt.
    /// - A launch that read the cache and then failed turns this generation off.
    pub(crate) fn finish(self, ending: Ending) {
        let result = match (self.mode, ending) {
            (Mode::Off, _)
            | (Mode::Load, Ending::Clean | Ending::Cancelled | Ending::Unstarted) => Ok(()),
            (Mode::Load, Ending::Failed) => {
                tracing::warn!("translated code cache off after a failed server that used it");
                disable(&self.dir)
            }
            (Mode::Produce, Ending::Clean) => seal(&self.dir, &self.server_prefix),
            (Mode::Produce, Ending::Cancelled | Ending::Unstarted) => {
                discard(&self.dir).and_then(|()| return_attempt(&self.dir))
            }
            (Mode::Produce, Ending::Failed) => discard(&self.dir),
        };
        if let Err(error) = result {
            tracing::warn!("record translated code cache: {error}");
        }
    }
}

impl Drop for Use {
    fn drop(&mut self) {
        if let Some(root) = self.leased.take() {
            lock(&LEASED).remove(&root);
        }
    }
}

/// Mode selection, or why the cache is off. Any I/O error leaves the cache off for this launch.
fn choose(
    cache: &CodeCache,
    dir: &Path,
    available: impl FnOnce(&Path) -> io::Result<u64>,
) -> io::Result<Result<Mode, Off>> {
    remove_other_generations(&cache.root, &cache.key)?;
    create_private_dir(dir)?;
    let files = cache_files(dir)?;
    if dir.join(DISABLED).exists() {
        remove_all(dir, &files)?;
        return Ok(Err(Off::Disabled));
    }
    if let Some(sealed) = read_sealed(dir) {
        if sealed.files == files && holds_server(&files, &cache.server_prefix) {
            return Ok(Ok(Mode::Load));
        }
        // The translator deleted a file it rejected, or storage lost or changed one.
        tracing::info!("translated code cache changed since it was sealed; discarding it");
    }
    remove_seal(dir)?;
    remove_all(dir, &files)?;
    let attempts = read_attempts(dir);
    if attempts >= PRODUCER_ATTEMPTS {
        write_private(&dir.join(DISABLED), b"")?;
        tracing::warn!("translated code cache off after {attempts} unsealed generations");
        return Ok(Err(Off::Exhausted(attempts)));
    }
    let needed = (LIMIT_MIB + RESERVE_MIB) << 20;
    let free = available(dir)?;
    if free < needed {
        tracing::info!("translated code cache off: {free} bytes free, {needed} needed");
        return Ok(Err(Off::LowSpace { free, needed }));
    }
    // Counted before launch, so a server that dies with the app still uses its attempt.
    write_private(&dir.join(ATTEMPTS), (attempts + 1).to_string().as_bytes())?;
    Ok(Ok(Mode::Produce))
}

/// Removes every entry under `root` except the current generation folder.
fn remove_other_generations(root: &Path, key: &str) -> io::Result<()> {
    let entries = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        if entry.file_name() != key {
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                fs::remove_dir_all(path)?;
            } else {
                fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

/// Deletes interrupted translator writes, then returns the published cache files as sorted
/// names and lengths.
fn cache_files(dir: &Path) -> io::Result<Vec<(String, u64)>> {
    list_files(dir, true)
}

/// The published cache files as sorted names and lengths, leaving interrupted writes in place.
fn published_files(dir: &Path) -> io::Result<Vec<(String, u64)>> {
    list_files(dir, false)
}

/// The published cache files of `dir` as sorted names and lengths; `remove_partial` also deletes
/// interrupted translator writes.
fn list_files(dir: &Path, remove_partial: bool) -> io::Result<Vec<(String, u64)>> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.contains(PARTIAL_MARK) {
            if remove_partial {
                fs::remove_file(entry.path())?;
            }
        } else if name.ends_with(CACHE_SUFFIX) && entry.file_type()?.is_file() {
            files.push((name, entry.metadata()?.len()));
        }
    }
    files.sort();
    Ok(files)
}

/// Removes `files` from `dir`.
fn remove_all(dir: &Path, files: &[(String, u64)]) -> io::Result<()> {
    for (name, _) in files {
        fs::remove_file(dir.join(name))?;
    }
    Ok(())
}

/// Removes every published and interrupted cache file from `dir`.
fn discard(dir: &Path) -> io::Result<()> {
    remove_all(dir, &cache_files(dir)?)
}

/// Removes the seal, if there is one.
fn remove_seal(dir: &Path) -> io::Result<()> {
    match fs::remove_file(dir.join(SEALED)) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

/// Turns the generation off and frees its space; a new key starts a new generation.
fn disable(dir: &Path) -> io::Result<()> {
    write_private(&dir.join(DISABLED), b"")?;
    remove_seal(dir)?;
    discard(dir)
}

/// Whether `files` include the server executable's own cache file, named from `server_prefix`.
fn holds_server(files: &[(String, u64)], server_prefix: &str) -> bool {
    files
        .iter()
        .any(|(name, _)| name.starts_with(server_prefix))
}

/// Seals what a clean producer published. A generation over the limit turns the cache off; one
/// without the server executable's file is discarded and its attempt stays used.
fn seal(dir: &Path, server_prefix: &str) -> io::Result<()> {
    let files = cache_files(dir)?;
    let total: u64 = files.iter().map(|(_, bytes)| bytes).sum();
    if total > LIMIT_MIB << 20 {
        tracing::warn!("translated code cache off: generation holds {total} bytes");
        return disable(dir);
    }
    if !holds_server(&files, server_prefix) {
        tracing::warn!("translated code cache: the server exited without writing its own code");
        return remove_all(dir, &files);
    }
    let mut temporary = tempfile::NamedTempFile::new_in(dir)?;
    temporary.write_all(&serde_json::to_vec(&Sealed { files })?)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist(dir.join(SEALED))
        .map_err(|error| error.error)?;
    sync_dir(dir)
}

/// Makes a rename in `dir` durable where directories can be synced.
fn sync_dir(dir: &Path) -> io::Result<()> {
    if cfg!(unix) {
        fs::File::open(dir)?.sync_all()?;
    }
    Ok(())
}

/// The sealed file list; `None` when missing or unreadable.
fn read_sealed(dir: &Path) -> Option<Sealed> {
    serde_json::from_slice(&fs::read(dir.join(SEALED)).ok()?).ok()
}

/// Producer launches already counted for this generation; zero when unrecorded.
fn read_attempts(dir: &Path) -> u32 {
    fs::read_to_string(dir.join(ATTEMPTS))
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(0)
}

/// Gives back the attempt of a producer that never got to run its server to an exit.
fn return_attempt(dir: &Path) -> io::Result<()> {
    let attempts = read_attempts(dir).saturating_sub(1);
    write_private(&dir.join(ATTEMPTS), attempts.to_string().as_bytes())
}

/// Free bytes for unprivileged writers on the file system holding `dir`.
#[cfg(unix)]
pub(crate) fn available_bytes(dir: &Path) -> io::Result<u64> {
    let stats = rustix::fs::statvfs(dir)?;
    Ok(stats.f_bavail.saturating_mul(stats.f_frsize))
}

/// Free space is not measured here, so the cache stays off; only Android translates the server.
#[cfg(not(unix))]
pub(crate) fn available_bytes(_dir: &Path) -> io::Result<u64> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}

#[cfg(test)]
#[path = "code_cache_tests.rs"]
mod tests;
