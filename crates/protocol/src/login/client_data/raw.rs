//! Bounded untrusted JSON objects whose opaque value lexemes survive native Login signing.

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{MapAccess, Visitor},
    ser::SerializeMap,
};
use serde_json::{Value, value::RawValue};
use std::{fmt, io};

mod schema_generated;
mod semantic;
mod types;
pub use semantic::{ClientDataRefusal, ValidatedClientData};

/// Untrusted relay metadata, retaining value spelling and entry order without granting an identity.
/// Parsing bounds the object and checks every known field's generated Go JSON type.
/// Full title metadata validation is a separate gate.
#[derive(Clone, Default)]
pub struct RawClientData {
    entries: Vec<(String, Box<RawValue>)>,
}

impl fmt::Debug for RawClientData {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RawClientData")
            .finish_non_exhaustive()
    }
}

impl PartialEq for RawClientData {
    fn eq(&self, other: &Self) -> bool {
        self.entries.len() == other.entries.len()
            && self.entries.iter().zip(&other.entries).all(
                |((key, value), (other_key, other_value))| {
                    key == other_key && value.get() == other_value.get()
                },
            )
    }
}

impl RawClientData {
    /// Preserves a bounded object after checking every occurrence of each known JSON field type.
    /// This does not validate skin, name, locale or account-owned identity.
    ///
    /// # Errors
    /// Rejects oversized or malformed objects and any known-field JSON type mismatch.
    pub fn from_raw(raw: &RawValue) -> io::Result<Self> {
        if raw.get().len() > bedrock_net::BatchLimits::default().max_decoded_bytes {
            return Err(invalid());
        }
        let entries = serde_json::from_str::<Entries>(raw.get()).map_err(|_| invalid())?;
        types::check(&entries)?;
        Ok(Self { entries: entries.0 })
    }

    /// Adopts locally constructed claims through the same raw object owner.
    #[cfg(test)]
    pub(crate) fn from_value(value: &Value) -> io::Result<Self> {
        Self::from_raw(&serde_json::value::to_raw_value(value).map_err(|_| invalid())?)
    }

    /// Consumes the typed raw owner at the title metadata boundary; no unchecked success exists.
    pub(super) fn validate_relay(
        self,
        profile: &super::NativeDeviceProfile,
        protocol: i32,
    ) -> Result<ValidatedClientData, ClientDataRefusal> {
        semantic::validate(self, profile, protocol)
    }

    /// Reads the actual version claim without discarding an opaque value elsewhere in the object.
    pub fn game_version(&self) -> io::Result<Option<String>> {
        self.read("GameVersion")
    }

    /// Retains the complete wire i64 domain, including explicit zero; absence remains distinct.
    pub fn random_id(&self) -> io::Result<Option<i64>> {
        self.integer("ClientRandomId")
    }

    /// Borrows no account authority from the downstream's requested display name.
    pub fn display_name(&self) -> io::Result<Option<String>> {
        self.read(super::DISPLAY_NAME)
    }

    /// Replaces every case alias of one owned field with one canonical field at its original slot.
    pub(super) fn insert(&mut self, name: &'static str, value: Value) -> io::Result<()> {
        let value = serde_json::value::to_raw_value(&value).map_err(|_| invalid())?;
        let index = self
            .entries
            .iter()
            .position(|(key, _)| same_field(key, name));
        self.entries.retain(|(key, _)| !same_field(key, name));
        self.entries.insert(
            index.unwrap_or(self.entries.len()).min(self.entries.len()),
            (name.into(), value),
        );
        Ok(())
    }

    /// Reads a nonnullable scalar; a later null keeps the earlier folded value.
    pub(super) fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> io::Result<Option<T>> {
        self.scalar(name)
            .map(|value| serde_json::from_str(value.get()).map_err(|_| invalid()))
            .transpose()
    }

    /// Projects a signed integer token without losing Go's accepted negative-zero spelling.
    pub(super) fn integer(&self, name: &str) -> io::Result<Option<i64>> {
        self.scalar(name).map(types::integer).transpose()
    }

    /// Selects only the scalar view; retained raw entries and nullable/record values are untouched.
    fn scalar(&self, name: &str) -> Option<&RawValue> {
        self.entries
            .iter()
            .rev()
            .find(|(key, value)| same_field(key, name) && value.get() != "null")
            .map(|(_, value)| &**value)
    }
}

impl Serialize for RawClientData {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.entries.len()))?;
        for (name, value) in &self.entries {
            map.serialize_entry(name, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for RawClientData {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = Box::<RawValue>::deserialize(deserializer)?;
        Self::from_raw(&raw).map_err(serde::de::Error::custom)
    }
}

/// The standard JSON object parser supplies original raw values, including duplicate and unknown keys.
struct Entries(Vec<(String, Box<RawValue>)>);

impl<'de> Deserialize<'de> for Entries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Entries;
            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("a client data object")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Entries, M::Error> {
                let mut entries = Vec::new();
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Entries(entries))
            }
        }
        deserializer.deserialize_map(Object)
    }
}

/// Matches ASCII schema names using the same simple-fold aliases as Go's encoding/json.
/// The only non-ASCII folds into ASCII are long s and Kelvin; full metadata Unicode policy is separate.
fn same_field(actual: &str, canonical: &str) -> bool {
    let mut actual = actual.chars();
    for expected in canonical.bytes() {
        let Some(found) = actual.next() else {
            return false;
        };
        let found = match found {
            '\u{017f}' => 's',
            '\u{212a}' => 'k',
            found => found,
        };
        if !found.is_ascii() || !(found as u8).eq_ignore_ascii_case(&expected) {
            return false;
        }
    }
    actual.next().is_none()
}

/// Omits raw values, field names and parser diagnostics from the native metadata boundary.
fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "invalid native raw client data",
    )
}

#[cfg(test)]
mod tests;
