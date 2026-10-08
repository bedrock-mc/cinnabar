//! Single-line normalization and ellipsis results share the ordinary text cache.

use super::*;

impl TextLayoutCache {
    /// Retains normalization and ellipsis for an unwrapped request; empty means no text fits.
    pub fn single_line(
        &mut self,
        request: TextLayoutRequest<'_>,
        width_64: u32,
    ) -> Result<Arc<TextLayout>, TextError> {
        validate_request(TextLayoutRequest {
            text: "",
            ..request
        })?;
        let key = CacheKey::SingleLine(layout_key(request), width_64);
        let now = self.advance_clock()?;
        if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_used = now;
            return Ok(Arc::clone(&entry.layout));
        }
        let mut normalized = String::new();
        for word in request.text.split_whitespace() {
            let bytes = normalized.len() + word.len() + usize::from(!normalized.is_empty());
            if bytes > crate::UiLimits::MAX_TEXT_BYTES {
                return Err(TextError::TextBytesExceeded {
                    actual: bytes,
                    limit: crate::UiLimits::MAX_TEXT_BYTES,
                });
            }
            if !normalized.is_empty() {
                normalized.push(' ');
            }
            normalized.push_str(word);
        }
        let layout = self.layout(TextLayoutRequest {
            text: &normalized,
            ..request
        })?;
        let result = if layout.size_64()[0] <= width_64 {
            layout
        } else {
            let ends: Vec<_> = normalized.char_indices().map(|(at, _)| at).collect();
            let (mut low, mut high) = (0, ends.len());
            let mut best = self.layout(TextLayoutRequest {
                text: "",
                ..request
            })?;
            while low < high {
                let mid = low + (high - low) / 2;
                let shown = format!("{}…", normalized[..ends[mid]].trim_end());
                let layout = self.layout(TextLayoutRequest {
                    text: &shown,
                    ..request
                })?;
                if layout.size_64()[0] <= width_64 {
                    best = layout;
                    low = mid + 1;
                } else {
                    high = mid;
                }
            }
            best
        };
        let now = self.advance_clock()?;
        self.retain(key, result, now)
    }
}
