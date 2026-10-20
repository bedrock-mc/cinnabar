//! UTF-8 appends for output limits and the shared resolution allowance.

use super::{MAX_RAW_TEXT_EXPANSION_BYTES, ResolvedRawText};

/// Appends within both budgets, charging every retained byte to the shared allowance.
/// Returns whether this text lost a suffix.
pub(super) fn push_bounded(
    resolved: &mut ResolvedRawText,
    text: &str,
    max_bytes: usize,
    remaining: &mut usize,
) -> bool {
    let before = resolved.text.len();
    let limit = max_bytes.min(before.saturating_add(*remaining));
    let omitted = append_prefix(&mut resolved.text, text, limit);
    *remaining -= resolved.text.len() - before;
    resolved.truncated |= omitted;
    omitted
}

/// Appends complete UTF-8 scalars without reserving beyond the string's budget.
/// Returns whether any suffix was omitted.
fn append_prefix(output: &mut String, text: &str, max_bytes: usize) -> bool {
    let end = text.floor_char_boundary(text.len().min(max_bytes.saturating_sub(output.len())));
    let required = output.len() + end;
    if required > output.capacity() {
        let target = required.max(output.capacity().saturating_mul(2).max(16).min(max_bytes));
        output.reserve_exact(target - output.len());
    }
    output.push_str(&text[..end]);
    end < text.len()
}

pub(super) struct TranslationPrefix {
    pub(super) text: String,
    pub(super) sealed: bool,
    pub(super) max_bytes: usize,
}

impl TranslationPrefix {
    /// Reserves a prefix within the caller's text budget.
    pub(super) fn new(template_bytes: usize, max_bytes: usize) -> Self {
        let max_bytes = max_bytes.min(MAX_RAW_TEXT_EXPANSION_BYTES);
        Self {
            text: String::with_capacity(template_bytes.min(max_bytes)),
            sealed: false,
            max_bytes,
        }
    }

    /// Appends a scalar unless an earlier suffix was omitted.
    pub(super) fn push(&mut self, character: char) {
        let mut bytes = [0; 4];
        self.push_str(character.encode_utf8(&mut bytes));
    }

    /// Appends a prefix and seals this output at the first omitted scalar.
    pub(super) fn push_str(&mut self, text: &str) {
        if !self.sealed {
            self.sealed = append_prefix(&mut self.text, text, self.max_bytes);
        }
    }
}
