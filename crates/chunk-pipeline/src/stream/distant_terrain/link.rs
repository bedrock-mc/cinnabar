//! The owner's handle on one ingestion run, which outlives the stream's part of it.

use std::sync::Arc;

use super::{DistantTerrainStats, shared::LodShared};

/// The owner's handle on distant terrain ingestion started by
/// [`crate::WorldStream::enable_distant_terrain`]. It tells when the stream has stopped
/// capturing and everything it captured has reached the store's queue, so the owner closes
/// the store only after the stream's final captures. Dropping it stops ingestion at once,
/// dropping whatever is still queued.
pub struct DistantTerrainLink {
    shared: Arc<LodShared>,
}

impl DistantTerrainLink {
    /// The link of ingestion through `shared`.
    pub(super) fn new(shared: Arc<LodShared>) -> Self {
        Self { shared }
    }

    /// Nudges a writer waiting for the store's full queue, as the stream's poll does,
    /// and returns true once the stream stopped capturing and nothing it captured still
    /// waits to reach the store's queue. Never blocks.
    pub fn poll_written(&self) -> bool {
        self.shared.nudge_writer();
        self.shared.released() && self.shared.stats(0).is_settled()
    }

    /// True once the stream stopped capturing, finished or dropped.
    #[must_use]
    pub fn is_released(&self) -> bool {
        self.shared.released()
    }

    /// The counters, with no dirty columns once the stream stopped capturing.
    #[must_use]
    pub fn stats(&self) -> DistantTerrainStats {
        self.shared.stats(0)
    }
}

impl Drop for DistantTerrainLink {
    fn drop(&mut self) {
        self.shared.disable();
    }
}
