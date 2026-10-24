//! Versioned typed records for the callback's player, instance, and mod scopes.
use super::{Callback, StoreKey, StoreScope, WorldError};
use serde::{Serialize, de::DeserializeOwned};

/// A stored type defines its stable schema revision; bump it when its encoding changes.
pub trait Persistent: Serialize + DeserializeOwned {
    const SCHEMA: u32;
}

/// A host refusal or invalid stored encoding, neither of which overwrites existing data.
#[derive(Debug)]
pub enum StateError {
    Host(WorldError),
    Encoding(serde_json::Error),
}

impl Callback {
    /// Reads one typed record. A different schema revision requires explicit migration.
    pub fn read_state<T: Persistent>(&self, scope: StoreScope) -> Result<Option<T>, StateError> {
        self.store_read(StoreKey {
            scope,
            schema: T::SCHEMA,
        })
        .map_err(StateError::Host)?
        .map(|data| serde_json::from_slice(&data).map_err(StateError::Encoding))
        .transpose()
    }
    /// Stages one typed record, or deletes it when value is absent.
    pub fn write_state<T: Persistent>(
        &self,
        scope: StoreScope,
        value: Option<&T>,
    ) -> Result<(), StateError> {
        let data = value
            .map(serde_json::to_vec)
            .transpose()
            .map_err(StateError::Encoding)?;
        self.store_write(
            StoreKey {
                scope,
                schema: T::SCHEMA,
            },
            data.as_deref(),
        )
        .map_err(StateError::Host)
    }
}
