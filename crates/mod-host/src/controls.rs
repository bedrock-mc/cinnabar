//! Bounded, transactional mod controls. No import exposes arbitrary files or input injection.

use super::{MAX_IMPORT_WRITES, State, cinnabar};
use crate::{ControlFrame, InteractionOutput, empty_controls};
use anyhow::{Result, bail, ensure};
use ui::mod_panel::{MAX_PANEL_BYTES, Panel};

pub(super) struct ControlState {
    pub frame: ControlFrame,
    pub panel: Option<Panel>,
    pub open: bool,
    pub keys: Vec<String>,
    pub interaction: InteractionOutput,
    pub dirty_settings: Option<String>,
    settings: String,
    pending_panel: Option<Option<Panel>>,
    pending_keys: Option<Vec<String>>,
    pending_settings: Option<String>,
    pub pending_interaction: InteractionOutput,
    reads: u32,
    writes: u32,
}

impl ControlState {
    pub(super) fn settings(&self) -> &str {
        &self.settings
    }
    pub fn new(settings: String) -> Self {
        Self {
            frame: empty_controls(),
            panel: None,
            open: false,
            keys: Vec::new(),
            interaction: InteractionOutput::default(),
            dirty_settings: None,
            settings,
            pending_panel: None,
            pending_keys: None,
            pending_settings: None,
            pending_interaction: InteractionOutput::default(),
            reads: 0,
            writes: 0,
        }
    }

    /// Starts a callback's import budget without changing retained controls or pending output.
    pub(super) fn reset_budget(&mut self) {
        self.reads = 0;
        self.writes = 0;
    }

    pub fn begin_frame(&mut self) {
        self.reset_budget();
        self.frame = empty_controls();
        self.interaction = InteractionOutput::default();
        self.pending_interaction = InteractionOutput::default();
    }

    pub fn commit(&mut self) {
        if let Some(panel) = self.pending_panel.take() {
            self.panel = panel;
            if self.panel.is_none() {
                self.open = false;
            }
        }
        if let Some(keys) = self.pending_keys.take() {
            self.keys = keys;
        }
        if let Some(settings) = self.pending_settings.take() {
            self.settings.clone_from(&settings);
            self.dirty_settings = Some(settings);
        }
        self.interaction = std::mem::take(&mut self.pending_interaction);
    }

    pub fn revoke(&mut self) {
        self.panel = None;
        self.open = false;
        self.keys.clear();
        self.pending_panel = None;
        self.pending_keys = None;
        self.pending_settings = None;
        self.pending_interaction = InteractionOutput::default();
        self.interaction = InteractionOutput::default();
        self.frame = empty_controls();
    }

    fn budget(&mut self, read: bool) -> Result<()> {
        let count = if read {
            &mut self.reads
        } else {
            &mut self.writes
        };
        *count += 1;
        if *count > MAX_IMPORT_WRITES {
            bail!("controls import budget exhausted");
        }
        Ok(())
    }
}

fn key_valid(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= mod_api::MAX_CONTROL_KEY_BYTES
        && key.bytes().all(|c| c.is_ascii_alphanumeric())
}

pub(super) fn validate_frame(frame: &ControlFrame) -> Result<()> {
    ensure!(
        frame.seconds.is_finite() && (0.0..=1.0).contains(&frame.seconds),
        "invalid controls frame duration"
    );
    ensure!(
        frame.keys_pressed.len() <= mod_api::MAX_CONTROL_KEYS
            && frame.keys_pressed.iter().all(|key| key_valid(key))
            && frame.keys_held.len() <= mod_api::MAX_CONTROL_KEYS
            && frame.keys_held.iter().all(|key| key_valid(key)),
        "invalid controls key edges"
    );
    ensure!(
        frame.events.len() <= ui::mod_panel::MAX_PANEL_CONTROLS
            && frame.events.iter().all(|event| !event.id.is_empty()
                && event.id.len() <= ui::mod_panel::MAX_PANEL_ID_BYTES
                && event
                    .id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_-.".contains(&byte))
                && event.value.is_finite()),
        "invalid panel events"
    );
    Ok(())
}

pub(super) fn read(state: &mut State) -> Result<Result<ControlFrame, String>> {
    state.controls.budget(true)?;
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    Ok(Ok(state.controls.frame.clone()))
}

