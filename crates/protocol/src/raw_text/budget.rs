//! UTF-8 appends for output limits and the shared resolution allowance.

use std::cell::Cell;

use super::{RawTextResolver, ResolvedRawText, format_translation_chars_into};

/// Sixteen packet sizes leave ample display text while bounding a whole form's expansion.
pub const MAX_FORM_TEXT_EXPANSION_BYTES: usize = 16 * crate::ui::MAX_FORM_JSON_BYTES;

/// One form build's allowance for literals, substitutions and all nested arguments.
/// Shared references let every presentation callback charge the same counter.
pub struct FormTextBudget {
    remaining: Cell<usize>,
}

impl Default for FormTextBudget {
    fn default() -> Self {
        Self {
            remaining: Cell::new(MAX_FORM_TEXT_EXPANSION_BYTES),
        }
    }
}

impl FormTextBudget {
    /// Resolves one label using this form's remaining allowance, preserving a UTF-8 prefix.
    pub fn resolve(&self, text: &crate::ui::FormText, resolver: &RawTextResolver<'_>) -> String {
        match text {
            crate::ui::FormText::Literal(text) => self.copy(text),
            crate::ui::FormText::Raw(document) => {
                self.spend(|remaining| document.resolve_with_budget(resolver, remaining).text)
            }
        }
    }

    /// Copies literal or already resolved text, charging only its retained prefix.
    pub fn copy(&self, text: &str) -> String {
        self.spend(|remaining| {
            let mut output = String::new();
            append_prefix(&mut output, text, *remaining);
            *remaining -= output.len();
            output
        })
    }

    /// Formats a control label within the same allowance as its resolved arguments.
    pub fn format_translation(&self, template: &str, arguments: &[String]) -> String {
        self.spend(|remaining| {
            let prefix = TranslationPrefix::new(template.len(), *remaining);
            let output = format_translation_chars_into(
                template.chars(),
                |index| arguments.get(index).map(|text| (text.as_str(), false)),
                prefix,
            )
            .text;
            *remaining -= output.len();
            output
        })
    }

    /// Lends the single counter to eager resolution without resetting it between labels.
    fn spend<T>(&self, build: impl FnOnce(&mut usize) -> T) -> T {
        let mut remaining = self.remaining.get();
        let output = build(&mut remaining);
        self.remaining.set(remaining);
        output
    }
}

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
