//! Shared text decoding and JSONC parsing for resource-pack documents.

use std::borrow::Cow;

use serde::de::DeserializeOwned;

/// Removes a leading UTF-8 BOM, preserves valid UTF-8, and maps otherwise invalid
/// bytes to their Latin-1 characters. Mixed encodings keep their valid UTF-8 spans.
#[must_use]
pub fn decode_text(bytes: &[u8]) -> Cow<'_, str> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(text);
    }
    let mut remaining = bytes;
    let mut text = String::with_capacity(bytes.len());
    loop {
        match std::str::from_utf8(remaining) {
            Ok(tail) => {
                text.push_str(tail);
                return Cow::Owned(text);
            }
            Err(error) => {
                let (valid, tail) = remaining.split_at(error.valid_up_to());
                text.push_str(std::str::from_utf8(valid).expect("validated UTF-8 prefix"));
                let length = error.error_len().unwrap_or(tail.len());
                text.extend(tail[..length].iter().map(|&byte| char::from(byte)));
                remaining = &tail[length..];
            }
        }
    }
}

/// Bytes a parsed document may need at most. A pack can inflate one entry to hundreds of MiB, and
/// a dense value tree costs far more than its text, so larger trees are refused before parsing
/// rather than aborting the process on allocation failure.
const MAX_PARSED_BYTES: usize = 1 << 30;

/// Worst-case parsed cost of one value: its tree node plus spare `Vec` capacity after doubling.
const PARSED_BYTES_PER_VALUE: usize = 2 * size_of::<serde_json::Value>();

/// One map entry's key and value, as a `BTreeMap` node stores them.
const MAP_ENTRY_BYTES: usize = size_of::<String>() + size_of::<serde_json::Value>();

/// Worst-case extra cost of one object key: entries in nodes that are only half full.
const PARSED_BYTES_PER_KEY: usize = 2 * MAP_ENTRY_BYTES;

/// Worst-case cost of one non-empty object: a `BTreeMap` leaf sized for 11 entries plus its header.
const PARSED_BYTES_PER_OBJECT: usize = 11 * MAP_ENTRY_BYTES + 64;

/// Whether a document's value tree fits `budget` bytes, bounded by the separators and openers
/// outside its strings and comments.
fn fits_parse_budget(bytes: &[u8], budget: usize) -> bool {
    let (mut values, mut keys, mut objects) = (1usize, 0usize, 0usize);
    let (mut in_string, mut escaped) = (false, false);
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        index += 1;
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match (byte, bytes.get(index)) {
            (b'"', _) => in_string = true,
            (b'/', Some(b'/')) => {
                while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                    index += 1;
                }
            }
            (b'/', Some(b'*')) => {
                index += 1;
                while index < bytes.len() && bytes.get(index..index + 2) != Some(b"*/") {
                    index += 1;
                }
                index += 2;
            }
            (b',' | b'[', _) => values += 1,
            (b':', _) => {
                values += 1;
                keys += 1;
            }
            (b'{', _) => {
                values += 1;
                objects += 1;
            }
            _ => {}
        }
    }
    // An empty object allocates no map node, and every non-empty one has a key.
    [
        (values, PARSED_BYTES_PER_VALUE),
        (keys, PARSED_BYTES_PER_KEY),
        (objects.min(keys), PARSED_BYTES_PER_OBJECT),
    ]
    .into_iter()
    .try_fold(bytes.len(), |total, (count, cost)| {
        total.checked_add(count.checked_mul(cost)?)
    })
    .is_some_and(|parsed| parsed <= budget)
}

/// Parses one complete pack document, accepting comments, a BOM and trailing
/// commas. The requested type keeps its own shape and duplicate-key validation.
/// A document whose value tree could exceed [`MAX_PARSED_BYTES`] is an error.
pub fn parse_jsonc<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    if !fits_parse_budget(bytes, MAX_PARSED_BYTES) {
        return Err(<serde_json::Error as serde::de::Error>::custom(
            "document is too large to parse",
        ));
    }
    parse_unbudgeted(bytes)
}

/// Checks a pack document's syntax as [`parse_jsonc`] reads it. No value tree is built, so the
/// tree budget does not apply.
pub fn validate_jsonc(bytes: &[u8]) -> Result<(), serde_json::Error> {
    parse_unbudgeted::<serde::de::IgnoredAny>(bytes).map(drop)
}

/// [`parse_jsonc`] without the tree budget.
fn parse_unbudgeted<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, serde_json::Error> {
    // Byte-oriented deserializers can accept invalid UTF-8 without decode_text's conversion.
    if let Ok(text) = std::str::from_utf8(bytes)
        && let Ok(value) = serde_json::from_str(text)
    {
        return Ok(value);
    }
    serde_json::from_slice(&prepare_jsonc(bytes)?)
}

/// Returns valid strict JSON bytes, or `None` for malformed syntax.
#[must_use]
pub fn normalize_jsonc(bytes: &[u8]) -> Option<Vec<u8>> {
    let normalized = prepare_jsonc(bytes).ok()?;
    serde_json::from_slice::<serde::de::IgnoredAny>(&normalized).ok()?;
    Some(normalized)
}

/// Converts pack text and replaces comments and trailing commas with whitespace.
fn prepare_jsonc(bytes: &[u8]) -> Result<Vec<u8>, serde_json::Error> {
    #[cfg(test)]
    tests::PREPARE_CALLS.with(|calls| calls.set(calls.get() + 1));
    let text = decode_text(bytes);
    let stripped = strip_comments(text.as_bytes()).ok_or_else(|| {
        <serde_json::Error as serde::de::Error>::custom("unterminated string or block comment")
    })?;
    Ok(strip_trailing_commas(&stripped))
}

/// Replaces comments outside strings with spaces while preserving line breaks.
fn strip_comments(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut output = bytes.to_vec();
    let mut index = 0;
    let mut in_string = false;
    let mut escaped = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            index += 1;
        } else if byte == b'"' {
            in_string = true;
            index += 1;
        } else if bytes.get(index..index + 2) == Some(b"//") {
            while index < bytes.len() && !matches!(bytes[index], b'\n' | b'\r') {
                output[index] = b' ';
                index += 1;
            }
        } else if bytes.get(index..index + 2) == Some(b"/*") {
            output[index..index + 2].fill(b' ');
            index += 2;
            loop {
                if index >= bytes.len() {
                    return None;
                }
                if bytes.get(index..index + 2) == Some(b"*/") {
                    output[index..index + 2].fill(b' ');
                    index += 2;
                    break;
                }
                if !matches!(bytes[index], b'\n' | b'\r') {
                    output[index] = b' ';
                }
                index += 1;
            }
        } else {
            index += 1;
        }
    }
    (!in_string).then_some(output)
}

/// Blanks a final comma after a value, leaving strings and missing values intact.
fn strip_trailing_commas(bytes: &[u8]) -> Vec<u8> {
    let mut output = bytes.to_vec();
    let mut in_string = false;
    let mut escaped = false;
    let mut previous = None;
    for (index, &byte) in bytes.iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if byte == b'"' {
            in_string = true;
        } else if byte == b','
            && previous.is_some_and(|byte| !matches!(byte, b'[' | b'{' | b',' | b':'))
            && bytes[index + 1..]
                .iter()
                .find(|next| !next.is_ascii_whitespace())
                .is_some_and(|next| matches!(next, b'}' | b']'))
        {
            output[index] = b' ';
        }
        if !byte.is_ascii_whitespace() {
            previous = Some(byte);
        }
    }
    output
}

#[cfg(test)]
mod tests;
