//! Generated message codecs use the existing positional channel records.

use std::{borrow::Cow, marker::PhantomData};

use crate::{Channel, Field, Value};

/// A typed, stable identifier. The marker keeps world, entity and item definitions distinct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Id<K>(Cow<'static, str>, PhantomData<K>);

impl<K> Id<K> {
    /// Creates a constant identifier and rejects malformed names at compile time in constants.
    pub const fn new(value: &'static str) -> Self {
        assert!(
            valid_id(value),
            "expected a namespaced lowercase identifier"
        );
        Self(Cow::Borrowed(value), PhantomData)
    }

    /// Returns the stable wire and persistence key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Marks a registered entity definition, rather than a live entity handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entity {}
/// Marks a registered world definition, rather than a live world instance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum World {}
/// Marks a registered item definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {}

pub type EntityId = Id<Entity>;
pub type WorldId = Id<World>;
pub type ItemId = Id<Item>;

/// Creates a typed ID; its type comes from the surrounding declaration.
#[macro_export]
macro_rules! id {
    ($value:literal) => {
        $crate::message::Id::new($value)
    };
}

/// Whether an ID has one nonempty namespace and one nonempty portable name.
pub const fn valid_id(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut colon = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b':' {
            if colon || i == 0 || i + 1 == bytes.len() {
                return false;
            }
            colon = true;
        } else if !(b.is_ascii_lowercase()
            || b.is_ascii_digit()
            || b == b'_'
            || b == b'-'
            || b == b'.')
        {
            return false;
        }
        i += 1;
    }
    colon
}

/// A Rust value with an unambiguous representation in a channel record.
pub trait WireValue: Sized {
    /// Encodes this value; the containing message checks its declared bounds.
    fn to_value(&self) -> Value;
    /// Decodes an already bounded value, still checking its wire type.
    fn from_value(value: Value) -> Result<Self, String>;
}

macro_rules! scalar {
    ($ty:ty, $variant:ident) => {
        impl WireValue for $ty {
            /// Encodes the scalar with its wire tag.
            fn to_value(&self) -> Value {
                Value::$variant(self.to_owned())
            }
            /// Requires the scalar's exact wire tag.
            fn from_value(value: Value) -> Result<Self, String> {
                match value {
                    Value::$variant(value) => Ok(value),
                    _ => Err("message field type mismatch".into()),
                }
            }
        }
    };
}
scalar!(bool, Bool);
scalar!(String, Text);

macro_rules! integer {
    ($($ty:ty),*) => {$(
        impl WireValue for $ty {
            /// Encodes an integer representable by the wire's signed 64-bit value.
            fn to_value(&self) -> Value { Value::Integer(i64::from(*self)) }
            /// Rejects values outside the Rust integer's range.
            fn from_value(value: Value) -> Result<Self, String> {
                match value {
                    Value::Integer(value) => Self::try_from(value).map_err(|_| "integer out of range".into()),
                    _ => Err("message field type mismatch".into()),
                }
            }
        }
    )*};
}
integer!(i8, i16, i32, i64, u8, u16, u32);

impl<K> WireValue for Id<K> {
    /// Encodes the stable namespaced key.
    fn to_value(&self) -> Value {
        Value::Text(self.as_str().to_owned())
    }
    /// Checks the namespaced key before giving it its domain type.
    fn from_value(value: Value) -> Result<Self, String> {
        match value {
            Value::Text(value) if valid_id(&value) => Ok(Self(Cow::Owned(value), PhantomData)),
            _ => Err("invalid typed identifier".into()),
        }
    }
}

impl<T: WireValue> WireValue for Vec<T> {
    /// Encodes the items in order.
    fn to_value(&self) -> Value {
        Value::List(self.iter().map(WireValue::to_value).collect())
    }
    /// Decodes each item using its exact wire type.
    fn from_value(value: Value) -> Result<Self, String> {
        match value {
            Value::List(values) => values.into_iter().map(T::from_value).collect(),
            _ => Err("message field type mismatch".into()),
        }
    }
}

