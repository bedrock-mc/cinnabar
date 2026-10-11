//! Several local mods in one frame. Load order resolves every conflict.

use std::path::{Path, PathBuf};

use serde::Deserialize;
use {
    mod_api::MAX_LOADED_MODS,
    mod_host::{
        CameraDelta, ControlFrame, GameplayCameraRig, GameplayMob, GameplayMovementSnapshot,
        GameplaySnapshot, ModCue, ModGrants, ModHost, PlayerStateSnapshot,
    },
};

/// Loaded personal components and their merged labels, in load order.
pub struct ModHosts {
    pub host: ModHost,
    pub companions: Vec<Companion>,
    panel_selection: usize,
    label: Option<String>,
    label_inputs: Vec<String>,
    label_rebuilds: u64,
}

/// Selects an ordered set of components and packages, each with its own grants.
pub const SET_ENV: &str = "CINNABAR_MOD_SET";
const MAX_SET_BYTES: usize = 16 * 1024;

/// A mod after the first, loaded only from an explicit set.
pub struct Companion {
    pub host: ModHost,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModSet {
    version: u32,
    mods: Vec<SetEntry>,
}

/// One set entry names a bare component or a package directory, never both.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetEntry {
    #[serde(default)]
    component: Option<PathBuf>,
    #[serde(default)]
    package: Option<PathBuf>,
    #[serde(default)]
    grants: ModGrants,
}

/// Where a set entry's mod comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModSource {
    Component(PathBuf),
    /// A package's permissions need both its manifest's ask and the entry's grant.
    Package(PathBuf),
}

impl ModSource {
    /// Returns the component or package path selected by this entry.
    pub fn path(&self) -> &Path {
        match self {
            Self::Component(path) | Self::Package(path) => path,
        }
    }

    /// Loads this entry with only its explicitly supplied grants.
    pub fn load(&self, grants: ModGrants) -> anyhow::Result<ModHost> {
        match self {
            Self::Component(path) => ModHost::load_with_grants(path, grants),
            Self::Package(dir) => ModHost::load_package_with_grants(dir, grants),
        }
    }
}

/// Reads a bounded set file; its order is the load order.
pub fn read_set(path: &Path) -> Result<Vec<(ModSource, ModGrants)>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| {
            use std::io::Read;
            file.take(MAX_SET_BYTES as u64 + 1).read_to_end(&mut bytes)
        })
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    if bytes.len() > MAX_SET_BYTES {
        return Err("mod set exceeds its byte limit".into());
    }
    let set: ModSet =
        serde_json::from_slice(&bytes).map_err(|error| format!("invalid mod set: {error}"))?;
    if set.version != 1 {
        return Err("unsupported mod set version".into());
    }
    if set.mods.is_empty() || set.mods.len() > MAX_LOADED_MODS {
        return Err(format!("a mod set lists 1 to {MAX_LOADED_MODS} components"));
    }
    set.mods
        .into_iter()
        .map(|entry| {
            let source = match (entry.component, entry.package) {
                (Some(path), None) => ModSource::Component(path),
                (None, Some(dir)) => ModSource::Package(dir),
                _ => return Err("a mod set entry names one component or one package".into()),
            };
            if !source.path().is_absolute() {
                return Err(format!(
                    "mod set entry {} must be absolute",
                    source.path().display()
                ));
            }
            Ok((source, entry.grants))
        })
        .collect()
}

/// Loads valid entries in order and reports failures without discarding the other entries.
pub fn load_entries(mods: Vec<(ModSource, ModGrants)>) -> Vec<ModHost> {
    mods.into_iter()
        .filter_map(|(source, grants)| {
            source
                .load(grants)
                .inspect_err(|error| {
                    let path = source.path().display();
                    eprintln!("Cinnabar extension {path} disabled: {error:#}");
                })
                .ok()
        })
        .collect()
}

