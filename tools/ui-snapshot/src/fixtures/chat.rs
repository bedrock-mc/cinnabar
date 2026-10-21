//! Anonymous chat histories, drafts and completion lists.

use std::sync::Arc;

use launcher::menu::settings_options::SettingId;

use super::Fixture;

/// Exercises the native chat collection with an unfinished mention.
pub(super) fn chat_mentions() -> Fixture {
    Fixture::Chat {
        server_hover: None,
        draft: "@",
        suggestions: ["@here", "@Alex", "@Steve"].map(Arc::from).into(),
        messages: Default::default(),
        options: Default::default(),
    }
}

/// Exercises the native command completion collection with target candidates.
pub(super) fn chat_command_targets() -> Fixture {
    Fixture::Chat {
        server_hover: None,
        draft: "/give @",
        suggestions: ["@a", "@e", "@p", "@r", "@s"].map(Arc::from).into(),
        messages: Default::default(),
        options: Default::default(),
    }
}

/// Shows readable history and a draft under a selected background-opacity setting.
pub(super) fn chat_background(percent: i32) -> Fixture {
    use launcher::menu::settings_options::{SettingId, SettingsOptions};
    let mut options = SettingsOptions::default();
    options.set(SettingId::ChatBackgroundOpacity.index(), percent);
    let messages = [
        format!("Chat background opacity: {percent}%"),
        "The chat text stays visible.".into(),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, message)| ui::ChatMessage {
        fifo_sequence: index as u64 + 1,
        received_millis: 0,
        kind: ui::ChatMessageKind::System,
        source: None,
        message: message.into(),
        parameters: Arc::from([]),
    })
    .collect();
    Fixture::Chat {
        server_hover: None,
        draft: "Typing a message",
        suggestions: Vec::new(),
        messages,
        options: Arc::new(options),
    }
}

/// Shows the command input and suggestions with chat backgrounds set to zero percent.
pub(super) fn chat_command_background() -> Fixture {
    chat_completion("/", &["/give", "/help", "/say"])
}

/// Shows equal-width history backgrounds and a message that wraps inside them.
pub(super) fn chat_history_width() -> Fixture {
    let mut fixture = chat_background(50);
    if let Fixture::Chat { messages, .. } = &mut fixture {
        messages.push_back(ui::ChatMessage {
            fifo_sequence: messages.len() as u64 + 1,
            received_millis: 0,
            kind: ui::ChatMessageKind::System,
            source: None,
            message: "A longer chat message wraps inside the history box while every visible line keeps the same background width.".into(),
            parameters: Arc::from([]),
        });
    }
    fixture
}

/// Shows completions after an anonymous draft with transparent message backgrounds.
pub(super) fn chat_completion(draft: &'static str, suggestions: &[&str]) -> Fixture {
    use launcher::menu::settings_options::SettingsOptions;

    let mut options = SettingsOptions::default();
    options.set(SettingId::ChatBackgroundOpacity.index(), 0);
    Fixture::Chat {
        server_hover: None,
        draft,
        suggestions: suggestions.iter().map(|text| Arc::from(*text)).collect(),
        messages: Default::default(),
        options: Arc::new(options),
    }
}

/// Keeps encoded message metadata out of history and reveals it through a hover renderer.
#[cfg(feature = "test-support")]
pub(super) fn chat_server_content(hover: bool) -> Fixture {
    Fixture::Chat {
        draft: "",
        suggestions: Vec::new(),
        messages: [ui::ChatMessage {
            fifo_sequence: 1,
            received_millis: 0,
            kind: ui::ChatMessageKind::System,
            source: None,
            message: ui_screens::test_support::hover_chat::message().into(),
            parameters: Arc::from([]),
        }]
        .into(),
        options: Default::default(),
        server_hover: Some(hover),
    }
}
