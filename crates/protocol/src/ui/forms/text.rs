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

/// Reads strings, boolean labels and structured text; other values display empty text.
pub(super) fn text_value(value: &Value) -> Result<FormText, UnsupportedForm> {
    match value {
        Value::String(_) => return literal_value(value).map(FormText::from),
        Value::Bool(value) => return Ok(if *value { "true" } else { "false" }.into()),
        Value::Object(_) => {}
        _ => return Ok("".into()),
    }
    let Some(rawtext) = value.get("rawtext") else {
        return Ok("".into());
    };
    let json = form_text_document(rawtext).to_string();
    if json.len() > MAX_UI_TEXT_BYTES {
        return Err(UnsupportedForm::Limit);
    }
    match crate::parse_raw_text(&json) {
        Ok(document) => Ok(FormText::Raw(document)),
        Err(crate::UiPacketError::InvalidRawText) => Ok("".into()),
        Err(_) => Err(UnsupportedForm::Limit),
    }
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

/// Keeps recognized components before the bounded display parser validates the document.
fn form_text_document(rawtext: &Value) -> Value {
    let parts = match rawtext {
        Value::Array(parts) => Value::Array(parts.iter().map(form_text_component).collect()),
        other => other.clone(),
    };
    serde_json::json!({"rawtext": parts})
}

/// Normalizes component precedence, nested text and translation arguments, ignoring metadata.
fn form_text_component(value: &Value) -> Value {
    let Some(object) = value.as_object() else {
        return Value::Null;
    };
    if ["text", "translate", "selector"].iter().any(|key| {
        object
            .get(*key)
            .is_some_and(|value| !value.is_null() && !value.is_string())
    }) || object
        .get("score")
        .is_some_and(|value| !value.is_null() && !value.is_object())
        || object
            .get("rawtext")
            .is_some_and(|value| !value.is_null() && !value.is_array())
    {
        return Value::Null;
    }
    let main = if let Some(key) = object.get("translate").and_then(Value::as_str) {
        let mut component = serde_json::json!({"translate": key});
        if let Some(with) = object.get("with").filter(|value| !value.is_null()) {
            let with = match with {
                Value::Array(parts) => Value::Array(
                    parts
                        .iter()
                        .filter(|part| part.is_string())
                        .cloned()
                        .collect(),
                ),
                Value::Object(parts) => {
                    form_text_document(parts.get("rawtext").unwrap_or(&Value::Null))
                }
                _ => return Value::Null,
            };
            component
                .as_object_mut()
                .unwrap()
                .insert("with".into(), with);
        }
        Some(component)
    } else if let Some(text) = object.get("text").and_then(Value::as_str) {
        Some(serde_json::json!({"text": text}))
    } else if let Some(selector) = object.get("selector").and_then(Value::as_str) {
        Some(serde_json::json!({"selector": selector}))
    } else if let Some(score) = object.get("score").and_then(Value::as_object) {
        let (Some(name), Some(objective)) = (
            score.get("name").and_then(Value::as_str),
            score.get("objective").and_then(Value::as_str),
        ) else {
            return Value::Null;
        };
        Some(serde_json::json!({"score":{"name":name,"objective":objective}}))
    } else {
        None
    };
    let nested = object
        .get("rawtext")
        .filter(|value| value.is_array())
        .map(form_text_document);
    match (main, nested) {
        (Some(main), Some(nested)) => serde_json::json!({"rawtext":[main,nested]}),
        (Some(component), None) | (None, Some(component)) => component,
        (None, None) => serde_json::json!({"text":""}),
    }
}
