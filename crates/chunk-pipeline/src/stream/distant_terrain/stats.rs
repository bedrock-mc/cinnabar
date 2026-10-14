//! Distant terrain ingestion counters.

use std::time::Duration;

/// Distant terrain ingestion counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistantTerrainStats {
    /// Columns waiting to settle or for room in the backlog, including those the writer
    /// inbox handed back.
    pub dirty_columns: usize,
    /// Captures queued and not yet sampled.
    pub in_flight: usize,
    /// Final captures not yet written.
    pub final_captures: usize,
    /// Samples waiting for the writer.
    pub queued: usize,
    /// A writer is working, holds requests back for a full store queue, or has slabs
    /// another writer handed it.
    pub writer_busy: bool,
    /// Jobs queued on the Lod lane.
    pub jobs_queued: u64,
    /// Columns sampled.
    pub sampled: u64,
    /// Columns skipped because the store holds their content hash.
    pub skipped_unchanged: u64,
    /// Captures dropped because a newer capture of their column was taken.
    pub superseded: u64,
    /// Changed captures a full writer inbox refused; each column was marked dirty again
    /// and is captured once there is room, so no change is lost.
    pub deferred: u64,
    /// Evicted columns dropped beyond [`super::MAX_LOD_PENDING_FINAL_CAPTURES`].
    pub dropped_final_captures: u64,
    /// Columns merged into stored sections.
    pub merged_columns: u64,
    /// Section records submitted.
    pub sections_written: u64,
    /// Reads and requests the store failed or refused after stopping.
    pub store_errors: u64,
    /// Requests a full store queue handed back; they are retried in order.
    pub store_queue_full: u64,
    /// Merges the store refused because a session changed while they were prepared; their
    /// columns were merged again.
    pub stale_writes: u64,
    /// Neighbour slab re-mips one writer handed to the writer owning the neighbour.
    pub handed_slabs: u64,
    /// Columns outside the section key range, never sampled.
    pub unkeyed_columns: u64,
    /// Ids the session's assets do not know, sampled as air; once per sampled column.
    pub unknown_ids: u64,
    /// Visible ids no material key names, sampled as air; once per sampled column.
    pub unkeyed_ids: u64,
    /// Visible states sampled as air because the store's registry is full; once per
    /// sampled column.
    pub registry_full: u64,
    /// Time sample jobs held background world workers.
    pub worker_time: Duration,
}

impl DistantTerrainStats {
    /// True when no sampled or captured column still waits to reach the store's queue;
    /// dirty columns do not count.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        self.in_flight == 0 && self.final_captures == 0 && self.queued == 0 && !self.writer_busy
    }
}
