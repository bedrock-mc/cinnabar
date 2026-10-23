//! The client's mirror of the server's DDUI data stores, with the update counters both sides
//! use to order concurrent writes.

use std::collections::HashMap;
use std::sync::Arc;

use super::{DataPrimitive, DataStoreChange, DataStoreUpdate, DataValue, PathError};

/// Named data stores, each a set of properties the server publishes and the UI reads.
#[derive(Debug, Clone, Default)]
pub struct DataStores {
    stores: HashMap<Arc<str>, HashMap<Arc<str>, Property>>,
}

#[derive(Debug, Clone, Default)]
struct Property {
    value: DataValue,
    /// Increments each time the server replaces the whole property.
    update_count: u32,
    /// The last counter seen or sent for each path written inside the property.
    path_counts: HashMap<Arc<str>, u32>,
}

/// What applying one server change did; everything but `Applied` leaves the stores unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreApply {
    Applied,
    /// A path update for a store or property the client does not hold.
    UnknownProperty,
    /// A path update written against an older or newer replacement of its property.
    StaleProperty,
    InvalidPath(PathError),
}

impl DataStores {
    /// Applies one server change.
    ///
    /// A replacement overwrites the property and adopts its counter. A path update applies only
    /// when it was written against the property's current replacement; its path counter is
    /// adopted whenever it applies. A removal drops the whole store.
    pub fn apply(&mut self, change: DataStoreChange) -> StoreApply {
        match change {
            DataStoreChange::Replace {
                store,
                property,
                update_count,
                value,
            } => {
                let entry = self
                    .stores
                    .entry(store)
                    .or_default()
                    .entry(property)
                    .or_default();
                entry.value = value;
                entry.update_count = update_count;
                StoreApply::Applied
            }
            DataStoreChange::Update(update) => {
                let Some(entry) = self
                    .stores
                    .get_mut(&update.store)
                    .and_then(|properties| properties.get_mut(&update.property))
                else {
                    return StoreApply::UnknownProperty;
                };
                if entry.update_count != update.property_update_count {
                    return StoreApply::StaleProperty;
                }
                if let Err(error) = entry.value.set_path(&update.path, update.value) {
                    return StoreApply::InvalidPath(error);
                }
                entry
                    .path_counts
                    .insert(update.path, update.path_update_count);
                StoreApply::Applied
            }
            DataStoreChange::Remove { store } => {
                self.stores.remove(&store);
                StoreApply::Applied
            }
        }
    }

    /// The current value of a property.
    #[must_use]
    pub fn property(&self, store: &str, property: &str) -> Option<&DataValue> {
        self.stores
            .get(store)?
            .get(property)
            .map(|entry| &entry.value)
    }

    /// Writes a user's input locally and returns the update the server must receive.
    ///
    /// The update carries the property's current replacement counter and the path's next
    /// counter, which the server requires to accept the write.
    pub fn write(
        &mut self,
        store: &str,
        property: &str,
        path: &str,
        value: DataPrimitive,
    ) -> Result<DataStoreUpdate, StoreApply> {
        let entries = self
            .stores
            .get_mut(store)
            .ok_or(StoreApply::UnknownProperty)?;
        let property: Arc<str> = entries
            .get_key_value(property)
            .ok_or(StoreApply::UnknownProperty)?
            .0
            .clone();
        let entry = entries
            .get_mut(&property)
            .ok_or(StoreApply::UnknownProperty)?;
        entry
            .value
            .set_path(path, value.clone())
            .map_err(StoreApply::InvalidPath)?;
        let path: Arc<str> = path.into();
        let count = entry.path_counts.entry(path.clone()).or_default();
        *count = count.wrapping_add(1);
        Ok(DataStoreUpdate {
            store: store.into(),
            property,
            path,
            value,
            property_update_count: entry.update_count,
            path_update_count: *count,
        })
    }

    /// Forgets every store, as when the session that owned them ends.
    pub fn clear(&mut self) {
        self.stores.clear();
    }
}
