//! Native input imports and admission for text edits owned by one component.

use super::{Instance, State, cinnabar, controls};
use anyhow::Result;

impl cinnabar::extension::input::Host for State {
    /// Commits bounded, subtractive click policy only after a successful callback.
    fn set_click_limits(
        &mut self,
        attack: Option<u32>,
        item_use: Option<u32>,
    ) -> Result<Result<(), String>> {
        self.environment_writes += 1;
        if self.environment_writes > super::MAX_IMPORT_WRITES {
            anyhow::bail!("input policy budget exhausted");
        }
        if !self.grants.controls {
            return Ok(Err("controls capability denied".into()));
        }
        let limits = [attack, item_use];
        if limits
            .iter()
            .flatten()
            .any(|rate| !(1..=mod_api::MAX_CLICK_RATE).contains(rate))
        {
            return Ok(Err(format!(
                "click limits must be between 1 and {}",
                mod_api::MAX_CLICK_RATE
            )));
        }
        self.pending_click_limits = Some(limits);
        Ok(Ok(()))
    }

    /// Retains bounded switch debounce only after a successful callback.
    fn set_click_debounce(
        &mut self,
        attack_ms: Option<u32>,
        item_use_ms: Option<u32>,
    ) -> Result<Result<(), String>> {
        self.environment_writes += 1;
        if self.environment_writes > super::MAX_IMPORT_WRITES {
            anyhow::bail!("input policy budget exhausted");
        }
        if !self.grants.controls {
            return Ok(Err("controls capability denied".into()));
        }
        let intervals = [attack_ms, item_use_ms];
        if intervals
            .iter()
            .flatten()
            .any(|ms| !(1..=mod_api::MAX_CLICK_DEBOUNCE_MS).contains(ms))
        {
            return Ok(Err(format!(
                "click debounce must be between 1 and {} milliseconds",
                mod_api::MAX_CLICK_DEBOUNCE_MS
            )));
        }
        self.pending_click_debounce = Some(intervals);
        Ok(Ok(()))
    }

    /// Keeps native text payloads separate from the existing numeric event ABI.
    fn read_text_events(
        &mut self,
    ) -> Result<Result<Vec<cinnabar::extension::input::TextEvent>, String>> {
        controls::read_text_events(self)
    }
    /// Reads the same stable additive field batch throughout a frame callback.
    fn read_field_events(
        &mut self,
    ) -> Result<Result<Vec<cinnabar::extension::input::FieldEvent>, String>> {
        controls::read_field_events(self)
    }
    /// Reads only the host's one-frame action edge, never raw keyboard state.
    fn demo_pressed(&mut self) -> Result<bool> {
        Ok(self.pressed)
    }

    fn read_controls(&mut self) -> Result<Result<crate::ControlFrame, String>> {
        controls::read(self)
    }

    fn read_selected_controls(
        &mut self,
        selection: cinnabar::extension::input::Selection,
    ) -> Result<Result<crate::ControlFrame, String>> {
        controls::read_selected(self, selection)
    }

    fn reserve_keys(&mut self, keys: Vec<String>) -> Result<Result<(), String>> {
        controls::reserve(self, keys)
    }
}

impl Instance {
    /// Retains native committed text while a callback is deferred, without granting guest writes.
    pub(crate) fn queue_text_events(
        &mut self,
        events: Vec<ui::mod_panel::TextEvent>,
    ) -> Result<()> {
        anyhow::ensure!(
            self.is_active(),
            "native text events require an active component"
        );
        controls::queue_text_events(self.store.data_mut(), events)
    }
}

impl Instance {
    /// Admits bounded live field input only for an active component.
    pub(crate) fn queue_field_events(
        &mut self,
        events: Vec<ui::mod_panel::FieldEvent>,
    ) -> Result<()> {
        anyhow::ensure!(
            self.is_active(),
            "native field events require an active component"
        );
        controls::queue_field_events(self.store.data_mut(), events)
    }
}
