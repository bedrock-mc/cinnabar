//! Bounded marker output through the browser's tracing subscriber, without native log threads.
use std::io::{self, Write};

const MAX_MARKER_BYTES: usize = 16 * 1024;

/// A bounded marker assembled until flush or drop, then forwarded to browser logging.
pub struct Writer {
    bytes: Vec<u8>,
    error: bool,
}

/// Opens a normal diagnostic marker at the browser logging edge.
pub fn stdout() -> Writer {
    Writer {
        bytes: Vec::new(),
        error: false,
    }
}

/// Opens an error diagnostic marker at the browser logging edge.
pub fn stderr() -> Writer {
    Writer {
        bytes: Vec::new(),
        error: true,
    }
}

impl Write for Writer {
    /// Accepts the whole write while retaining only the marker's bounded prefix.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let keep = bytes
            .len()
            .min(MAX_MARKER_BYTES.saturating_sub(self.bytes.len()));
        self.bytes.extend_from_slice(&bytes[..keep]);
        Ok(bytes.len())
    }
    /// Publishes the retained marker once through the platform's tracing subscriber.
    fn flush(&mut self) -> io::Result<()> {
        if self.bytes.is_empty() {
            return Ok(());
        }
        let text = String::from_utf8_lossy(&self.bytes);
        if self.error {
            tracing::warn!("{}", text.trim_end());
        } else {
            tracing::info!("{}", text.trim_end());
        }
        self.bytes.clear();
        Ok(())
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}
