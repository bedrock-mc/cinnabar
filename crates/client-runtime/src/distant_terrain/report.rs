//! What the session adapter reports for diagnostics.

use chunk_pipeline::DistantTerrainStats;

/// Where the store stands.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StoreState {
    /// No store is wanted.
    #[default]
    Off,
    /// The store is being opened, or waits for an earlier one to close.
    Opening,
    /// The store is open; a read-only store, held by another client, ingests nothing.
    Open { read_only: bool },
    /// The store is open, but its ingestion writer thread could not start, so it ingests
    /// nothing.
    WriterUnavailable,
    /// The store could not be opened for this target.
    Failed,
}

/// The adapter's view of ingestion, for diagnostics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistantTerrainReport {
    pub state: StoreState,
    /// The stream's counters while it ingests.
    pub ingest: Option<DistantTerrainStats>,
    /// Store sessions whose epoch changed: began, merged or split.
    pub epoch_changes: u64,
    /// Failed opens, failed requests and store IO errors.
    pub store_errors: u64,
    /// Store events lost to a full event queue.
    pub dropped_events: u64,
}

impl DistantTerrainReport {
    /// The [`diagnostics::markers::LOD_INGEST`] line of this report: the store's state,
    /// then each counter as `name=value`, ingestion's zero while it is off.
    #[must_use]
    pub fn marker_line(&self) -> String {
        let ingest = self.ingest.unwrap_or_default();
        let state = match self.state {
            StoreState::Off => "off",
            StoreState::Opening => "opening",
            StoreState::Open { read_only: false } => "open",
            StoreState::Open { read_only: true } => "read_only",
            StoreState::WriterUnavailable => "writer_unavailable",
            StoreState::Failed => "failed",
        };
        format!(
            "{marker} state={state} jobs_queued={} sampled={} skipped_unchanged={} superseded={} dropped={} dropped_final_captures={} merged_columns={} sections_written={} store_queue_full={} stale_writes={} handed_slabs={} unkeyed_columns={} unknown_ids={} unkeyed_ids={} registry_full={} worker_ms={} ingest_store_errors={} store_errors={} epoch_changes={} dropped_events={}",
            ingest.jobs_queued,
            ingest.sampled,
            ingest.skipped_unchanged,
            ingest.superseded,
            ingest.dropped,
            ingest.dropped_final_captures,
            ingest.merged_columns,
            ingest.sections_written,
            ingest.store_queue_full,
            ingest.stale_writes,
            ingest.handed_slabs,
            ingest.unkeyed_columns,
            ingest.unknown_ids,
            ingest.unkeyed_ids,
            ingest.registry_full,
            ingest.worker_time.as_millis(),
            ingest.store_errors,
            self.store_errors,
            self.epoch_changes,
            self.dropped_events,
            marker = diagnostics::markers::LOD_INGEST,
        )
    }
}
