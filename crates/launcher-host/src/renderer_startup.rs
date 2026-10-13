//! Durable renderer startup attempts and bounded recovery after a failed launch.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use launcher::graphics_backend::{Adapter, Backend};

pub(crate) mod context;
#[cfg(target_os = "android")]
mod direct_driver;
mod owner;
pub(crate) mod termination;

use owner::Owner;

#[derive(Serialize, Deserialize)]
struct Sentinel {
    #[serde(default)]
    owner: Option<Owner>,
    #[serde(flatten)]
    attempt: Attempt,
}

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
    #[serde(default)]
    direct_driver: Option<String>,
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

    /// The sentinel exclusions are applied after automatic vendor ordering.
    pub fn failed_backends(&self) -> &[Backend] {
        &self.failed
    }

    /// Allows a new driver or adapter set to begin a fresh recovery cycle.
    pub fn reset(&mut self) {
        self.failed.clear();
        self.working = None;
        self.direct_driver = None;
    }

    /// Remembers a completed choice while retaining exclusions for other crashed APIs.
    fn succeeded(&mut self, mut attempt: Attempt) {
        attempt.stage = Stage::Ready;
        self.failed.retain(|backend| *backend != attempt.backend);
        self.working = Some(attempt);
    }
}

/// Owns the two local files used to diagnose an interrupted renderer startup.
pub struct StartupGuard {
    root: PathBuf,
    lock: Option<Arc<fs::File>>,
    owner: Owner,
    pub recovery: Recovery,
    pub interrupted: Option<Attempt>,
    pending: Option<Attempt>,
}

impl StartupGuard {
    /// Leases recovery state before consuming a sentinel left by an exited startup.
    pub fn open(root: &Path) -> Result<Self> {
        let mut active = termination::active()?;
        fs::create_dir_all(root)?;
        let lock = fs::File::options()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(LOCK_FILE))?;
        lock.try_lock().map_err(|error| match error {
            fs::TryLockError::WouldBlock => std::io::Error::new(
                std::io::ErrorKind::WouldBlock,
                "another client is starting its renderer; retry after its first frame",
            ),
            fs::TryLockError::Error(error) => error,
        })?;
        let owner = Owner::current()?;
        let interrupted =
            load::<Sentinel>(&root.join(SENTINEL_FILE))?.map(|sentinel| sentinel.attempt);
        let mut recovery = Self::saved(root)?;
        if let Some(attempt) = &interrupted {
            recovery.recover(attempt);
        }
        save(&root.join(CHOICE_FILE), &recovery)?;
        let lock = Arc::new(lock);
        active.register(root, &lock);
        Ok(Self {
            root: root.to_owned(),
            lock: Some(lock),
            owner,
            recovery,
            interrupted,
            pending: None,
        })
    }

    /// Reads the pending operation for a local crash report without changing recovery state.
    pub fn pending(root: &Path) -> Result<Option<Attempt>> {
        let owner = Owner::current()?;
        Ok(load::<Sentinel>(&root.join(SENTINEL_FILE))?
            .filter(|sentinel| sentinel.owner.as_ref() == Some(&owner))
            .map(|sentinel| sentinel.attempt))
    }

    /// Reads the last successful choice and the bounded failure history.
    pub fn saved(root: &Path) -> Result<Recovery> {
        Ok(load(&root.join(CHOICE_FILE))?.unwrap_or_default())
    }

    /// Persists a fresh recovery cycle when the adapter set or driver versions changed.
    pub fn reset_recovery(&mut self) -> Result<()> {
        let _active = termination::active()?;
        self.require_lease()?;
        self.recovery.reset();
        self.interrupted = None;
        if self.root.join(SENTINEL_FILE).exists() {
            fs::remove_file(self.root.join(SENTINEL_FILE))?;
        }
        save(&self.root.join(CHOICE_FILE), &self.recovery)
    }

    /// Invalidates Android recovery after an OS or vendor driver build changes, without loading it.
    pub fn refresh_direct_driver(&mut self) -> Result<()> {
        #[cfg(target_os = "android")]
        self.refresh_direct_identity(&direct_driver::identity()?)?;
        Ok(())
    }

    /// Migrates unkeyed recovery and discards exclusions from a different driver build.
    #[cfg(any(target_os = "android", test))]
    fn refresh_direct_identity(&mut self, identity: &str) -> Result<()> {
        self.require_lease()?;
        if self.recovery.direct_driver.as_deref() != Some(identity) {
            self.reset_recovery()?;
            self.recovery.direct_driver = Some(identity.to_owned());
            save(&self.root.join(CHOICE_FILE), &self.recovery)?;
        }
        Ok(())
    }

    /// Flushes the next operation to disk before the caller enters a native driver.
    pub fn begin(&mut self, attempt: Attempt) -> Result<()> {
        let _active = termination::active()?;
        self.require_lease()?;
        save(
            &self.root.join(SENTINEL_FILE),
            &Sentinel {
                owner: Some(self.owner.clone()),
                attempt: attempt.clone(),
            },
        )?;
        context::update(&self.root, Some(&attempt));
        self.pending = Some(attempt);
        Ok(())
    }

    /// Clears a returned initialization error without classifying it as a native crash.
    pub fn failed(&mut self) -> Result<()> {
        let _active = termination::active()?;
        self.require_lease()?;
        if self.pending.is_some() {
            fs::remove_file(self.root.join(SENTINEL_FILE))
                .context("clear returned renderer startup error")?;
            self.pending = None;
        }
        context::update(&self.root, None);
        Ok(())
    }

    /// Cancels a controlled exit without marking an unfinished backend as working or failed.
    pub fn cancel(&mut self) -> Result<()> {
        if self.lock.is_some() {
            self.failed()?;
            self.lock = None;
        }
        Ok(())
    }

    /// Saves the successful choice before removing the first-frame sentinel.
    pub fn presented(&mut self) -> Result<()> {
        let _active = termination::active()?;
        let Some(attempt) = self.pending.clone() else {
            return Ok(());
        };
        self.require_lease()?;
        self.recovery.succeeded(attempt);
        save(&self.root.join(CHOICE_FILE), &self.recovery)?;
        fs::remove_file(self.root.join(SENTINEL_FILE))
            .context("clear renderer startup sentinel")?;
        context::update(&self.root, self.recovery.working.as_ref());
        self.pending = None;
        self.lock = None;
        Ok(())
    }

    /// Prevents a completed guard from mutating files leased by a later startup.
    fn require_lease(&self) -> Result<()> {
        if self.lock.is_none() {
            bail!("renderer startup already completed");
        }
        Ok(())
    }
}

const SENTINEL_FILE: &str = "renderer-startup.json";
const LOCK_FILE: &str = "renderer-startup.lock";
const CHOICE_FILE: &str = "renderer-choice.json";
const MAX_FILE_BYTES: u64 = 64 * 1024;

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