impl ModHosts {
    /// Keeps the first host and its companions in their original load order.
    pub fn new(host: ModHost, companions: Vec<Companion>) -> Self {
        Self {
            host,
            companions,
            panel_selection: 0,
            label: None,
            label_inputs: Vec::new(),
            label_rebuilds: 0,
        }
    }

    /// Counts the primary component and every companion.
    pub fn host_count(&self) -> usize {
        1 + self.companions.len()
    }

    /// Index 0 is the first-loaded mod; companions follow in load order.
    pub fn host(&self, index: usize) -> &ModHost {
        match index {
            0 => &self.host,
            _ => &self.companions[index - 1].host,
        }
    }

    /// Borrows a component in load order for one callback or host action.
    pub fn host_mut(&mut self, index: usize) -> &mut ModHost {
        match index {
            0 => &mut self.host,
            _ => &mut self.companions[index - 1].host,
        }
    }

    /// The selected panel publisher owns the retained panel and its events.
    pub fn panel_owner(&self) -> usize {
        if self.panel_selection < self.host_count()
            && self.host(self.panel_selection).panel().is_some()
        {
            return self.panel_selection;
        }
        (0..self.host_count())
            .find(|&index| self.host(index).panel().is_some())
            .unwrap_or(0)
    }

    /// Selects the earliest active panel whose toggle key was pressed, closing the old panel.
    pub fn select_panel(&mut self, pressed: &[String]) -> bool {
        let current = self.host(self.panel_owner());
        if current.panel_open() && current.panel().is_some_and(|panel| panel.capture_key) {
            return false;
        }
        let selected = (0..self.host_count()).find(|&index| {
            let host = self.host(index);
            host.is_active()
                && host
                    .panel()
                    .is_some_and(|panel| pressed.contains(&panel.toggle_key))
        });
        let Some(selected) = selected else {
            return false;
        };
        let previous = self.panel_owner();
        self.panel_selection = selected;
        if selected != previous {
            self.host_mut(previous).set_panel_open(false);
            return true;
        }
        false
    }

    /// Removes a companion without changing the remaining components' order.
    pub fn remove_companion(&mut self, index: usize) -> ModHost {
        assert!(index > 0);
        let host = self.companions.remove(index - 1).host;
        if self.panel_selection == index {
            self.panel_selection = 0;
        } else if self.panel_selection > index {
            self.panel_selection -= 1;
        }
        host
    }

    /// The earliest card publisher owns the retained card surface.
    pub fn hud_owner(&self) -> Option<usize> {
        (0..self.host_count()).find(|&index| self.host(index).hud().is_some())
    }

    /// The earliest cursor publisher owns the retained cursor replacement.
    pub fn crosshair_owner(&self) -> Option<usize> {
        (0..self.host_count()).find(|&index| self.host(index).crosshair().is_some())
    }

    /// The mod whose package draws beside the container screens and receives their input.
    pub fn screen_owner(&self) -> Option<usize> {
        screen_owner((0..self.host_count()).map(|index| {
            let host = self.host(index);
            let screens = host.screens();
            (
                host.package().is_some() && host.is_active() && host.grants().screen,
                screens.overlay.is_some() || screens.view.is_some(),
            )
        }))
    }

    /// Every mod's label in load order, joined and cut to the plain-text limit.
    pub fn merged_label(&mut self) -> Option<&str> {
        if self.host_count() == 1 {
            return self.host.label();
        }
        let mut current = (0..self.host_count()).filter_map(|index| self.host(index).label());
        let unchanged = self
            .label_inputs
            .iter()
            .map(String::as_str)
            .eq(&mut current);
        if !unchanged {
            self.label_inputs = (0..self.host_count())
                .filter_map(|index| self.host(index).label().map(str::to_owned))
                .collect();
            self.label = join_labels(&self.label_inputs);
            self.label_rebuilds += 1;
        }
        self.label.as_deref()
    }

