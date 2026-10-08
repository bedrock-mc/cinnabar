//! Form event fixture shared by app input integration tests.

use client_ui::ui_runtime::SequencedUiEvent;
use protocol::{FormKind, FormRequestEvent, ServerFormModel, TextMenuForm, UiEvent};
use std::sync::Arc;

/// Builds an authoritative two-button form event for one session.
pub(super) fn retained(form_id: u32, sequence: u64) -> SequencedUiEvent {
    SequencedUiEvent {
        session_id: 1,
        fifo_sequence: sequence,
        local_millis: sequence * 10,
        server_tick: None,
        event: UiEvent::Form(FormRequestEvent {
            form_id,
            kind: FormKind::Menu,
            title: Some(Arc::from("Choose 世界")),
            json: Arc::from("{}"),
            model: ServerFormModel::TextMenu(TextMenuForm {
                title: Arc::from("Choose 世界"),
                content: Arc::from("Pick one"),
                buttons: vec![Arc::from("First ✓"), Arc::from("第二")].into(),
                button_images: [].into(),
                omitted_images: 0,
            }),
        }),
    }
}
