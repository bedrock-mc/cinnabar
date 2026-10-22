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
    dir: PathBuf,
    /// The root this use leased, released when it drops.
    leased: Option<PathBuf>,
}

/// The code cache's root folder and the generation the current runtime selects.
pub(crate) struct CodeCache {
    root: PathBuf,
    key: String,
}

impl CodeCache {
    /// A cache under `root`, which it owns exclusively. `parts` identify the exact translated
    /// code: server bytes, translator bytes, runtime pins and code-generation settings.
    pub(crate) fn new(root: PathBuf, parts: &[&str]) -> Self {
        let mut hash = Sha256::new();
        for part in parts {
            hash.update(part.as_bytes());
            hash.update([0]);
        }
        Self {
            root,
            key: format!("{:x}", hash.finalize()),
        }
    }

    /// Chooses the next launch's mode after removing other generations and interrupted writes.
    /// `available` reports free bytes on the folder's file system. Callers launch servers that use
    /// this cache one at a time; a launch while another still holds the root runs with it off.
    pub(crate) fn prepare(&self, available: impl FnOnce(&Path) -> io::Result<u64>) -> Use {
        let dir = self.root.join(&self.key);
        if !lock(&LEASED).insert(self.root.clone()) {
            tracing::warn!("translated code cache in use by another server; off for this launch");
            return Self::off();
        }
        let leased = Some(self.root.clone());
        let mode = choose(&self.root, &self.key, &dir, available).unwrap_or_else(|error| {
            tracing::warn!("translated code cache unavailable: {error}");
            Mode::Off
        });
        tracing::info!("translated code cache: {mode:?}");
        Use { mode, dir, leased }
    }

    /// The folder of the generation this cache selects.
    #[cfg(test)]
    pub(crate) fn generation(&self) -> PathBuf {
        self.root.join(&self.key)
    }

    /// A launch that neither reads nor writes, for runtimes whose identity cannot be established.
    pub(crate) fn off() -> Use {
        Use {
            mode: Mode::Off,
            dir: PathBuf::new(),
            leased: None,
        }
    }
}

impl Use {
    #[cfg(test)]
    pub(crate) fn mode(&self) -> Mode {
        self.mode
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
            (Mode::Produce, Ending::Clean) => seal(&self.dir),
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

/// Mode selection. Any I/O error leaves the cache off for this launch.
fn choose(
    root: &Path,
    key: &str,
    dir: &Path,
    available: impl FnOnce(&Path) -> io::Result<u64>,
) -> io::Result<Mode> {
    remove_other_generations(root, key)?;
    create_private_dir(dir)?;
    let files = cache_files(dir)?;
    if dir.join(DISABLED).exists() {
        remove_all(dir, &files)?;
        return Ok(Mode::Off);
    }
    if let Some(sealed) = read_sealed(dir) {
        if sealed.files == files {
            return Ok(Mode::Load);
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
        return Ok(Mode::Off);
    }
    let needed = (LIMIT_MIB + RESERVE_MIB) << 20;
    let free = available(dir)?;
    if free < needed {
        tracing::info!("translated code cache off: {free} bytes free, {needed} needed");
        return Ok(Mode::Off);
    }
    // Counted before launch, so a server that dies with the app still uses its attempt.
    write_private(&dir.join(ATTEMPTS), (attempts + 1).to_string().as_bytes())?;
    Ok(Mode::Produce)
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
    let mut files = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.contains(PARTIAL_MARK) {
            fs::remove_file(entry.path())?;
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

/// Seals what a clean producer published. A generation over the limit turns the cache off; one
/// with nothing published is discarded and its attempt stays used.
fn seal(dir: &Path) -> io::Result<()> {
    let files = cache_files(dir)?;
    let total: u64 = files.iter().map(|(_, bytes)| bytes).sum();
    if total > LIMIT_MIB << 20 {
        tracing::warn!("translated code cache off: generation holds {total} bytes");
        return disable(dir);
    }
    if files.is_empty() {
        tracing::warn!("translated code cache: the server exited without writing a generation");
        return Ok(());
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
