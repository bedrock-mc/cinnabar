//! Shared, sorted source metadata for full-resolution artwork requests.

use std::{ops::Deref, sync::Arc};

/// Sorted keys and their shared encoded source bytes.
type SourceEntries = Vec<(String, Arc<[u8]>)>;

/// An immutable view of oversized texture keys and their original encoded bytes.
/// The atlas keeps at most `MAX_OVERSIZED` entries. Readers share the current vector;
/// a source edit detaches it without copying pixels or retaining previous snapshots.
#[derive(Clone, Default)]
pub struct OversizedTextures(Option<Arc<SourceEntries>>);

impl OversizedTextures {
    /// Whether both views retain the same metadata, without walking their entries.
    pub fn shares_storage(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }

    /// Compares keys and byte ownership, inspecting entries only for different snapshots.
    pub fn same(&self, other: &Self) -> bool {
        self.shares_storage(other)
            || (self.len() == other.len()
                && self
                    .iter()
                    .zip(other.iter())
                    .all(|(a, b)| a.0 == b.0 && Arc::ptr_eq(&a.1, &b.1)))
    }

    /// Publishes a changed source in key order, preserving identical byte ownership on retries.
    pub(super) fn insert(&mut self, key: &str, bytes: &Arc<[u8]>) {
        let index = self.binary_search_by(|entry| entry.0.as_str().cmp(key));
        match index {
            Ok(index) if Arc::ptr_eq(&self[index].1, bytes) => return,
            Err(_) if self.len() >= super::MAX_OVERSIZED => return,
            _ => {}
        }
        let entries = Arc::make_mut(self.0.get_or_insert_with(|| Arc::new(Vec::new())));
        match index {
            Ok(index) => entries[index].1 = Arc::clone(bytes),
            Err(index) => {
                entries.reserve_exact(1);
                entries.insert(index, (key.to_owned(), Arc::clone(bytes)));
            }
        }
    }
}

impl Deref for OversizedTextures {
    type Target = [(String, Arc<[u8]>)];

    /// Borrows the sorted entries without creating an empty allocation.
    fn deref(&self) -> &Self::Target {
        self.0.as_deref().map_or(&[], Vec::as_slice)
    }
}

// Fixtures may exceed the atlas's source bound to exercise downstream admission limits.
#[cfg(any(test, feature = "test-support"))]
impl FromIterator<(String, Arc<[u8]>)> for OversizedTextures {
    /// Builds a sorted fixture with one entry per key, retaining the last supplied bytes.
    fn from_iter<T: IntoIterator<Item = (String, Arc<[u8]>)>>(entries: T) -> Self {
        let entries: std::collections::BTreeMap<_, _> = entries.into_iter().collect();
        if entries.is_empty() {
            Self::default()
        } else {
            Self(Some(Arc::new(entries.into_iter().collect())))
        }
    }
}

#[cfg(test)]
mod tests;
