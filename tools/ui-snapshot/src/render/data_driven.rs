//! Renders script forms shown through data-driven UI, from a server-shaped store.

use std::path::Path;

use client_ui::ui_runtime::{SequencedUiEvent, UiRuntime};
use protocol::ddui::ScriptedScreen;
use protocol::test_support::ddui::{custom_form_property, message_box_property, show_events};

use super::form_overlap::png_for;
use crate::{carriers::Error, fixtures::State};

/// Captures `screen` after its store arrives and the server shows it.
pub(super) fn png(state: &State, dir: &Path, screen: ScriptedScreen) -> Result<Vec<u8>, Error> {
    let mut runtime = UiRuntime::new(1);
    let mut player = player_state::PlayerState::new(1);
    let property = match screen {
        ScriptedScreen::CustomForm => custom_form_property(),
        ScriptedScreen::MessageBox => message_box_property(),
    };
    for (sequence, event) in (1..).zip(show_events(screen, 1, 7, property)) {
        runtime
            .apply(
                &mut player,
                SequencedUiEvent {
                    session_id: 1,
                    fifo_sequence: sequence,
                    local_millis: sequence * 10,
                    server_tick: None,
                    event,
                },
            )
            .map_err(|error| format!("{error:?}"))?;
    }
    png_for(state, dir, &runtime)
}
