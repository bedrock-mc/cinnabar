//! Bounded, transactional mod controls. No import exposes arbitrary files or input injection.

use super::{MAX_IMPORT_WRITES, State, cinnabar};
use crate::{ControlFrame, InteractionOutput, ModHost, empty_controls};
use anyhow::{Context, Result, bail, ensure};
use std::sync::Arc;
use ui::mod_panel::{MAX_PANEL_BYTES, Panel, PanelBundle, PanelPatch};

impl ModHost {
    /// Queues validated native text edits for this owner's next executed frame callback.
    pub fn queue_text_events(&mut self, events: Vec<ui::mod_panel::TextEvent>) -> Result<()> {
        self.instance.queue_text_events(events)
    }
    /// Queues additive live field input for this owner's next frame callback.
    pub fn queue_field_events(&mut self, events: Vec<ui::mod_panel::FieldEvent>) -> Result<()> {
        self.instance.queue_field_events(events)
    }
    /// Shares the validated local UI catalog with presentation preparation workers.
    pub fn panel_bundle(&self) -> Option<Arc<PanelBundle>> {
        self.instance.panel_bundle()
    }

    /// Returns the panel from the last successful callback.
    pub fn panel(&self) -> Option<&Panel> {
        self.instance.panel()
    }

    /// Prepares a bounded local UI catalog once, outside component callbacks.
    pub fn set_panel_bundle(&mut self, json: &str) -> Result<()> {
        ensure!(
            self.grants.controls && self.instance.is_active(),
            "controls capability denied"
        );
        let bundle = PanelBundle::parse(json).map_err(anyhow::Error::msg)?;
        let bundle = Arc::new(bundle);
        self.instance.set_panel_bundle(Arc::clone(&bundle))?;
        self.panel_bundle_override = Some(bundle);
        Ok(())
    }

    /// Reports whether the retained panel is open.
    pub fn panel_open(&self) -> bool {
        self.instance.panel_open()
    }

    /// Changes the retained panel's open state without entering the guest.
    pub fn set_panel_open(&mut self, open: bool) {
        self.instance.set_panel_open(open);
    }

    /// Returns the physical keys reserved by the last successful callback.
    pub fn reserved_keys(&self) -> &[String] {
        self.instance.reserved_keys()
    }

    /// Consumes committed local interaction requests once.
    pub fn take_interaction(&mut self) -> InteractionOutput {
        self.instance.take_interaction()
    }
}

pub(super) struct ControlState {
    pub frame: ControlFrame,
    pub text_events: Vec<ui::mod_panel::TextEvent>,
    pub field_events: Vec<ui::mod_panel::FieldEvent>,
    text_events_in_frame: bool,
    pub panel: Option<Panel>,
    pub bundle: Option<Arc<PanelBundle>>,
    pub open: bool,
    pub keys: Vec<String>,
    pub interaction: InteractionOutput,
    pub dirty_settings: Option<String>,
    settings: String,
    pending_panel: Option<Option<Panel>>,
    pending_open: Option<bool>,
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
            text_events: Vec::new(),
            field_events: Vec::new(),
            text_events_in_frame: false,
            panel: None,
            bundle: None,
            open: false,
            keys: Vec::new(),
            interaction: InteractionOutput::default(),
            dirty_settings: None,
            settings,
            pending_panel: None,
            pending_open: None,
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
        self.text_events_in_frame = false;
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
        if let Some(open) = self.pending_open.take() {
            self.open = open && self.panel.is_some();
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
        self.text_events.clear();
        self.field_events.clear();
        self.text_events_in_frame = false;
        self.panel = None;
        self.open = false;
        self.keys.clear();
        self.pending_panel = None;
        self.pending_open = None;
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

    /// Exposes committed native text only while a regular frame callback owns input.
    pub(super) fn begin_text_frame(&mut self) {
        self.text_events_in_frame = true;
    }

    /// Keeps input for a deferred frame while preventing unrelated callbacks from reading it.
    pub(super) fn finish_text_frame(&mut self, deferred: bool) {
        self.text_events_in_frame = false;
        if !deferred {
            self.text_events.clear();
            self.field_events.clear();
        }
    }
}

/// Admits an entire native batch before updating its retained values or pending input.
pub(super) fn queue_text_events(
    state: &mut State,
    events: Vec<ui::mod_panel::TextEvent>,
) -> Result<()> {
    ensure!(state.grants.controls, "controls capability denied");
    ensure!(
        state.controls.text_events.len() + events.len() <= ui::mod_panel::MAX_PANEL_EVENTS,
        "too many native text events"
    );
    if events.is_empty() {
        return Ok(());
    }
    let panel = state
        .controls
        .panel
        .as_mut()
        .context("native text events require a retained panel")?;
    for event in &events {
        let Some(ui::mod_panel::Control::Text {
            min_chars,
            max_chars,
            ..
        }) = panel.controls.iter().find(|c| c.id() == event.id)
        else {
            bail!("native text event does not match a text control");
        };
        ui::mod_panel::validate_text_value(&event.value, *min_chars, *max_chars)
            .map_err(anyhow::Error::msg)?;
    }
    for event in &events {
        if let Some(ui::mod_panel::Control::Text { value, .. }) =
            panel.controls.iter_mut().find(|c| c.id() == event.id)
        {
            value.clone_from(&event.value);
        }
    }
    state.controls.text_events.extend(events);
    Ok(())
}

/// Reads the native text batch without changing its stable view during this callback.
pub(super) fn read_text_events(
    state: &mut State,
) -> Result<Result<Vec<cinnabar::extension::input::TextEvent>, String>> {
    state.controls.budget(true)?;
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    if !state.controls.text_events_in_frame {
        return Ok(Ok(Vec::new()));
    }
    Ok(Ok(state
        .controls
        .text_events
        .iter()
        .map(|event| cinnabar::extension::input::TextEvent {
            id: event.id.clone(),
            value: event.value.clone(),
        })
        .collect()))
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
        frame.events.len() <= ui::mod_panel::MAX_PANEL_EVENTS
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
    fn set_open(&mut self, open: bool) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.controls {
            return Ok(Err("controls capability denied".into()));
        }
        let present = self
            .controls
            .pending_panel
            .as_ref()
            .map_or(self.controls.panel.is_some(), |p| p.is_some());
        if open && !present {
            return Ok(Err("opening requires a panel".into()));
        }
        self.controls.pending_open = Some(open);
        Ok(Ok(()))
    }
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

