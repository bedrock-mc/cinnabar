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
pub use mod_api::{
    MAX_CAMERA_DELTA_RADIANS, MAX_CONTROL_KEYS, MAX_GAMEPLAY_MOBS, MAX_GAMEPLAY_PLAYERS,
    MAX_LOADED_MODS, MAX_MOB_RANGE_BLOCKS, MAX_MOB_TYPE_BYTES,
};
#[cfg(feature = "execution")]
pub use mod_render;
#[cfg(feature = "execution")]
pub use runtime::cinnabar::extension::gameplay::{
    CameraRig as GameplayCameraRig, Mob as GameplayMob, Player as GameplayPlayer,
    Snapshot as GameplaySnapshot, Vector3 as GameplayVector3,
};
#[cfg(feature = "execution")]
pub use runtime::cinnabar::extension::{
    events::Cue as ModCue, input::Controls as ControlFrame, panel::Event as ControlEvent,
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
    anyhow::{Context, Result},
    runtime::Instance,
    sha2::{Digest, Sha256},
    std::path::PathBuf,
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
/// Field names are the registration and set-file grant names.
#[cfg(feature = "execution")]
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModGrants {
    /// Allows this instance to replace visual time only.
    pub environment: bool,
    /// Allows current-frame remote player and camera pose reads.
    pub players: bool,
    /// Allows bounded local camera rotation, rigs, and per-frame teleport aim preservation.
    pub camera: bool,
    /// Allows local key edges, reserved bindings and the retained settings panel.
    pub controls: bool,
    /// Allows bounded actor attack range and held-attack press requests.
    pub interaction: bool,
    /// Allows the selected component's bounded companion settings file.
    pub settings: bool,
    /// Allows sandboxed post passes and bounded world primitives.
    pub render: bool,
    /// Lets render passes read scene depth.
    pub render_depth: bool,
    /// Allows current-frame reads of nearby non-player actors.
    pub entities: bool,
    /// Command names this instance may request; empty denies command requests.
    pub commands: Vec<String>,
    /// Allows bounded post-login packet delay through the private core endpoint.
    pub packet_delay: bool,
    /// Allows retained full-block highlights of matching loaded blocks.
    pub block_highlights: bool,
    /// Allows retained local fullbright lighting, without altering server light data.
    pub fullbright: bool,
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
        self.frame_with_world(pressed, snapshot, Vec::new(), controls)
    }

    /// Adds nearby mobs, readable only with the entities grant and a current snapshot.
    pub fn frame_with_world(
        &mut self,
        pressed: bool,
        snapshot: Option<GameplaySnapshot>,
        mobs: Vec<GameplayMob>,
        controls: ControlFrame,
    ) -> Result<()> {
        self.instance.frame(pressed, snapshot, mobs, controls)?;
        self.queue_settings();
        Ok(())
    }

    /// Whether the last successful gameplay callback opted in to preserving teleport aim.
    pub fn preserves_teleport_rotation(&self) -> bool {
        self.instance.preserves_teleport_rotation()
    }

    /// The retained camera rig from the last successful callback.
    pub fn camera_rig(&self) -> Option<GameplayCameraRig> {
        self.instance.camera_rig()
    }

    /// Consumes the last successful frame's granted command requests once.
    pub fn take_commands(&mut self) -> Vec<String> {
        self.instance.take_commands()
    }

    /// Cues the next callback can poll, typically last frame's from every loaded mod.
    pub fn deliver_cues(&mut self, cues: Vec<ModCue>) {
        self.instance.deliver_cues(cues);
    }

    /// Consumes the last successful frame's presentation cues once.
    pub fn take_cues(&mut self) -> Vec<ModCue> {
        self.instance.take_cues()
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

    /// Retained request from a successful callback, independent of UI focus.
    pub fn packet_delay_ms(&self) -> u32 {
        self.instance.packet_delay_ms()
    }
    /// Successfully committed local lighting override.
    pub fn fullbright(&self) -> bool {
        self.instance.fullbright()
    }

    /// Committed selection; no raw block reads are exposed to the component.
    pub fn block_highlights(&self) -> Option<&mod_api::BlockHighlightSpec> {
        self.instance.block_highlights()
    }

    /// Explicit opt-in to the private core's last-relayed local position witness.
    pub fn show_real_position(&self) -> bool {
        self.instance.show_real_position()
    }

    /// Consumes the last successful frame's rotation once, without entering the guest.
    pub fn take_camera_delta(&mut self) -> Option<CameraDelta> {
        self.instance.take_camera_delta()
    }

    /// Committed render output and a process-unique generation that changes with it.
    pub fn render(&self) -> (&mod_render::RenderOutput, u64) {
        self.instance.render()
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
        let bytes = load::read_component(&self.path)?;
        let digest = Sha256::digest(&bytes).into();
        if self.attempted == digest {
            return Ok(false);
        }
        self.attempted = digest;
        let candidate = Instance::new(
            &self.engine,
            &bytes,
            self.grants.clone(),
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
        keys_held: Vec::new(),
        events: Vec::new(),
    }
}

#[cfg(all(test, feature = "execution"))]
mod tests;
