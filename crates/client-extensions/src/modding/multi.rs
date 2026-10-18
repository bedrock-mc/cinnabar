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
    /// Always contains at least one host, in load order.
    hosts: Vec<ModHost>,
    panel_selection: usize,
    budget: mod_host::CallbackBudget,
    hud_claims: mod_host::HudClaims,
    label: Option<String>,
    label_inputs: Vec<String>,
    label_rebuilds: u64,
}

/// Selects an ordered set of components and packages, each with its own grants.
pub const SET_ENV: &str = "CINNABAR_MOD_SET";
const MAX_SET_BYTES: usize = 16 * 1024;

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
    let bytes = mod_host::local_files::read(path, MAX_SET_BYTES)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
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
                    bevy::log::warn!("Cinnabar extension {path} disabled: {error:#}");
                })
                .ok()
        })
        .collect()
}

impl ModHosts {
    /// Keeps a nonempty collection of hosts in their original load order.
    pub fn new(host: ModHost, remaining: Vec<ModHost>) -> Self {
        Self {
            hosts: std::iter::once(host).chain(remaining).collect(),
            panel_selection: 0,
            budget: mod_host::CallbackBudget::new(mod_api::TOTAL_MOD_FRAME_FUEL),
            hud_claims: Default::default(),
            label: None,
            label_inputs: Vec::new(),
            label_rebuilds: 0,
        }
    }

    /// Resets the shared frame, event and loading allowances, and grants
    /// exclusive camera and time output to the first active instance publishing that output.
    pub fn begin_frame(&mut self) {
        self.budget.reset(mod_api::TOTAL_MOD_FRAME_FUEL);
        self.hud_claims.reset();
        for index in 0..self.host_count() {
            self.host_mut(index).begin_frame();
        }
        for index in 0..self.host_count() {
            self.authorize(index);
        }
    }

    /// Refreshes precedence before a callback, allowing a later mod to follow an earlier trap.
    fn authorize(&mut self, index: usize) {
        let camera = (0..index).any(|earlier| {
            self.host(earlier).is_active() && self.host(earlier).has_camera_output()
        });
        let environment = (0..index).any(|earlier| {
            self.host(earlier).is_active() && self.host(earlier).time_override().is_some()
        });
        let budget = self.budget.clone();
        let claims = self.hud_claims.clone();
        let host = self.host_mut(index);
        if host.is_active() {
            host.set_frame_authority(budget, camera, environment, index, claims);
        }
    }

    /// Exchanges two hosts while keeping the selected panel attached to its instance.
    pub fn swap(&mut self, left: usize, right: usize) {
        if left == right {
            return;
        }
        self.hosts.swap(left, right);
        if self.panel_selection == left {
            self.panel_selection = right;
        } else if self.panel_selection == right {
            self.panel_selection = left;
        }
    }

    /// Removes any host while another remains, preserving the surviving order.
    pub fn remove(&mut self, index: usize) -> ModHost {
        assert!(
            self.hosts.len() > 1,
            "a mod host collection must stay nonempty"
        );
        let host = self.hosts.remove(index);
        if self.panel_selection == index {
            self.panel_selection = 0;
        } else if self.panel_selection > index {
            self.panel_selection -= 1;
        }
        host
    }

    /// Appends a host at the lowest load precedence without changing panel selection.
    pub fn push(&mut self, host: ModHost) {
        self.hosts.push(host);
    }

    /// Consumes a collection containing exactly one host when its runtime is removed.
    pub fn into_single(mut self) -> ModHost {
        assert_eq!(
            self.hosts.len(),
            1,
            "other mod hosts still belong to this runtime"
        );
        self.hosts.pop().expect("nonempty mod hosts")
    }

    /// Borrows every host in load order without allowing collection changes.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut ModHost> {
        self.hosts.iter_mut()
    }

    /// Combines distinct HUD slots in precedence order within all shared presentation limits.
    pub fn merged_hud(&self) -> Option<ui::mod_hud::Hud> {
        let owner = self.hud_owner()?;
        let mut hud = self.host(owner).hud()?.clone();
        let mut rows = hud.cards.iter().map(|card| card.rows.len()).sum::<usize>();
        let mut cells = hud.cards.iter().map(|card| card.cells.len()).sum::<usize>();
        let mut compasses = hud
            .cards
            .iter()
            .filter(|card| card.compass.is_some())
            .count();
        for index in owner + 1..self.host_count() {
            if let Some(other) = self.host(index).hud() {
                for card in &other.cards {
                    if !hud.cards.iter().any(|existing| existing.id == card.id)
                        && hud.cards.len() < ui::mod_hud::MAX_HUD_CARDS
                        && rows + card.rows.len() <= ui::mod_hud::MAX_HUD_ROWS
                        && cells + card.cells.len() <= ui::mod_hud::MAX_HUD_CELLS
                        && compasses + usize::from(card.compass.is_some())
                            <= ui::mod_hud::MAX_HUD_COMPASSES
                    {
                        rows += card.rows.len();
                        cells += card.cells.len();
                        compasses += usize::from(card.compass.is_some());
                        hud.cards.push(card.clone());
                    }
                }
            }
        }
        Some(hud)
    }

    /// Counts every loaded component.
    pub fn host_count(&self) -> usize {
        self.hosts.len()
    }

    /// Borrows a component in load order.
    pub fn host(&self, index: usize) -> &ModHost {
        &self.hosts[index]
    }

    /// Borrows a component in load order for one callback or host action.
    pub fn host_mut(&mut self, index: usize) -> &mut ModHost {
        &mut self.hosts[index]
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
            return self.host(0).label();
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
        seconds: frame.seconds,
        focused: frame.focused,
        gameplay: frame.gameplay,
        panel_open: frame.panel_open,
        keys_pressed: free(&frame.keys_pressed),
        keys_held: free(&frame.keys_held),
        events: if panel_owner {
            frame.events.clone()
        } else {
            Vec::new()
        },
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
    pub modules: Option<&'a mod_host::module_observations::ModuleObservations>,
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
        union.terrain |= grants.terrain;
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
        let movement = if grants.movement { movement } else { None };
        let mut controls = claim_controls(input.controls, &claimed, index == owner);
        controls.panel_open = input.controls.panel_open && index == owner;
        controls.gameplay = snapshot.is_some() || movement.is_some();
        claimed.extend(runtime.host(index).reserved_keys().iter().cloned());
        runtime.authorize(index);
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
        let observed = observed
            .and_then(|()| host.observe_modules(input.modules.cloned().unwrap_or_default()));
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
            bevy::log::warn!("Cinnabar extension preferences could not be saved: {error}");
        }
        merged.absorb(host);
    }
    merged
}

/// Whether the host may receive the common gameplay pose and input snapshot.
fn needs_gameplay(grants: &ModGrants) -> bool {
    grants.terrain
        || grants.players
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
    pub hurt_style: Option<render_api::HurtCameraStyle>,
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
        self.hurt_style = self.hurt_style.or(host.camera_hurt_style());
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
