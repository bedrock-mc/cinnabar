//! Retires cached fallback artwork when its owning provider changes.

use std::sync::Arc;

use super::ServerAtlas;

/// Compares optional source owners without examining their bytes.
pub(super) fn same_owner<T: ?Sized>(a: &Option<Arc<T>>, b: &Option<Arc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Arc::ptr_eq(a, b),
        _ => false,
    }
}

impl ServerAtlas {
    /// Drops derived artwork and pending fallback work; declared pack sources stay indexed.
    pub(super) fn reset_fallback_artwork(&mut self) {
        self.extra.get_mut().clear();
        self.extra_sidecars.get_mut().clear();
        self.local_reads = Default::default();
        self.decodes = Default::default();
        self.native = Default::default();
        self.outlines = Default::default();
        self.resident.clear();
        self.pages.clear();
        self.images.clear();
        self.oversized = Default::default();
        self.dirty = true;
        self.metadata_revision = self.metadata_revision.wrapping_add(1);
        self.metadata_dirty = true;
    }
}