/// Implemented by the build generator for each annotated message.
pub trait Message: Sized {
    const CHANNEL: Channel;
    /// Encodes fields in declaration order and validates all bounds.
    fn encode(&self) -> Result<Vec<Value>, String>;
    /// Requires exactly the declared fields and validates all bounds.
    fn decode(record: Vec<Value>) -> Result<Self, String>;
}

/// Validates a record before a generated encoder or decoder exposes it.
pub fn validate(channel: &Channel, record: &[Value]) -> Result<(), String> {
    if channel.fields.len() == record.len()
        && channel
            .fields
            .iter()
            .zip(record)
            .all(|(field, value)| admits(field, value))
    {
        Ok(())
    } else {
        Err(format!("{}: message fields or bounds mismatch", channel.id))
    }
}

/// Applies the existing channel field bounds recursively.
fn admits(field: &Field, value: &Value) -> bool {
    match (field, value) {
        (Field::Bool, Value::Bool(_)) => true,
        (Field::Integer { min, max }, Value::Integer(value)) => min <= value && value <= max,
        (Field::Text { max_bytes }, Value::Text(value)) => value.len() <= usize::from(*max_bytes),
        (Field::Choice { variants }, Value::Choice(value)) => value < variants,
        (Field::List { item, max_items }, Value::List(values)) => {
            values.len() <= usize::from(*max_items)
                && values.iter().all(|value| admits(item, value))
        }
        (Field::Record { fields }, Value::Record(values)) => {
            fields.len() == values.len()
                && fields
                    .iter()
                    .zip(values)
                    .all(|(field, value)| admits(field, value))
        }
        _ => false,
    }
}

/// A generated registry confirmation channel and the schema digest it must carry.
pub struct Registry {
    pub channel: Channel,
    pub fingerprint: &'static str,
}

/// Keeps authored callbacks inactive until the server confirms the generated registry.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RegistryState {
    #[default]
    Waiting,
    Active,
    Refused,
}

impl RegistryState {
    /// Accepts only one matching confirmation; a mismatch permanently refuses this instance.
    pub fn confirm(&mut self, registry: &Registry, record: &[Value]) -> Result<bool, String> {
        if *self == Self::Refused {
            return Err("mod registry already refused".into());
        }
        if record != [Value::Text(registry.fingerprint.to_owned())] {
            *self = Self::Refused;
            return Err("mod registry fingerprint mismatch; rebuild the client and server definitions together".into());
        }
        let first = *self == Self::Waiting;
        *self = Self::Active;
        Ok(first)
    }
}

/// A generated package asset; consumers use its indexed path or compile-checked bytes.
#[derive(Clone, Copy, Debug)]
pub struct Asset {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Direction;

    #[test]
    fn mismatch_never_activates_authored_callbacks() {
        let registry = Registry {
            channel: Channel {
                id: "space.registry",
                schema: 1,
                direction: Direction::ToClient,
                fields: &[Field::Text { max_bytes: 64 }],
            },
            fingerprint: "one",
        };
        let mut state = RegistryState::default();
        assert!(
            state
                .confirm(&registry, &[Value::Text("two".into())])
                .is_err()
        );
        assert_eq!(state, RegistryState::Refused);
        assert!(
            state
                .confirm(&registry, &[Value::Text("one".into())])
                .is_err()
        );
        let mut state = RegistryState::default();
        assert_eq!(
            state.confirm(&registry, &[Value::Text("one".into())]),
            Ok(true)
        );
        assert_eq!(
            state.confirm(&registry, &[Value::Text("one".into())]),
            Ok(false)
        );
    }

    #[test]
    fn typed_ids_validate_wire_names() {
        const SHIP: EntityId = crate::id!("space:ship");
        assert_eq!(EntityId::from_value(SHIP.to_value()).unwrap(), SHIP);
        assert!(WorldId::from_value(Value::Text("world".into())).is_err());
    }
}