    /// Every mod's render output with its generation, in load order.
    pub fn render_outputs(&self) -> impl Iterator<Item = (&mod_render::RenderOutput, u64)> {
        (0..self.host_count()).map(|index| self.host(index).render())
    }

    /// Collects the distinct physical keys reserved by all loaded components.
    pub fn reserved_keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = (0..self.host_count())
            .flat_map(|index| self.host(index).reserved_keys().iter().cloned())
            .collect();
        keys.sort();
        keys.dedup();
        keys
    }
}

/// The controls one mod sees: keys reserved by an earlier mod are withheld, and only the
/// panel owner receives panel events.
fn claim_controls(frame: &ControlFrame, claimed: &[String], panel_owner: bool) -> ControlFrame {
    let free = |keys: &[String]| -> Vec<String> {
        keys.iter()
            .filter(|key| !claimed.contains(key))
            .cloned()
            .collect()
    };
    ControlFrame {
        keys_pressed: free(&frame.keys_pressed),
        keys_held: free(&frame.keys_held),
        events: if panel_owner {
            frame.events.clone()
        } else {
            Vec::new()
        },
        ..frame.clone()
    }
}

/// Host-owned input shared by every mod in one frame.
pub struct FrameInput<'a> {
    pub pressed: bool,
    pub controls: &'a ControlFrame,
    /// Every mod's committed cues from the previous frame.
    pub previous_cues: &'a [ModCue],
    /// Captured once from the current local session, delivered only to granted components.
    pub player_state: Option<&'a PlayerStateSnapshot>,
    pub world_observation: Option<&'a mod_host::WorldObservation>,
    pub fps: Option<f32>,
    pub sidebar: Option<&'a mod_host::SidebarSnapshot>,
    pub chat: &'a [mod_host::ObservedChatMessage],
}

/// Runs each mod once in load order with its own grants and merges what they commit.
/// A failing mod is reported and quarantined by its host; the others still run.
pub fn run_frame(
    runtime: &mut ModHosts,
    input: FrameInput<'_>,
    mut world: impl FnMut(
        &ModGrants,
    ) -> (
        Option<GameplaySnapshot>,
        Vec<GameplayMob>,
        Option<GameplayMovementSnapshot>,
    ),
    mut failed: impl FnMut(usize, String),
) -> Merged {
    let mut union = ModGrants::default();
    let mut active = false;
    for index in 0..runtime.host_count() {
        let host = runtime.host(index);
        if !host.is_active() {
            continue;
        }
        active = true;
        let grants = host.grants();
        union.players |= grants.players;
        union.camera |= grants.camera;
        union.item_use |= grants.item_use;
        union.movement |= grants.movement;
        union.interaction |= grants.interaction;
        union.entities |= grants.entities;
        union.commands.extend(grants.commands.iter().cloned());
    }
    let (snapshot, mobs, movement) = if active {
        world(&union)
    } else {
        (None, Vec::new(), None)
    };
    let owner = runtime.panel_owner();
    let mut claimed = Vec::new();
    let mut merged = Merged::default();
    for index in 0..runtime.host_count() {
        let host = runtime.host(index);
        if !host.is_active() {
            continue;
        }
        let grants = host.grants();
        let snapshot = snapshot
            .as_ref()
            .filter(|_| needs_gameplay(grants))
            .map(|frame| GameplaySnapshot {
                players: if grants.players {
                    frame.players.clone()
                } else {
                    Vec::new()
                },
                session: frame.session,
                dimension: frame.dimension,
                eye: frame.eye,
                yaw: frame.yaw,
                pitch: frame.pitch,
                frame_seconds: frame.frame_seconds,
                attack_held: frame.attack_held,
            });
        let mobs = if grants.entities {
            mobs.clone()
        } else {
            Vec::new()
        };
        let movement = if grants.movement {
            movement.clone()
        } else {
            None
        };
        let mut controls = claim_controls(input.controls, &claimed, index == owner);
        controls.panel_open = input.controls.panel_open && index == owner;
        controls.gameplay = snapshot.is_some() || movement.is_some();
        claimed.extend(runtime.host(index).reserved_keys().iter().cloned());
        let host = runtime.host_mut(index);
        host.deliver_cues(input.previous_cues.to_vec());
        let player_state = host
            .grants()
            .player_state
            .then(|| input.player_state.cloned())
            .flatten();
        let observed = host.observe_presentation(
            input.world_observation.cloned(),
            input.fps,
            input.sidebar.cloned(),
            input.chat.to_vec(),
        );
        if let Err(error) = observed {
            failed(index, format!("{error:#}"));
            continue;
        }
        if let Err(error) = host.frame_with_movement(
            input.pressed,
            snapshot,
            mobs,
            player_state,
            movement,
            controls,
        ) {
            failed(index, format!("{error:#}"));
        }
        if let Some(error) = host.take_settings_error() {
            eprintln!("Cinnabar extension preferences could not be saved: {error}");
        }
        merged.absorb(host);
    }
    merged
}

