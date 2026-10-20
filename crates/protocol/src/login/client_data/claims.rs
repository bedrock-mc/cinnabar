//! One signing boundary for canonical desktop maps and original relay value representations.

use super::{DISPLAY_NAME, RawClientData};
use serde_json::{Map, Value, value::RawValue};
use std::io;

/// Concrete representation only; neither variant supplies account or transport authorization.
pub(crate) enum NativeClaims {
    Canonical(Map<String, Value>),
    Relay(RawClientData),
}

impl serde::Serialize for NativeClaims {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Canonical(claims) => serde::Serialize::serialize(claims, serializer),
            Self::Relay(claims) => serde::Serialize::serialize(claims, serializer),
        }
    }
}

impl NativeClaims {
    /// Keeps the original desktop map parser and sorted serialization unchanged.
    pub(crate) fn canonical(raw: &RawValue) -> io::Result<Self> {
        let Value::Object(claims) = serde_json::from_str(raw.get()).map_err(|_| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid native client data")
        })?
        else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native client data must be a JSON object",
            ));
        };
        Ok(Self::Canonical(claims))
    }

    /// Installs only the provider-verified or application-selected offline display name.
    pub(crate) fn set_display_name(&mut self, name: &str) -> io::Result<()> {
        self.insert(DISPLAY_NAME, Value::String(name.into()))
    }

    /// Publishes the actual selected transport metadata at the sole Login signing boundary.
    pub(crate) fn set_transport(&mut self, address: &str, nonce: &str) -> io::Result<()> {
        self.insert("ServerAddress", Value::String(address.into()))?;
        self.insert("Nonce", Value::String(nonce.into()))
    }

    /// Preserves the desktop's original empty-name fallback for typed OfflineIdentity rejection.
    pub(crate) fn display_name(&self) -> io::Result<String> {
        match self {
            Self::Canonical(claims) => Ok(claims
                .get(DISPLAY_NAME)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into()),
            Self::Relay(claims) => Ok(claims.display_name()?.unwrap_or_default()),
        }
    }

    /// Retains desktop map bytes and opaque relay lexemes without converting relay values to Value.
    pub(crate) fn into_raw(self) -> io::Result<Box<RawValue>> {
        serde_json::value::to_raw_value(&self).map_err(io::Error::other)
    }

    /// Uses one field overlay for both actual representation owners.
    fn insert(&mut self, name: &'static str, value: Value) -> io::Result<()> {
        match self {
            Self::Canonical(claims) => {
                claims.insert(name.into(), value);
                Ok(())
            }
            Self::Relay(claims) => claims.insert(name, value),
        }
    }
}
