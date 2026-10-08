//! Several local mods in one frame. Load order resolves every conflict.

use std::path::{Path, PathBuf};

use mod_host::{
    CameraDelta, ControlFrame, GameplayCameraRig, GameplayMob, GameplaySnapshot, MAX_LOADED_MODS,
    ModCue, ModGrants, ModHost, PlayerStateSnapshot,
};
use serde::Deserialize;

use super::ModRuntime;

/// Selects an ordered set of components, each with its own grants.
pub(super) const SET_ENV: &str = "CINNABAR_MOD_SET";
const MAX_SET_BYTES: usize = 16 * 1024;

/// A mod after the first, loaded only from an explicit set.
pub(super) struct Companion {
    pub host: ModHost,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModSet {
    version: u32,
    mods: Vec<SetEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SetEntry {
    component: PathBuf,
    #[serde(default)]
    grants: ModGrants,
}

/// Reads a bounded set file; its order is the load order.
pub(super) fn read_set(path: &Path) -> Result<Vec<(PathBuf, ModGrants)>, String> {
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
    if let Some(entry) = set.mods.iter().find(|entry| !entry.component.is_absolute()) {
        return Err(format!(
            "mod set component {} must be absolute",
            entry.component.display()
        ));
    }
    Ok(set
        .mods
        .into_iter()
        .map(|entry| (entry.component, entry.grants))
        .collect())
}

impl ModRuntime {
    pub(super) fn host_count(&self) -> usize {
        1 + self.companions.len()
    }

    /// Index 0 is the first-loaded mod; companions follow in load order.
    pub(super) fn host(&self, index: usize) -> &ModHost {
        match index {
            0 => &self.host,
            _ => &self.companions[index - 1].host,
        }
    }

    pub(super) fn host_mut(&mut self, index: usize) -> &mut ModHost {
        match index {
            0 => &mut self.host,
            _ => &mut self.companions[index - 1].host,
        }
    }

    /// The first mod in load order that publishes a panel owns the panel and its events.
    pub(super) fn panel_owner(&self) -> usize {
        (0..self.host_count())
            .find(|&index| self.host(index).panel().is_some())
            .unwrap_or(0)
    }

    /// The earliest card publisher owns the retained card surface.
    pub(super) fn hud_owner(&self) -> Option<usize> {
        (0..self.host_count()).find(|&index| self.host(index).hud().is_some())
    }

    /// The earliest cursor publisher owns the retained cursor replacement.
    pub(super) fn crosshair_owner(&self) -> Option<usize> {
        (0..self.host_count()).find(|&index| self.host(index).crosshair().is_some())
    }

    /// Every mod's label in load order, joined and cut to the plain-text limit.
    pub(super) fn merged_label(&mut self) -> Option<&str> {
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
    pub(super) fn render_outputs(
        &self,
    ) -> impl Iterator<Item = (&mod_host::mod_render::RenderOutput, u64)> {
        (0..self.host_count()).map(|index| self.host(index).render())
    }

    pub(super) fn reserved_keys(&self) -> Vec<String> {
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
pub(super) fn claim_controls(
    frame: &ControlFrame,
    claimed: &[String],
    panel_owner: bool,
) -> ControlFrame {
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
pub(super) struct FrameInput<'a> {
    pub pressed: bool,
    pub controls: &'a ControlFrame,
    /// Every mod's committed cues from the previous frame.
    pub previous_cues: &'a [ModCue],
    /// Captured once from the current local session, delivered only to granted components.
    pub player_state: Option<&'a PlayerStateSnapshot>,
}

/// Runs each mod once in load order with its own grants and merges what they commit.
/// A failing mod is reported and quarantined by its host; the others still run.
pub(super) fn run_frame(
    runtime: &mut ModRuntime,
    input: FrameInput<'_>,
    mut world: impl FnMut(&ModGrants) -> (Option<GameplaySnapshot>, Vec<GameplayMob>),
    mut failed: impl FnMut(usize, String),
) -> Merged {
    let owner = runtime.panel_owner();
    let mut claimed = Vec::new();
    let mut merged = Merged::default();
    for index in 0..runtime.host_count() {
        let (snapshot, mobs) = world(runtime.host(index).grants());
        let mut controls = claim_controls(input.controls, &claimed, index == owner);
        controls.gameplay = snapshot.is_some();
        claimed.extend(runtime.host(index).reserved_keys().iter().cloned());
        let host = runtime.host_mut(index);
        host.deliver_cues(input.previous_cues.to_vec());
        let player_state = host
            .grants()
            .player_state
            .then(|| input.player_state.cloned())
            .flatten();
        if host.is_active()
            && let Err(error) =
                host.frame_with_player_state(input.pressed, snapshot, mobs, player_state, controls)
        {
            failed(index, format!("{error:#}"));
        }
        if let Some(error) = host.take_settings_error() {
            eprintln!("Cinnabar extension preferences could not be saved: {error}");
        }
        merged.absorb(host);
    }
    merged
}

/// One frame's combined output; for single-valued outputs the earliest mod wins.
#[derive(Default)]
pub(super) struct Merged {
    pub rig: Option<GameplayCameraRig>,
    pub preserve_teleport_rotation: bool,
    pub item_use_delay_fix: Option<(u64, i32)>,
    pub delta: Option<CameraDelta>,
    pub time_override: Option<u32>,
    pub attack_reach: Option<f32>,
    pub attack_pulse: bool,
    pub commands: Vec<String>,
    pub cues: Vec<ModCue>,
}

impl Merged {
    /// Consumes `host`'s committed output, in load order.
    pub fn absorb(&mut self, host: &mut ModHost) {
        let interaction = host.take_interaction();
        self.attack_reach = self.attack_reach.or(interaction.attack_reach);
        self.attack_pulse |= interaction.attack_pulse;
        let delta = host.take_camera_delta();
        self.delta = self.delta.or(delta);
        self.rig = self.rig.or(host.camera_rig());
        self.preserve_teleport_rotation |= host.preserves_teleport_rotation();
        self.item_use_delay_fix = self.item_use_delay_fix.or(host.item_use_delay_fix());
        self.time_override = self.time_override.or(host.time_override());
        self.commands.extend(host.take_commands());
        self.cues.extend(host.take_cues());
    }
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
