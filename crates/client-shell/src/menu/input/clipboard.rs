//! Clipboard adapters for menu fields and shared text editing.

use bevy::prelude::Resource;
use client_ui::ui_runtime::PlatformClipboard;
use ui::ChatClipboard;

/// Clipboard access used by menu fields and chat-style text editing.
#[derive(Resource)]
pub struct MenuClipboard(
    Box<dyn FnMut(usize) -> Option<String> + Send + Sync + 'static>,
    Box<dyn FnMut(String) + Send + Sync + 'static>,
);

impl MenuClipboard {
    /// Uses the supplied bounded reader and writer for menu clipboard access.
    pub fn with_access(
        reader: impl FnMut(usize) -> Option<String> + Send + Sync + 'static,
        writer: impl FnMut(String) + Send + Sync + 'static,
    ) -> Self {
        Self(Box::new(reader), Box::new(writer))
    }

    /// Passes the byte limit to the reader without changing the returned text.
    pub(super) fn read_text_bounded(&mut self, maximum_bytes: usize) -> Option<String> {
        (self.0)(maximum_bytes)
    }

    /// Sends the full text to the configured clipboard writer.
    pub fn write_text(&mut self, text: String) {
        (self.1)(text);
    }
}

impl Default for MenuClipboard {
    /// Uses the host clipboard and treats read failures as unavailable text.
    fn default() -> Self {
        let mut reader = PlatformClipboard;
        let mut writer = PlatformClipboard;
        Self::with_access(
            move |maximum_bytes| {
                reader
                    .read_text_bounded(maximum_bytes)
                    .ok()
                    .flatten()
                    .map(|text| text.to_string())
            },
            move |text| {
                let _ = writer.write_text(text);
            },
        )
    }
}

impl ChatClipboard for MenuClipboard {
    type Error = std::convert::Infallible;

    /// Rejects reader results that exceed the editor's byte limit.
    fn read_text_bounded(
        &mut self,
        maximum_bytes: usize,
    ) -> Result<Option<std::sync::Arc<str>>, Self::Error> {
        Ok(self
            .read_text_bounded(maximum_bytes)
            .filter(|text| text.len() <= maximum_bytes)
            .map(std::sync::Arc::from))
    }
}