/// Imports only requested current keys and events without changing native input ownership.
pub(super) fn read_selected(
    state: &mut State,
    selection: cinnabar::extension::input::Selection,
) -> Result<Result<ControlFrame, String>> {
    state.controls.budget(true)?;
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    if ![
        selection.keys_pressed.as_deref(),
        selection.keys_held.as_deref(),
    ]
    .into_iter()
    .all(|names| {
        names.is_none_or(|names| {
            names.len() <= mod_api::MAX_CONTROL_KEYS && names.iter().all(|key| key_valid(key))
        })
    }) {
        return Ok(Err("invalid controls selection".into()));
    }
    let frame = &state.controls.frame;
    Ok(Ok(ControlFrame {
        seconds: frame.seconds,
        focused: frame.focused,
        gameplay: frame.gameplay,
        panel_open: frame.panel_open,
        keys_pressed: selected_keys(&frame.keys_pressed, selection.keys_pressed.as_deref()),
        keys_held: selected_keys(&frame.keys_held, selection.keys_held.as_deref()),
        events: if selection.events {
            frame.events.clone()
        } else {
            Vec::new()
        },
    }))
}

/// Retains matching physical key observations in their original native order.
fn selected_keys(keys: &[String], selection: Option<&[String]>) -> Vec<String> {
    let Some(names) = selection else {
        return keys.to_vec();
    };
    keys.iter()
        .filter(|key| names.contains(*key))
        .cloned()
        .collect()
}

pub(super) fn reserve(state: &mut State, keys: Vec<String>) -> Result<Result<(), String>> {
    state.controls.budget(false)?;
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    if keys.len() > mod_api::MAX_CONTROL_KEYS || !keys.iter().all(|key| key_valid(key)) {
        return Ok(Err("invalid reserved key list".into()));
    }
    let mut keys = keys;
    keys.sort();
    keys.dedup();
    state.controls.pending_keys = Some(keys);
    Ok(Ok(()))
}

impl cinnabar::extension::panel::Host for State {
    fn set_content(&mut self, json: String) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.controls {
            return Ok(Err("controls capability denied".into()));
        }
        if json.len() > MAX_PANEL_BYTES {
            return Ok(Err("panel exceeds byte limit".into()));
        }
        if json.is_empty() {
            self.controls.pending_panel = Some(None);
            return Ok(Ok(()));
        }
        let panel = match serde_json::from_str::<Panel>(&json).and_then(|panel| {
            panel
                .validate()
                .map(|()| panel)
                .map_err(serde::de::Error::custom)
        }) {
            Ok(panel) => panel,
            Err(error) => return Ok(Err(format!("invalid mod panel: {error}"))),
        };
        if !key_valid(&panel.toggle_key) {
            return Ok(Err("invalid panel toggle key".into()));
        }
        self.controls.pending_panel = Some(Some(panel));
        Ok(Ok(()))
    }
}

impl cinnabar::extension::settings::Host for State {
    fn load(&mut self) -> Result<Result<String, String>> {
        self.controls.budget(true)?;
        if !self.grants.settings {
            return Ok(Err("settings capability denied".into()));
        }
        Ok(Ok(self.controls.settings.clone()))
    }

    fn save(&mut self, json: String) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.settings {
            return Ok(Err("settings capability denied".into()));
        }
        if json.len() > mod_api::MAX_SETTINGS_BYTES
            || !serde_json::from_str::<serde_json::Value>(&json)
                .is_ok_and(|value| value.is_object())
        {
            return Ok(Err("settings must be a bounded JSON object".into()));
        }
        self.controls.pending_settings = Some(json);
        Ok(Ok(()))
    }
}

impl State {
    pub(super) fn set_reach(&mut self, blocks: Option<f32>) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.interaction {
            return Ok(Err("interaction capability denied".into()));
        }
        if blocks.is_some_and(|blocks| {
            !blocks.is_finite() || !(0.0..=mod_api::MAX_ENTITY_REACH_BLOCKS).contains(&blocks)
        }) {
            return Ok(Err("attack reach exceeds local range limit".into()));
        }
        if blocks.is_some() && self.snapshot.is_none() {
            return Ok(Err("attack reach requires a current gameplay frame".into()));
        }
        self.controls.pending_interaction.attack_reach = blocks;
        Ok(Ok(()))
    }

    pub(super) fn pulse_attack(&mut self) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.interaction {
            return Ok(Err("interaction capability denied".into()));
        }
        if !self
            .snapshot
            .as_ref()
            .is_some_and(|frame| frame.attack_held)
        {
            return Ok(Err(
                "attack pulse requires a held current gameplay attack".into()
            ));
        }
        self.controls.pending_interaction.attack_pulse = true;
        Ok(Ok(()))
    }
}

#[cfg(test)]
#[path = "controls_tests.rs"]
mod tests;
