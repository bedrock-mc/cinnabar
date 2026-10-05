//! Experimental component host. Only the explicit WIT imports carry authority.

pub mod helper;
#[cfg(feature = "execution")]
mod load;
#[cfg(feature = "execution")]
mod runtime;
#[cfg(feature = "execution")]
pub mod server;
#[cfg(feature = "execution")]
mod settings;

#[cfg(feature = "execution")]
pub use mod_api::{MAX_CAMERA_DELTA_RADIANS, MAX_CONTROL_KEYS, MAX_GAMEPLAY_PLAYERS};
#[cfg(feature = "execution")]
pub use runtime::cinnabar::extension::gameplay::{
    Player as GameplayPlayer, Snapshot as GameplaySnapshot, Vector3 as GameplayVector3,
};
#[cfg(feature = "execution")]
pub use runtime::cinnabar::extension::{
    input::Controls as ControlFrame, panel::Event as ControlEvent,
};

/// Successfully committed local interaction requests, consumed once per frame.
#[cfg(feature = "execution")]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InteractionOutput {
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
}

/// Committed local actor rotation; yaw turns left and pitch turns up, in radians.
#[cfg(feature = "execution")]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraDelta {
    pub yaw: f32,
    pub pitch: f32,
}
#[cfg(feature = "execution")]
use {
    anyhow::{Context, Result, ensure},
    runtime::Instance,
    sha2::{Digest, Sha256},
    std::{
        fs::File,
        io::Read,
        path::{Path, PathBuf},
    },
    wasmtime::Engine,
};

/// Maximum bytes accepted before compilation or allocation of a package buffer.
pub const MAX_COMPONENT_BYTES: usize = 4 * 1024 * 1024;
/// Plain-text UI limit, checked before publishing any guest output.
pub const MAX_LABEL_BYTES: usize = 256;
#[cfg(feature = "execution")]
pub(crate) const FRAME_FUEL: u64 = 100_000;
#[cfg(feature = "execution")]
pub(crate) const MEMORY_BYTES: usize = 16 * 1024 * 1024;

/// Explicit per-instance authority; optional capabilities are denied by default.
#[cfg(feature = "execution")]
#[derive(Clone, Copy, Debug, Default)]
pub struct ModGrants {
    /// Allows this instance to replace visual time only.
    pub environment: bool,
    /// Allows current-frame remote player and camera pose reads.
    pub players: bool,
    /// Allows bounded, transactional local camera rotation.
    pub camera: bool,
    /// Allows local key edges, reserved bindings and the retained settings panel.
    pub controls: bool,
    /// Allows bounded actor attack range and held-attack press requests.
    pub interaction: bool,
    /// Allows the selected component's bounded companion settings file.
    pub settings: bool,
}

/// A developer-selected component with transactional reload and trap quarantine.
#[cfg(feature = "execution")]
pub struct ModHost {
    engine: Engine,
    instance: Instance,
    path: PathBuf,
    attempted: [u8; 32],
    grants: ModGrants,
    settings_writer: Option<settings::SettingsWriter>,
    settings_seed: Option<String>,
}

#[cfg(feature = "execution")]
impl ModHost {
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
        self.frame_with_controls(pressed, snapshot, empty_controls())
    }

    /// Receives only bounded host-owned edges, alongside the current gameplay frame.
    pub fn frame_with_controls(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        controls: ControlFrame,
    ) -> Result<()> {
        self.instance.frame(pressed, snapshot, controls)?;
        self.queue_settings();
        Ok(())
    }

    fn queue_settings(&mut self) {
        if let Some(writer) = &self.settings_writer
            && writer.is_active()
            && let Some(json) = self.instance.settings_write()
        {
            writer.submit(json.to_owned());
            self.instance.settings_written();
        }
    }

    /// Consumes an asynchronous persistence error without quarantining the guest.
    pub fn take_settings_error(&self) -> Option<String> {
        self.settings_writer
            .as_ref()
            .and_then(settings::SettingsWriter::take_error)
    }

    pub fn panel(&self) -> Option<&ui::mod_panel::Panel> {
        self.instance.panel()
    }
    pub fn panel_open(&self) -> bool {
        self.instance.panel_open()
    }
    pub fn set_panel_open(&mut self, open: bool) {
        self.instance.set_panel_open(open);
    }
    pub fn reserved_keys(&self) -> &[String] {
        self.instance.reserved_keys()
    }
    pub fn take_interaction(&mut self) -> InteractionOutput {
        self.instance.take_interaction()
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
        let candidate = Instance::new(
            &self.engine,
            &bytes,
            self.grants,
            self.instance.settings().to_owned(),
        )
        .context("reload rejected; previous mod retained")?;
        self.instance = candidate;
        self.queue_settings();
        Ok(true)
    }
}

#[cfg(feature = "execution")]
pub fn empty_controls() -> ControlFrame {
    ControlFrame {
        seconds: 0.0,
        focused: false,
        gameplay: false,
        panel_open: false,
        keys_pressed: Vec::new(),
        events: Vec::new(),
    }
}

#[cfg(feature = "execution")]
fn read_settings(path: &Path, grants: ModGrants) -> Result<String> {
    if !grants.settings {
        return Ok(String::new());
    }
    let path = path.with_extension("settings.json");
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(String::new()),
        Err(error) => {
            return Err(error).with_context(|| format!("read mod settings {}", path.display()));
        }
    };
    let mut bytes = Vec::new();
    file.take((mod_api::MAX_SETTINGS_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= mod_api::MAX_SETTINGS_BYTES,
        "mod settings exceed byte limit"
    );
    Ok(String::from_utf8(bytes)?)
}

/// Bounds file reads even if a writer grows the file between metadata and read.
#[cfg(feature = "execution")]
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

#[cfg(all(test, feature = "execution"))]
mod tests;
