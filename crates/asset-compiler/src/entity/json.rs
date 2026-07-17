use std::path::Path;

use assets::AssetError;
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value};

use super::invalid;

pub(super) fn parse_unique_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let uncommented = strip_json_comments(bytes)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&uncommented);
    let UniqueRootValue(value) =
        UniqueRootValue::deserialize(&mut deserializer).map_err(|source| AssetError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    deserializer.end().map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(value)
}

pub(super) fn parse_fully_unique_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    let bytes = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let uncommented = strip_json_comments(bytes)?;
    let mut deserializer = serde_json::Deserializer::from_slice(&uncommented);
    let UniqueNestedValue(value) =
        UniqueNestedValue::deserialize(&mut deserializer).map_err(|source| AssetError::Json {
            path: path.to_path_buf(),
            source,
        })?;
    deserializer.end().map_err(|source| AssetError::Json {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(value)
}

/// Parses payloads whose nested maps now have runtime meaning. Once a family
/// is compiled, duplicate nested keys are ambiguous rather than opaque.
pub(super) fn parse_semantic_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    parse_fully_unique_json(path, bytes)
}

fn strip_json_comments(bytes: &[u8]) -> Result<Vec<u8>, AssetError> {
    #[derive(Clone, Copy)]
    enum State {
        Normal,
        String,
        LineComment,
        BlockComment,
    }

    let mut output = bytes.to_vec();
    let mut state = State::Normal;
    let mut index = 0;
    while index < bytes.len() {
        match state {
            State::Normal => match bytes[index] {
                b'"' => {
                    state = State::String;
                    index += 1;
                }
                b'/' if bytes.get(index + 1) == Some(&b'/') => {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    state = State::LineComment;
                    index += 2;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    state = State::BlockComment;
                    index += 2;
                }
                _ => index += 1,
            },
            State::String => match bytes[index] {
                b'\\' => index = (index + 2).min(bytes.len()),
                b'"' => {
                    state = State::Normal;
                    index += 1;
                }
                _ => index += 1,
            },
            State::LineComment => match bytes[index] {
                b'\n' | b'\r' => {
                    state = State::Normal;
                    index += 1;
                }
                _ => {
                    output[index] = b' ';
                    index += 1;
                }
            },
            State::BlockComment => {
                if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    output[index] = b' ';
                    output[index + 1] = b' ';
                    state = State::Normal;
                    index += 2;
                } else {
                    if bytes[index] != b'\n' && bytes[index] != b'\r' {
                        output[index] = b' ';
                    }
                    index += 1;
                }
            }
        }
    }
    if matches!(state, State::BlockComment) {
        return Err(invalid("unterminated block comment in entity JSON source"));
    }
    Ok(output)
}

struct UniqueRootValue(Value);

impl<'de> Deserialize<'de> for UniqueRootValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(UniqueRootValueVisitor)
    }
}

struct UniqueRootValueVisitor;

impl<'de> de::Visitor<'de> for UniqueRootValueVisitor {
    type Value = UniqueRootValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON object without duplicate root keys")
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some((key, value)) = map.next_entry::<String, Value>()? {
            if values.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("duplicate JSON key `{key}`")));
            }
        }
        Ok(UniqueRootValue(Value::Object(values)))
    }
}

struct UniqueNestedValue(Value);

impl<'de> Deserialize<'de> for UniqueNestedValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(UniqueNestedValueVisitor)
    }
}

struct UniqueNestedValueVisitor;

impl<'de> de::Visitor<'de> for UniqueNestedValueVisitor {
    type Value = UniqueNestedValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("JSON without duplicate object keys")
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Null))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Null))
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(Value::Number)
            .map(UniqueNestedValue)
            .ok_or_else(|| de::Error::custom("invalid non-finite JSON number"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::String(value.to_owned())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueNestedValue(Value::String(value)))
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: de::SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(UniqueNestedValue(value)) = sequence.next_element()? {
            values.push(value);
        }
        Ok(UniqueNestedValue(Value::Array(values)))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: de::MapAccess<'de>,
    {
        let mut values = Map::new();
        while let Some(key) = map.next_key::<String>()? {
            let UniqueNestedValue(value) = map.next_value()?;
            if values.insert(key.clone(), value).is_some() {
                return Err(de::Error::custom(format!("duplicate JSON key `{key}`")));
            }
        }
        Ok(UniqueNestedValue(Value::Object(values)))
    }
}
