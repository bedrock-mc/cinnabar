//! Bounded clipboard adapter for chat paste: arboard on native targets. Browsers only expose an
//! asynchronous, permission-gated clipboard, so there paste finds no text and copy is unavailable.

use std::sync::Arc;

use ui::ChatClipboard;

#[derive(Default)]
pub struct PlatformClipboard;

#[derive(Debug, thiserror::Error)]
pub enum PlatformClipboardError {
    #[cfg(not(target_arch = "wasm32"))]
    #[error("platform clipboard failed: {0}")]
    Platform(#[from] arboard::Error),
    /// Browsers expose the clipboard only through asynchronous, permission-gated calls.
    #[cfg(target_arch = "wasm32")]
    #[error("platform clipboard is unavailable in the browser")]
    Unavailable,
    #[error("clipboard text exceeds the {maximum}-byte chat insertion bound")]
    TooLong { maximum: usize },
}

impl ChatClipboard for PlatformClipboard {
    type Error = PlatformClipboardError;

    #[cfg(not(target_arch = "wasm32"))]
    fn read_text_bounded(&mut self, maximum_bytes: usize) -> Result<Option<Arc<str>>, Self::Error> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = maximum_bytes;
            Ok(None)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let text = arboard::Clipboard::new()?.get_text()?;
            if text.len() > maximum_bytes {
                return Err(PlatformClipboardError::TooLong {
                    maximum: maximum_bytes,
                });
            }
            Ok(Some(Arc::from(text)))
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn read_text_bounded(
        &mut self,
        _maximum_bytes: usize,
    ) -> Result<Option<Arc<str>>, Self::Error> {
        Err(PlatformClipboardError::Unavailable)
    }
}

impl PlatformClipboard {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn write_text(&mut self, text: String) -> Result<(), PlatformClipboardError> {
        #[cfg(target_arch = "wasm32")]
        {
            let _ = text;
            Err(PlatformClipboardError::Unavailable)
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            arboard::Clipboard::new()?.set_text(text)?;
            Ok(())
        }
    }

    #[cfg(target_arch = "wasm32")]
    pub fn write_text(&mut self, _text: String) -> Result<(), PlatformClipboardError> {
        Err(PlatformClipboardError::Unavailable)
    }
}
