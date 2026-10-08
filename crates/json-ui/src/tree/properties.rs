//! Shared property maps detach only when a control's properties change.

use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Weak};

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A control's properties, shared across unchanged tree copies.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Properties(Arc<BTreeMap<String, Value>>);

impl Properties {
    /// Identity of the immutable map, preserved when resolved templates are cloned.
    pub(crate) fn identity(&self) -> usize {
        Arc::as_ptr(&self.0) as usize
    }

    /// Observe this map's lifetime without retaining a removed pack's properties.
    pub(crate) fn weak(&self) -> Weak<BTreeMap<String, Value>> {
        Arc::downgrade(&self.0)
    }
}

impl PartialEq for Properties {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

impl Deref for Properties {
    type Target = BTreeMap<String, Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Properties {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

impl From<BTreeMap<String, Value>> for Properties {
    fn from(properties: BTreeMap<String, Value>) -> Self {
        Self(Arc::new(properties))
    }
}

impl<const N: usize> From<[(String, Value); N]> for Properties {
    fn from(properties: [(String, Value); N]) -> Self {
        BTreeMap::from(properties).into()
    }
}

impl FromIterator<(String, Value)> for Properties {
    fn from_iter<T: IntoIterator<Item = (String, Value)>>(iter: T) -> Self {
        BTreeMap::from_iter(iter).into()
    }
}

impl IntoIterator for Properties {
    type Item = (String, Value);
    type IntoIter = std::collections::btree_map::IntoIter<String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        Arc::unwrap_or_clone(self.0).into_iter()
    }
}

impl<'a> IntoIterator for &'a Properties {
    type Item = (&'a String, &'a Value);
    type IntoIter = std::collections::btree_map::Iter<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a mut Properties {
    type Item = (&'a String, &'a mut Value);
    type IntoIter = std::collections::btree_map::IterMut<'a, String, Value>;

    fn into_iter(self) -> Self::IntoIter {
        Arc::make_mut(&mut self.0).iter_mut()
    }
}
