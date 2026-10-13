//! Durable renderer startup attempts and bounded recovery after a failed launch.

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

// The enum and its recovery order have one source so another backend cannot be omitted silently.
macro_rules! define_backends {
    ($($backend:ident),+ $(,)?) => {
        /// Backends in automatic startup order; the caller skips unsupported backends.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
        #[serde(rename_all = "lowercase")]
        pub enum Backend { $($backend),+ }

        impl Backend {
            /// The recovery order shared by policy and renderer setup.
            pub const ORDER: &'static [Self] = &[$(Self::$backend),+];
        }
    };
}
define_backends!(Vulkan, Dx12, Gl);

/// Adapter identity and driver details recorded before device creation.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Adapter {
    pub name: String,
    pub vendor: u32,
    pub device: u32,
    pub driver: String,
    pub driver_info: String,
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

    /// Returns a bounded list, preferring the last working backend unless explicitly overridden.
    pub fn candidates(&self, supported: &[Backend], explicit: Option<&[Backend]>) -> Vec<Backend> {
        if let Some(explicit) = explicit {
            return Backend::ORDER
                .iter()
                .copied()
                .filter(|backend| supported.contains(backend) && explicit.contains(backend))
                .collect();
        }
        let mut order = Backend::ORDER.to_vec();
        if let Some(working) = &self.working {
            order.sort_by_key(|backend| *backend != working.backend);
        }
        order
            .into_iter()
            .filter(|backend| supported.contains(backend) && !self.failed.contains(backend))
            .collect()
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

    /// Flushes the next operation to disk before the caller enters a native driver.
    pub fn begin(&mut self, attempt: Attempt) -> Result<()> {
        save(&self.root.join(SENTINEL_FILE), &attempt)?;
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
        self.pending = None;
        Ok(())
    }
}

const SENTINEL_FILE: &str = "renderer-startup.json";
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