    fn update_content(&mut self, json: String) -> Result<Result<(), String>> {
        self.controls.budget(false)?;
        if !self.grants.controls {
            return Ok(Err("controls capability denied".into()));
        }
        let current = self
            .controls
            .pending_panel
            .as_ref()
            .map_or(self.controls.panel.as_ref(), Option::as_ref);
        let Some(current) = current else {
            return Ok(Err("panel update requires retained content".into()));
        };
        if let Some(bundle) = &self.controls.bundle
            && !bundle.matches_schema(current)
        {
            return Ok(Err(
                "panel update schema differs from the registered bundle".into(),
            ));
        }
        match PanelPatch::parse(&json).and_then(|patch| match &self.controls.bundle {
            Some(bundle) => patch.apply_bundle(current, bundle),
            None => patch.apply(current, &std::collections::BTreeMap::new()),
        }) {
            Ok(panel) => {
                self.controls.pending_panel = Some(Some(panel));
                Ok(Ok(()))
            }
            Err(error) => Ok(Err(format!("invalid panel update: {error}"))),
        }
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

/// Admits a batch transactionally, retaining new values independently of deferred delivery history.
pub(super) fn queue_field_events(
    state: &mut State,
    events: Vec<ui::mod_panel::FieldEvent>,
) -> Result<()> {
    use ui::mod_panel::{Control, FieldEvent, MAX_PANEL_EVENTS};
    ensure!(state.grants.controls, "controls capability denied");
    if events.is_empty() {
        return Ok(());
    }
    let panel = state
        .controls
        .panel
        .as_mut()
        .context("native field events require a retained panel")?;
    for event in &events {
        let id = match event {
            FieldEvent::Change(e) | FieldEvent::Submit(e) => &e.id,
            FieldEvent::Key(e) => &e.id,
        };
        let Some(Control::Text {
            min_chars,
            max_chars,
            live,
            navigation_keys,
            ..
        }) = panel.controls.iter().find(|c| c.id() == id)
        else {
            bail!("native field event does not match a text control");
        };
        match event {
            FieldEvent::Change(e) => {
                ensure!(*live, "native change event requires a live field");
                ui::mod_panel::validate_text_value(&e.value, 0, *max_chars)
                    .map_err(anyhow::Error::msg)?;
            }
            FieldEvent::Submit(e) => {
                ui::mod_panel::validate_text_value(&e.value, *min_chars, *max_chars)
                    .map_err(anyhow::Error::msg)?;
            }
            FieldEvent::Key(e) => {
                ensure!(
                    *navigation_keys && ui::mod_panel::is_field_navigation_key(&e.key),
                    "invalid native field navigation key"
                );
            }
        }
    }
    let mut pending = state.controls.field_events.clone();
    for event in &events {
        if let FieldEvent::Change(next) = event {
            pending.retain(|old| !matches!(old, FieldEvent::Change(old) if old.id == next.id));
        }
        ensure!(
            pending.len() < MAX_PANEL_EVENTS,
            "too many native field events"
        );
        pending.push(event.clone());
    }
    for event in &events {
        if let FieldEvent::Change(e) | FieldEvent::Submit(e) = event
            && let Some(Control::Text {
                value,
                min_chars,
                max_chars,
                ..
            }) = panel.controls.iter_mut().find(|c| c.id() == e.id)
            && ui::mod_panel::validate_text_value(&e.value, *min_chars, *max_chars).is_ok()
        {
            value.clone_from(&e.value);
        }
    }
    state.controls.field_events = pending;
    Ok(())
}

/// Exposes one stable field batch only in a regular frame callback with controls authority.
pub(super) fn read_field_events(
    state: &mut State,
) -> Result<Result<Vec<cinnabar::extension::input::FieldEvent>, String>> {
    use cinnabar::extension::input as abi;
    use ui::mod_panel::FieldEvent;
    state.controls.budget(true)?;
    if !state.grants.controls {
        return Ok(Err("controls capability denied".into()));
    }
    if !state.controls.text_events_in_frame {
        return Ok(Ok(Vec::new()));
    }
    Ok(Ok(state
        .controls
        .field_events
        .iter()
        .map(|event| match event {
            FieldEvent::Change(e) => abi::FieldEvent::Change(abi::TextEvent {
                id: e.id.clone(),
                value: e.value.clone(),
            }),
            FieldEvent::Submit(e) => abi::FieldEvent::Submit(abi::TextEvent {
                id: e.id.clone(),
                value: e.value.clone(),
            }),
            FieldEvent::Key(e) => abi::FieldEvent::Key(abi::FieldKey {
                id: e.id.clone(),
                key: e.key.clone(),
                shift: e.modifiers.shift,
                control: e.modifiers.control,
                alt: e.modifiers.alt,
                super_key: e.modifiers.super_key,
            }),
        })
        .collect()))
}
