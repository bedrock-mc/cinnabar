//! App input and scheduling tests over the extracted presentation model.
use super::*;

use client_ui::ui_runtime::SequencedUiEvent;
use protocol::{TextCategory, TextEvent, TextKind, UiEvent};
use std::sync::Arc;
use ui::DpiScale;

mod chat_screen_tests;
mod gui_scale_settings_tests;
mod installed_geometry_tests;
mod item_pipeline_tests;
mod menu_caret_tests;
mod menu_navigation_tests;
mod publication_split_tests;
mod scene_policy_tests;
mod settings_storage_tests;

/// Builds an authoritative chat event for app input tests.
fn chat_event(message: &str) -> UiEvent {
    UiEvent::Text(TextEvent {
        category: TextCategory::MessageOnly,
        kind: TextKind::Chat,
        needs_translation: false,
        source: None,
        message: Arc::from(message),
        parameters: Arc::from([]),
        xuid: Arc::from(""),
        platform_chat_id: Arc::from(""),
        filtered_message: None,
    })
}
