//! Native input imports and admission for text edits owned by one component.

use super::{Instance, State, cinnabar, controls};
use anyhow::Result;

impl cinnabar::extension::input::Host for State {
    /// Keeps native text payloads separate from the existing numeric event ABI.
    fn read_text_events(
        &mut self,
    ) -> Result<Result<Vec<cinnabar::extension::input::TextEvent>, String>> {
        controls::read_text_events(self)
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
