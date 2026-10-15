//! Ordered menu data shared between its owner and retained presentation views.

use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// A menu list whose clones retain the current storage generation.
///
/// Reading or cloning a list never copies its entries. Ordinary vector edits use
/// copy-on-write, so a previously published view keeps its original entries and order.
/// Whole-list feeds can use [`Self::replace`] to retain storage for equal deliveries.
#[derive(Debug)]
pub struct MenuList<T>(Arc<Vec<T>>);

impl<T> MenuList<T> {
    /// Whether both lists retain the same published storage generation.
    pub fn shares_storage(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T: PartialEq> MenuList<T> {
    /// Publishes changed entries in their supplied order; equal deliveries keep existing storage.
    pub fn replace(&mut self, entries: Vec<T>) {
        if self.0.as_ref() != &entries {
            self.0 = Arc::new(entries);
        }
    }
}

impl<T> Clone for MenuList<T> {
    /// Retains this generation without cloning any entries.
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> Default for MenuList<T> {
    /// Creates an empty list with its own storage generation.
    fn default() -> Self {
        Vec::new().into()
    }
}

impl<T> From<Vec<T>> for MenuList<T> {
    /// Takes an owned vector without copying its entries.
    fn from(entries: Vec<T>) -> Self {
        Self(Arc::new(entries))
    }
}

impl<T> FromIterator<T> for MenuList<T> {
    /// Collects entries once in their original iteration order.
    fn from_iter<I: IntoIterator<Item = T>>(entries: I) -> Self {
        Vec::from_iter(entries).into()
    }
}

impl<T> Deref for MenuList<T> {
    type Target = Vec<T>;

    /// Exposes the current entries through the vector's read-only API.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: Clone> DerefMut for MenuList<T> {
    /// Detaches shared entries before any vector edit can affect a retained view.
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.0)
    }
}

impl<T: Eq> PartialEq for MenuList<T> {
    /// Compares list contents, with no entry walk when storage is already shared.
    fn eq(&self, other: &Self) -> bool {
        self.shares_storage(other) || self.0 == other.0
    }
}

impl<T: Eq> Eq for MenuList<T> {}

impl<'a, T> IntoIterator for &'a MenuList<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    /// Iterates the retained entries without allocating or copying them.
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a, T: Clone> IntoIterator for &'a mut MenuList<T> {
    type Item = &'a mut T;
    type IntoIter = std::slice::IterMut<'a, T>;

    /// Detaches shared storage before yielding mutable entries.
    fn into_iter(self) -> Self::IntoIter {
        self.iter_mut()
    }
}

impl<T: Clone> IntoIterator for MenuList<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    /// Moves uniquely owned entries, or copies them if another view still retains them.
    fn into_iter(self) -> Self::IntoIter {
        Arc::unwrap_or_clone(self.0).into_iter()
    }
}

#[cfg(test)]
mod tests;
