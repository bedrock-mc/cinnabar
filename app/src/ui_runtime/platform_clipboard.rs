//! Arboard-backed bounded clipboard adapter for chat paste.

use std::sync::Arc;

use ui::ChatClipboard;

#[derive(Default)]
pub(crate) struct PlatformClipboard;

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlatformClipboardError {
    #[error("platform clipboard failed: {0}")]
    Platform(#[from] arboard::Error),
    #[error("clipboard text exceeds the {maximum}-byte chat insertion bound")]
    TooLong { maximum: usize },
}

impl ChatClipboard for PlatformClipboard {
    type Error = PlatformClipboardError;

    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Result<Option<Arc<str>>, Self::Error> {
        let text = arboard::Clipboard::new()?.get_text()?;
        if text.len() > maximum_bytes {
            return Err(PlatformClipboardError::TooLong {
                maximum: maximum_bytes,
            });
        }
        Ok(Some(Arc::from(text)))
    }
}
