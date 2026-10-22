//! Change stamps for texture state that retained paints compare.

use std::sync::atomic::{AtomicU64, Ordering};

/// A process-wide unique stamp: every new owner and every change takes a value no other owner
/// or earlier state ever had, so a replaced atlas never matches a paint of its predecessor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Revision(u64);

impl Revision {
    /// Marks the owner changed.
    pub(crate) fn bump(&mut self) {
        *self = Self::default();
    }
}

impl Default for Revision {
    fn default() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

/// Identifies everything a paint reads from one server atlas: its residency and sources, its
/// original sheets and its source outlines. Any change yields a new value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AtlasRevision {
    pub(crate) atlas: Revision,
    pub(crate) native: Revision,
    pub(crate) outlines: Revision,
}
