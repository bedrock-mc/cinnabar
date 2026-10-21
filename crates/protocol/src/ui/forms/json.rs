//! Heap-backed construction and cleanup for already validated form JSON.

use serde_json::{Map, Value};

/// Owns a JSON tree whose destruction does not recurse through server containers.
pub(super) struct FormValue(pub(super) Value);

impl Drop for FormValue {
    /// Drains children before dropping their containers, including replaced object members.
    fn drop(&mut self) {
        let mut pending = match self.0.take() {
            Value::Array(items) => items,
            Value::Object(fields) => fields.into_values().collect(),
            _ => return,
        };
        while let Some(value) = pending.pop() {
            match value {
                Value::Array(items) => pending.extend(items),
                Value::Object(fields) => pending.extend(fields.into_values()),
                _ => {}
            }
        }
    }
}

struct Frame {
    value: FormValue,
    key: Option<String>,
}

/// Builds a validated document without recursive deserialization or recursive cleanup.
/// The header reader must validate the complete JSON syntax before calling this.
pub(super) fn parse_validated(json: &str) -> Result<FormValue, serde_json::Error> {
    let bytes = json.as_bytes();
    let mut frames: Vec<Frame> = Vec::new();
    let mut root = FormValue(Value::Null);
    let mut cursor = 0;
    while cursor < bytes.len() {
        let value = match bytes[cursor] {
            b'{' | b'[' => {
                let value = if bytes[cursor] == b'{' {
                    Value::Object(Map::new())
                } else {
                    Value::Array(Vec::new())
                };
                frames.push(Frame {
                    value: FormValue(value),
                    key: None,
                });
                cursor += 1;
                continue;
            }
            b'}' | b']' => {
                cursor += 1;
                frames.pop().expect("validated container end").value
            }
            b',' | b':' | b' ' | b'\n' | b'\r' | b'\t' => {
                cursor += 1;
                continue;
            }
            b'"' => {
                let end = string_end(bytes, cursor).expect("validated string end");
                let text: String = serde_json::from_slice(&bytes[cursor..end])?;
                cursor = end;
                if let Some(frame) = frames.last_mut()
                    && frame.value.0.is_object()
                    && frame.key.is_none()
                {
                    frame.key = Some(text);
                    continue;
                }
                FormValue(Value::String(text))
            }
            _ => {
                let start = cursor;
                while cursor < bytes.len()
                    && !matches!(
                        bytes[cursor],
                        b',' | b'}' | b']' | b' ' | b'\n' | b'\r' | b'\t'
                    )
                {
                    cursor += 1;
                }
                FormValue(serde_json::from_slice(&bytes[start..cursor])?)
            }
        };
        append(value, frames.last_mut(), &mut root);
    }
    Ok(root)
}

/// Attaches one completed value and safely discards any duplicate member it replaces.
fn append(mut value: FormValue, parent: Option<&mut Frame>, root: &mut FormValue) {
    match parent {
        Some(frame) => match &mut frame.value.0 {
            Value::Array(items) => items.push(value.0.take()),
            Value::Object(fields) => {
                if let Some(old) = fields.insert(
                    frame.key.take().expect("validated object key"),
                    value.0.take(),
                ) {
                    drop(FormValue(old));
                }
            }
            _ => unreachable!("container frame"),
        },
        None => *root = value,
    }
}

/// Finds the end of a quoted JSON string without treating escaped quotes as delimiters.
pub(super) fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut cursor = start + 1;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'"' => return Some(cursor + 1),
            b'\\' => cursor = cursor.checked_add(2)?,
            0x00..=0x1f => return None,
            _ => cursor += 1,
        }
    }
    None
}

/// Replaces line and block comments with whitespace, preserving quoted strings and token boundaries.
pub(super) fn comment_free(json: &str) -> Result<std::borrow::Cow<'_, str>, super::UiPacketError> {
    use std::borrow::Cow;
    let bytes = json.as_bytes();
    let mut output: Option<Vec<u8>> = None;
    let mut cursor = 0;
    let mut previous = None;
    let mut comment_after_string = false;
    while cursor < bytes.len() {
        let start = cursor;
        match bytes[cursor] {
            b'"' => {
                cursor = string_end(bytes, cursor).ok_or(super::UiPacketError::InvalidFormJson)?;
                previous = Some(b'"');
                comment_after_string = false;
                continue;
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => {
                cursor += 2;
                while cursor < bytes.len() && !matches!(bytes[cursor], b'\n' | b'\r') {
                    cursor += 1;
                }
            }
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => {
                cursor += 2;
                let length = bytes[cursor..]
                    .windows(2)
                    .position(|pair| pair == b"*/")
                    .ok_or(super::UiPacketError::InvalidFormJson)?;
                cursor += length + 2;
            }
            byte => {
                if !matches!(byte, b' ' | b'\n' | b'\r' | b'\t') {
                    // A member name must be followed directly by its colon, apart from whitespace.
                    if byte == b':' && comment_after_string {
                        return Err(super::UiPacketError::InvalidFormJson);
                    }
                    previous = Some(byte);
                    comment_after_string = false;
                }
                cursor += 1;
                continue;
            }
        }
        comment_after_string |= previous == Some(b'"');
        for byte in &mut output.get_or_insert_with(|| bytes.to_vec())[start..cursor] {
            if !matches!(byte, b'\n' | b'\r') {
                *byte = b' ';
            }
        }
    }
    Ok(match output {
        Some(output) => {
            Cow::Owned(String::from_utf8(output).expect("comments replaced by ASCII whitespace"))
        }
        None => Cow::Borrowed(json),
    })
}
