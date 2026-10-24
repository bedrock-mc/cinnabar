use std::path::Path;

use assets::AssetError;
use serde::{Deserialize, Deserializer, de};
use serde_json::{Map, Value};

/// Parses a pack object while rejecting duplicate root members.
pub fn parse_unique_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    bedrock_packs::jsonc::parse_jsonc::<UniqueRootValue>(bytes)
        .map(|root| root.0)
        .map_err(|source| AssetError::Json {
            path: path.to_path_buf(),
            source,
        })
}

/// Parses a pack document while rejecting duplicate members at every depth.
pub fn parse_fully_unique_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    bedrock_packs::jsonc::parse_jsonc::<UniqueNestedValue>(bytes)
        .map(|root| root.0)
        .map_err(|source| AssetError::Json {
            path: path.to_path_buf(),
            source,
        })
}

/// Parses payloads whose nested maps have runtime meaning and must be unambiguous.
pub fn parse_semantic_json(path: &Path, bytes: &[u8]) -> Result<Value, AssetError> {
    parse_fully_unique_json(path, bytes)
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

/// Rejects unsupported root fields and missing required family maps.
pub fn validate_root_fields(
    value: &Value,
    path: &Path,
    allowed: &[&str],
    required: &[&str],
) -> Result<(), AssetError> {
    let object = value
        .as_object()
        .ok_or_else(|| AssetError::InvalidCompiledAssets {
            detail: format!("JSON root must be an object in {}", path.display()).into(),
        })?;
    if object
        .keys()
        .any(|field| !allowed.contains(&field.as_str()))
        || required.iter().any(|field| !object.contains_key(*field))
    {
        return Err(AssetError::InvalidCompiledAssets {
            detail: format!(
                "unknown or missing entity family root field in {}",
                path.display()
            )
            .into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_semantic_json, parse_unique_json};
    use std::path::Path;

    #[test]
    fn material_reader_accepts_comments_and_legacy_text() {
        let path = Path::new("example/materials/ui.material");
        let source = b"\xef\xbb\xbf{\"materials\":{/* note */\"example:ui\":{\"label\":\"\xa7rExample\",}, // tail\n}}";
        let value = parse_semantic_json(path, source).unwrap();
        assert_eq!(value["materials"]["example:ui"]["label"], "§rExample");
    }

    #[test]
    fn shared_decoder_keeps_duplicate_key_validation_and_file_context() {
        let path = Path::new("example/entity/test.json");
        let duplicate_root = br#"{/* c */"a":1,"a":2,}"#;
        assert!(parse_unique_json(path, duplicate_root).is_err());
        let duplicate_nested = br#"{"a":{"b":1,"b":2,},}"#;
        assert!(parse_semantic_json(path, duplicate_nested).is_err());
        let error = parse_semantic_json(path, b"{/* open").unwrap_err();
        assert!(error.to_string().contains("example/entity/test.json"));
    }
}
