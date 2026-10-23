//! Data store values and the property paths that address their nested members.

use std::collections::BTreeMap;
use std::sync::Arc;

use bedrock_protocol_1_26_51::types::DynamicValue;
use bedrock_protocol_1_26_51::wire;
use thiserror::Error;

/// One data store property value: a JSON-like tree carried as a wire `DynamicValue`.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum DataValue {
    #[default]
    None,
    Bool(bool),
    Int(i64),
    Number(f64),
    String(Arc<str>),
    List(Vec<DataValue>),
    Map(BTreeMap<Arc<str>, DataValue>),
}

/// A single value a path update or client write carries.
#[derive(Debug, Clone, PartialEq)]
pub enum DataPrimitive {
    Number(f64),
    Bool(bool),
    String(Arc<str>),
}

impl From<DataPrimitive> for DataValue {
    fn from(value: DataPrimitive) -> Self {
        match value {
            DataPrimitive::Number(value) => Self::Number(value),
            DataPrimitive::Bool(value) => Self::Bool(value),
            DataPrimitive::String(value) => Self::String(value),
        }
    }
}

impl DataValue {
    /// The member `key` of a map, if this is a map holding it.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Self> {
        match self {
            Self::Map(entries) => entries.get(key),
            _ => None,
        }
    }

    /// The element at `index`: a list position, or the decimal key of an array-shaped map.
    ///
    /// Server scripts publish arrays as maps keyed `"0"`, `"1"`, ... next to a `length` member.
    #[must_use]
    pub fn element(&self, index: usize) -> Option<&Self> {
        match self {
            Self::List(items) => items.get(index),
            Self::Map(entries) => entries.get(index.to_string().as_str()),
            _ => None,
        }
    }

    /// The stored elements in index order with their indices: a list's items, or the decimal-keyed
    /// members of an array-shaped map below its `length`. Missing indices are skipped, so the
    /// work follows the stored entries rather than a claimed length.
    #[must_use]
    pub fn elements(&self) -> Vec<(usize, &Self)> {
        match self {
            Self::List(items) => items.iter().enumerate().collect(),
            Self::Map(entries) => {
                let length = self.array_len();
                let mut elements: Vec<_> = entries
                    .iter()
                    .filter_map(|(key, value)| {
                        let index = key.parse::<usize>().ok()?;
                        (index < length && index.to_string() == key.as_ref())
                            .then_some((index, value))
                    })
                    .collect();
                elements.sort_unstable_by_key(|(index, _)| *index);
                elements
            }
            _ => Vec::new(),
        }
    }

    /// The element count of a list, or the `length` member of an array-shaped map.
    #[must_use]
    pub fn array_len(&self) -> usize {
        match self {
            Self::List(items) => items.len(),
            Self::Map(_) => self
                .get("length")
                .and_then(Self::as_number)
                .filter(|length| length.is_finite() && *length > 0.0)
                .map_or(0, |length| length as usize),
            _ => 0,
        }
    }

    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(value) => Some(*value),
            _ => None,
        }
    }

    /// A number from either numeric representation.
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Int(value) => Some(*value as f64),
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    /// Writes `value` at `path`, replacing the leaf that is there.
    ///
    /// An empty path replaces the whole value. A container leaf is never replaced by a
    /// primitive, so a mistyped update cannot erase a nested object.
    pub fn set_path(&mut self, path: &str, value: DataPrimitive) -> Result<(), PathError> {
        let segments = parse_path(path)?;
        let mut target = self;
        for segment in &segments {
            target = match (target, segment) {
                (Self::Map(entries), Segment::Key(key)) => {
                    entries.get_mut(*key).ok_or(PathError::MissingMember)?
                }
                (Self::Map(entries), Segment::Index(index)) => entries
                    .get_mut(index.to_string().as_str())
                    .ok_or(PathError::MissingMember)?,
                (Self::List(items), Segment::Index(index)) => {
                    items.get_mut(*index).ok_or(PathError::OutOfBounds)?
                }
                _ => return Err(PathError::WrongContainer),
            };
        }
        if matches!(target, Self::Map(_) | Self::List(_)) {
            return Err(PathError::WrongContainer);
        }
        *target = value.into();
        Ok(())
    }

    /// The value at `path`, if every segment resolves.
    #[must_use]
    pub fn at_path(&self, path: &str) -> Option<&Self> {
        let mut target = self;
        for segment in parse_path(path).ok()? {
            target = match segment {
                Segment::Key(key) => target.get(key)?,
                Segment::Index(index) => target.element(index)?,
            };
        }
        Some(target)
    }
}

