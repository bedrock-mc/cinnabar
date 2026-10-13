//! Durable renderer startup attempts and bounded recovery after a failed launch.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use launcher::graphics_backend::{Adapter, Backend, ProbeCache};

pub mod probe;

/// The last operation that must finish before startup is considered successful.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    EnumeratingAdapters,
    CreatingDevice,
    FirstPresentation,
    Ready,
}

/// A pending attempt remains on disk until a presented frame finishes its GPU work.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub backend: Backend,
    pub adapter: Option<Adapter>,
    pub stage: Stage,
    pub explicit: bool,
}

/// Successful choices and failed backends survive separate process launches.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Recovery {
    pub working: Option<Attempt>,
    failed: Vec<Backend>,
}

impl Recovery {
    /// Excludes an automatic attempt's entire backend, including its failed adapter.
    pub fn recover(&mut self, pending: &Attempt) {
        if pending.explicit {
            return;
        }
        if !self.failed.contains(&pending.backend) {
            self.failed.push(pending.backend);
        }
        if self
            .working
            .as_ref()
            .is_some_and(|working| working.backend == pending.backend)
        {
            self.working = None;
        }
    }

    /// The sentinel exclusions are applied after automatic probe and vendor ordering.
    pub fn failed_backends(&self) -> &[Backend] {
        &self.failed
    }

    /// Allows a new driver or adapter set to begin a fresh recovery cycle.
    pub fn reset(&mut self) {
        self.failed.clear();
        self.working = None;
    }

    /// Remembers any completed choice and starts a new recovery cycle.
    fn succeeded(&mut self, mut attempt: Attempt) {
        attempt.stage = Stage::Ready;
        self.working = Some(attempt);
        self.failed.clear();
    }
}

/// Owns the two local files used to diagnose an interrupted renderer startup.
pub struct StartupGuard {
    root: PathBuf,
    pub recovery: Recovery,
    pub interrupted: Option<Attempt>,
    pending: Option<Attempt>,
}

impl StartupGuard {
    /// Consumes the previous launch's sentinel before any GPU backend is loaded.
    pub fn open(root: &Path) -> Result<Self> {
        let interrupted = Self::pending(root)?;
        let mut recovery = Self::saved(root)?;
        if let Some(attempt) = &interrupted {
            recovery.recover(attempt);
        }
        save(&root.join(CHOICE_FILE), &recovery)?;
        Ok(Self {
            root: root.to_owned(),
            recovery,
            interrupted,
            pending: None,
        })
    }

    /// Reads the pending operation for a local crash report without changing recovery state.
    pub fn pending(root: &Path) -> Result<Option<Attempt>> {
        load(&root.join(SENTINEL_FILE))
    }

    /// Reads the last successful choice and the bounded failure history.
    pub fn saved(root: &Path) -> Result<Recovery> {
        Ok(load(&root.join(CHOICE_FILE))?.unwrap_or_default())
    }

    /// Persists a fresh recovery cycle when the adapter set or driver versions changed.
    pub fn reset_recovery(&mut self) -> Result<()> {
        self.recovery.reset();
        self.interrupted = None;
        if self.root.join(SENTINEL_FILE).exists() {
            fs::remove_file(self.root.join(SENTINEL_FILE))?;
        }
        save(&self.root.join(CHOICE_FILE), &self.recovery)
    }

    /// Flushes the next operation to disk before the caller enters a native driver.
    pub fn begin(&mut self, attempt: Attempt) -> Result<()> {
        save(&self.root.join(SENTINEL_FILE), &attempt)?;
        #[cfg(unix)]
        crate::lifecycle::crash::unix::update(Some(&attempt));
        self.pending = Some(attempt);
        Ok(())
    }

    /// Records a returned initialization error before trying another supported backend.
    pub fn failed(&mut self) -> Result<()> {
        if let Some(attempt) = &self.pending {
            self.recovery.recover(attempt);
        }
        save(&self.root.join(CHOICE_FILE), &self.recovery)
    }

    /// Saves the successful choice before removing the first-frame sentinel.
    pub fn presented(&mut self) -> Result<()> {
        let Some(attempt) = self.pending.clone() else {
            return Ok(());
        };
        self.recovery.succeeded(attempt);
        save(&self.root.join(CHOICE_FILE), &self.recovery)?;
        fs::remove_file(self.root.join(SENTINEL_FILE))
            .context("clear renderer startup sentinel")?;
        #[cfg(unix)]
        crate::lifecycle::crash::unix::update(self.recovery.working.as_ref());
        self.pending = None;
        Ok(())
    }
}

const SENTINEL_FILE: &str = "renderer-startup.json";
const CHOICE_FILE: &str = "renderer-choice.json";
const MAX_FILE_BYTES: u64 = 64 * 1024;
const PROBE_CACHE_FILE: &str = "graphics-probe-cache.json";

/// Reads bounded driver verdicts without loading a graphics library.
pub fn load_probe_cache(root: &Path) -> Result<ProbeCache> {
    Ok(load(&root.join(PROBE_CACHE_FILE))?.unwrap_or_default())
}

/// Flushes driver verdicts before the parent enters a cached backend.
pub fn save_probe_cache(root: &Path, cache: &ProbeCache) -> Result<()> {
    save(&root.join(PROBE_CACHE_FILE), cache)
}

/// Reads a bounded local state file, treating an absent file as a fresh install.
fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let file = match fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("open {}", path.display())),
    };
    let mut bytes = Vec::new();
    file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("renderer state exceeds size limit: {}", path.display());
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .with_context(|| format!("decode {}", path.display()))
}

/// Atomically replaces a state file after its bytes have reached local storage.
fn save(path: &Path, value: &impl Serialize) -> Result<()> {
    let root = path.parent().context("renderer state parent directory")?;
    fs::create_dir_all(root)?;
    let mut file = tempfile::NamedTempFile::new_in(root)?;
    file.write_all(&serde_json::to_vec(value)?)?;
    file.as_file().sync_all()?;
    file.persist(path)
        .with_context(|| format!("save {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests;
