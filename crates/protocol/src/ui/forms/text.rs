//! Display text retains its wire shape until the presentation resolves it.

use super::{MAX_UI_TEXT_BYTES, UnsupportedForm};
use serde_json::{Map, Value};
use std::{ops::Deref, sync::Arc};

/// A literal label or a structured text document resolved with live UI state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormText {
    Literal(Arc<str>),
    Raw(Arc<crate::RawTextDocument>),
}

impl Deref for FormText {
    type Target = str;
    /// Returns literal text for callers that do not have a resolver.
    fn deref(&self) -> &str {
        match self {
            Self::Literal(text) => text,
            Self::Raw(document) => document.literal_text(),
        }
    }
}

impl From<Arc<str>> for FormText {
    /// Retains an already bounded literal label.
    fn from(text: Arc<str>) -> Self {
        Self::Literal(text)
    }
}

impl From<&str> for FormText {
    /// Copies a literal label.
    fn from(text: &str) -> Self {
        Self::Literal(Arc::from(text))
    }
}

impl From<String> for FormText {
    /// Retains an owned literal label.
    fn from(text: String) -> Self {
        Self::Literal(Arc::from(text))
    }
}

/// Reads a display field; absent and null fields display no text.
pub(super) fn optional_text(
    object: &Map<String, Value>,
    key: &str,
) -> Result<FormText, UnsupportedForm> {
    match object.get(key) {
        None | Some(Value::Null) => Ok("".into()),
        Some(value) => text_value(value),
    }
}

/// Retains strings and parses structured text without presenting JSON as text.
pub(super) fn text_value(value: &Value) -> Result<FormText, UnsupportedForm> {
    if value.is_string() {
        return literal_value(value).map(FormText::from);
    }
    if !value.is_object() {
        return Err(UnsupportedForm::Controls);
    }
    let json = value.to_string();
    if json.len() > MAX_UI_TEXT_BYTES {
        return Err(UnsupportedForm::Limit);
    }
    crate::parse_raw_text(&json)
        .map(FormText::Raw)
        .map_err(|_| UnsupportedForm::Controls)
}

/// Reads an editable string or an image reference, where rawtext is not used.
pub(super) fn literal_value(value: &Value) -> Result<Arc<str>, UnsupportedForm> {
    let text = value.as_str().ok_or(UnsupportedForm::Controls)?;
    if text.len() > MAX_UI_TEXT_BYTES {
        return Err(UnsupportedForm::Limit);
    }
    Ok(Arc::from(text))
}

impl AsRef<str> for FormText {
    /// Returns the unresolved literal portion of this display text.
    fn as_ref(&self) -> &str {
        self
    }
}