/// Why a property path could not be followed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PathError {
    #[error("path syntax is malformed")]
    Syntax,
    #[error("path names a member the value does not have")]
    MissingMember,
    #[error("path index is outside the list")]
    OutOfBounds,
    #[error("path crosses a value of the wrong kind")]
    WrongContainer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Segment<'a> {
    Key(&'a str),
    Index(usize),
}

/// Splits `a.b[2].c` into member keys (separated by `.`) and bracketed element indices.
fn parse_path(path: &str) -> Result<Vec<Segment<'_>>, PathError> {
    let mut segments = Vec::new();
    let mut rest = path;
    let mut expect_key = true;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('[') {
            let close = after.find(']').ok_or(PathError::Syntax)?;
            let index = after[..close]
                .parse::<usize>()
                .map_err(|_| PathError::Syntax)?;
            segments.push(Segment::Index(index));
            rest = &after[close + 1..];
            expect_key = false;
            continue;
        }
        if !expect_key {
            rest = rest.strip_prefix('.').ok_or(PathError::Syntax)?;
        }
        let end = rest.find(['.', '[', ']']).unwrap_or(rest.len());
        if end == 0 {
            return Err(PathError::Syntax);
        }
        segments.push(Segment::Key(&rest[..end]));
        rest = &rest[end..];
        if rest.starts_with(']') {
            return Err(PathError::Syntax);
        }
        expect_key = false;
    }
    Ok(segments)
}

/// Converts a wire string, replacing invalid UTF-8 rather than rejecting the value.
pub(super) fn text(value: wire::Str) -> Arc<str> {
    value.to_string_lossy().into()
}

pub(super) fn from_wire(value: DynamicValue) -> DataValue {
    match value {
        DynamicValue::None => DataValue::None,
        DynamicValue::Bool(value) => DataValue::Bool(value),
        DynamicValue::Int64(value) => DataValue::Int(value.0),
        DynamicValue::Double(value) => DataValue::Number(value.0),
        DynamicValue::String(value) => DataValue::String(text(value)),
        DynamicValue::List(items) => DataValue::List(items.into_iter().map(from_wire).collect()),
        DynamicValue::Map(entries) => DataValue::Map(
            entries
                .into_iter()
                .map(|(key, value)| (text(key), from_wire(value)))
                .collect(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, DataValue)]) -> DataValue {
        DataValue::Map(
            entries
                .iter()
                .map(|(key, value)| (Arc::from(*key), value.clone()))
                .collect(),
        )
    }

    #[test]
    fn bracket_indices_address_array_shaped_maps_and_lists() {
        let mut value = map(&[(
            "layout",
            map(&[
                ("length", DataValue::Int(1)),
                ("0", map(&[("text", DataValue::String("old".into()))])),
            ]),
        )]);
        value
            .set_path("layout[0].text", DataPrimitive::String("new".into()))
            .unwrap();
        assert_eq!(
            value.at_path("layout[0].text"),
            Some(&DataValue::String("new".into()))
        );
        assert_eq!(value.get("layout").unwrap().array_len(), 1);

        let mut list = map(&[("items", DataValue::List(vec![DataValue::Bool(false)]))]);
        list.set_path("items[0]", DataPrimitive::Bool(true))
            .unwrap();
        assert_eq!(list.at_path("items[0]"), Some(&DataValue::Bool(true)));
        assert_eq!(
            list.set_path("items[1]", DataPrimitive::Bool(true)),
            Err(PathError::OutOfBounds)
        );
    }

    #[test]
    fn malformed_paths_and_container_leaves_are_rejected() {
        let mut value = map(&[(
            "layout",
            map(&[("0", map(&[("onClick", DataValue::Number(0.0))]))]),
        )]);
        for path in [
            "layout..0",
            "layout[0",
            "layout]",
            "[x]",
            ".layout",
            "layout[0]x",
        ] {
            assert_eq!(
                value.set_path(path, DataPrimitive::Number(1.0)),
                Err(PathError::Syntax),
                "{path}"
            );
        }
        assert_eq!(
            value.set_path("layout[0]", DataPrimitive::Number(1.0)),
            Err(PathError::WrongContainer)
        );
        value
            .set_path("layout.0.onClick", DataPrimitive::Number(1.0))
            .unwrap();
        assert_eq!(
            value.at_path("layout[0].onClick"),
            Some(&DataValue::Number(1.0))
        );
    }
}
