//! Experimental component host. Only the explicit WIT imports carry authority.

pub mod helper;
mod runtime;
pub mod server;

use anyhow::{Context, Result, ensure};
pub use mod_api::{MAX_CAMERA_DELTA_RADIANS, MAX_GAMEPLAY_PLAYERS};
use runtime::Instance;
pub use runtime::cinnabar::extension::gameplay::{
    Player as GameplayPlayer, Snapshot as GameplaySnapshot, Vector3 as GameplayVector3,
};

/// Committed local actor rotation; yaw turns left and pitch turns up, in radians.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraDelta {
    pub yaw: f32,
    pub pitch: f32,
}
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};
use wasmtime::{Config, Engine};

/// Maximum bytes accepted before compilation or allocation of a package buffer.
pub const MAX_COMPONENT_BYTES: usize = 4 * 1024 * 1024;
/// Plain-text UI limit, checked before publishing any guest output.
pub const MAX_LABEL_BYTES: usize = 256;
pub(crate) const FRAME_FUEL: u64 = 100_000;
pub(crate) const MEMORY_BYTES: usize = 16 * 1024 * 1024;

/// Explicit per-instance authority; optional capabilities are denied by default.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModGrants {
    /// Allows this instance to replace visual time only.
    pub environment: bool,
    /// Allows current-frame remote player and camera pose reads.
    pub players: bool,
    /// Allows bounded, transactional local camera rotation.
    pub camera: bool,
}

/// A developer-selected component with transactional reload and trap quarantine.
pub struct ModHost {
    engine: Engine,
    instance: Instance,
    path: PathBuf,
    attempted: [u8; 32],
    grants: ModGrants,
}

impl ModHost {
    /// Loads a local component with HUD and demo input, denying optional capabilities.
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with_grants(path, ModGrants::default())
    }

    /// Loads a component with the developer's explicit per-mod capability grants.
    pub fn load_with_grants(path: &Path, grants: ModGrants) -> Result<Self> {
        let bytes = read_component(path)?;
        let mut config = Config::new();
        config.wasm_component_model(true).consume_fuel(true);
        config.max_wasm_stack(256 * 1024);
        let engine = Engine::new(&config)?;
        let instance = Instance::new(&engine, &bytes, grants)?;
        Ok(Self {
            engine,
            instance,
            path: path.to_owned(),
            attempted: Sha256::digest(&bytes).into(),
            grants,
        })
    }

    /// Runs one bounded callback; a trap revokes its presentation and disables the guest.
    pub fn frame(&mut self, pressed: bool) -> Result<()> {
        self.frame_with_gameplay(pressed, None)
    }

    /// Runs a callback with a validated snapshot belonging only to this frame.
    pub fn frame_with_gameplay(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
    ) -> Result<()> {
        self.instance.frame(pressed, snapshot)
    }

    /// Consumes the last successful frame's rotation once, without entering the guest.
    pub fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        self.instance.take_camera_delta()
    }

    /// Returns only the last successfully committed plain-text label.
    pub fn label(&self) -> Option<&str> {
        self.instance.label()
    }

    /// Returns the committed visual override without entering the guest.
    pub fn time_override(&self) -> Option<u32> {
        self.instance.time_override()
    }

    /// Whether this guest can still receive callbacks.
    pub fn is_active(&self) -> bool {
        self.instance.active
    }

    /// Replaces an instance only after changed bytes compile and initialize.
    pub fn reload_if_changed(&mut self) -> Result<bool> {
        let bytes = read_component(&self.path)?;
        let digest = Sha256::digest(&bytes).into();
        if self.attempted == digest {
            return Ok(false);
        }
        self.attempted = digest;
        let candidate = Instance::new(&self.engine, &bytes, self.grants)
            .context("reload rejected; previous mod retained")?;
        self.instance = candidate;
        Ok(true)
    }
}

/// Bounds file reads even if a writer grows the file between metadata and read.
fn read_component(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("open mod {}", path.display()))?
        .take((MAX_COMPONENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= MAX_COMPONENT_BYTES,
        "component exceeds byte limit"
    );
    Ok(bytes)
}

#[cfg(test)]
mod tests;