/// Whether the host may receive the common gameplay pose and input snapshot.
fn needs_gameplay(grants: &ModGrants) -> bool {
    grants.players
        || grants.camera
        || grants.item_use
        || grants.movement
        || grants.interaction
        || grants.entities
        || !grants.commands.is_empty()
}

/// One frame's combined output; for single-valued outputs the earliest mod wins.
#[derive(Default)]
pub struct Merged {
    pub rig: Option<GameplayCameraRig>,
    pub preserve_teleport_rotation: bool,
    pub view_scale: Option<[f32; 2]>,
    pub item_use_delay_fix: Option<(u64, i32)>,
    pub delta: Option<CameraDelta>,
    pub time_override: Option<u32>,
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
    pub jump_pulse: bool,
    pub jump_cancel: bool,
    pub commands: Vec<String>,
    pub cues: Vec<ModCue>,
}

impl Merged {
    /// Consumes `host`'s committed output, in load order.
    pub fn absorb(&mut self, host: &mut ModHost) {
        self.jump_pulse |= host.take_jump_pulse();
        self.jump_cancel |= host.take_jump_cancel();
        let interaction = host.take_interaction();
        self.attack_reach = self.attack_reach.or(interaction.attack_reach);
        self.attack_pulse |= interaction.attack_pulse;
        let delta = host.take_camera_delta();
        self.delta = self.delta.or(delta);
        self.rig = self.rig.or(host.camera_rig());
        self.preserve_teleport_rotation |= host.preserves_teleport_rotation();
        self.view_scale = self.view_scale.or(host.camera_view_scale());
        self.item_use_delay_fix = self.item_use_delay_fix.or(host.item_use_delay_fix());
        self.time_override = self.time_override.or(host.time_override());
        self.commands.extend(host.take_commands());
        self.cues.extend(host.take_cues());
    }
}

/// Of mods in load order, each `(may draw, draws)`: the earliest that draws an overlay or view,
/// else the earliest that may draw one.
fn screen_owner(mods: impl Iterator<Item = (bool, bool)> + Clone) -> Option<usize> {
    let eligible = mods.enumerate().filter(|(_, (may, _))| *may);
    eligible
        .clone()
        .find(|(_, (_, draws))| *draws)
        .or_else(|| eligible.clone().next())
        .map(|(index, _)| index)
}

/// Joins labels with a separator, cut at a char boundary within the plain-text limit.
fn join_labels(labels: &[String]) -> Option<String> {
    if labels.is_empty() {
        return None;
    }
    let mut label = labels.join(" | ");
    if label.len() > mod_host::MAX_LABEL_BYTES {
        let mut end = mod_host::MAX_LABEL_BYTES;
        while !label.is_char_boundary(end) {
            end -= 1;
        }
        label.truncate(end);
    }
    Some(label)
}

#[cfg(test)]
#[path = "multi_tests.rs"]
mod tests;
