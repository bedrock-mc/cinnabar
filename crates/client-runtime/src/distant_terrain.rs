//! The session side of distant terrain ingestion, an opt-in Cinnabar extension: opens
//! the [`lod_store`] store of the session's destination world off the frame, begins and
//! ends its per-dimension sessions, attaches the store to the [`WorldStream`], stamps the
//! minute, paces flushes, syncs and quotas, and closes the store without blocking.
//!
//! The owner calls [`DistantTerrainSessions::update`] once a frame with the
//! [`DistantTerrainTarget`] the session should ingest into, or `None` while the Video
//! setting is off or no session runs. Edge code reads the clocks and passes the minute
//! and the monotonic uptime.

mod dimensions;
mod lifecycle;
#[cfg(test)]
mod tests;

use std::{collections::VecDeque, path::Path, sync::mpsc::Receiver, time::Duration};

use chunk_pipeline::{DistantTerrainLink, DistantTerrainStats, WorldStream};

use self::{dimensions::DimensionSessions, lifecycle::Finishing};
use lod_store::{
    actor::{Closing, Request, StoreActor},
    identity::StoreKey,
    minute::Minute,
    root::{Quotas, StoreRoot},
};

/// Minutes between commits of the store's written data.
pub const FLUSH_INTERVAL_MINUTES: u32 = 1;
/// Minutes between quota passes, which walk every store directory.
pub const QUOTA_INTERVAL_MINUTES: u32 = 10;
/// How long a closing store waits for the stream's final captures before it closes anyway.
pub const FINISH_LIMIT: Duration = Duration::from_secs(60);

/// The store a session ingests into: its world's key and the session it belongs to, so a
/// new session on the same world begins its epochs again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistantTerrainTarget {
    pub key: StoreKey,
    pub session_generation: u64,
}

impl DistantTerrainTarget {
    /// The target of session `session_generation` at `destination`, with StartGame's
    /// `level_id` and `seed`.
    #[must_use]
    pub fn new(destination: &str, level_id: &str, seed: u64, session_generation: u64) -> Self {
        Self {
            key: StoreKey::new(destination, level_id, seed),
            session_generation,
        }
    }
}

/// What one frame hands the adapter.
pub struct DistantTerrainFrame<'a> {
    /// The store to ingest into; `None` closes any open store.
    pub target: Option<DistantTerrainTarget>,
    /// The session's stream, when it is on this thread.
    pub stream: Option<&'a mut WorldStream>,
    /// The current minute, read by edge code.
    pub now: Minute,
    /// Monotonic time since an origin the caller keeps fixed, read by edge code; it times
    /// how long a closing store waits.
    pub uptime: Duration,
    /// The player's block `x` and `z`, which order quota eviction.
    pub player_block: Option<[i64; 2]>,
}

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
            "{marker} state={state} jobs_queued={} sampled={} skipped_unchanged={} superseded={} dropped={} dropped_final_captures={} merged_columns={} sections_written={} store_queue_full={} stale_writes={} unkeyed_columns={} unknown_ids={} unkeyed_ids={} registry_full={} worker_ms={} ingest_store_errors={} store_errors={} epoch_changes={} dropped_events={}",
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

/// The distant terrain store of the running session. It holds channel receivers, so a
/// Bevy owner keeps it behind a `Mutex` and reaches it with `Mutex::get_mut`.
pub struct DistantTerrainSessions {
    root: StoreRoot,
    quotas: Quotas,
    state: State,
    /// Stores waiting for their streams' final captures before they close.
    finishing: Vec<Finishing>,
    /// Stores on their way down; a new store opens once they released their locks.
    closing: Vec<Closing>,
    /// Events closed stores dropped for a full event queue.
    dropped_closed: u64,
    report: DistantTerrainReport,
}

/// The adapter's store, from none to open.
enum State {
    Off,
    Opening {
        target: DistantTerrainTarget,
        opened: Receiver<Result<StoreActor, String>>,
    },
    Failed(DistantTerrainTarget),
    Open(Box<OpenStore>),
}

/// Whether an open store's stream ingests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ingest {
    /// The stream ingests, or will once it is attached.
    Writable,
    /// Another client writes the store.
    ReadOnly,
    /// The writer thread could not start; not retried for this store.
    WriterUnavailable,
}

/// An open store and the sessions begun in it.
struct OpenStore {
    target: DistantTerrainTarget,
    actor: StoreActor,
    ingest: Ingest,
    /// Ingestion runs into this store: the attached stream's, and those of streams that
    /// stopped ingesting while their final captures are still written.
    links: Vec<DistantTerrainLink>,
    sessions: DimensionSessions,
    /// Session requests the full queue handed back, oldest first.
    pending: VecDeque<Request>,
    last_flush: Minute,
    last_quotas: Minute,
    /// Sections written when the store last synced.
    synced_sections: u64,
    /// A store request failed since the stream's cached sections were last dropped.
    invalidate_due: bool,
}

impl DistantTerrainSessions {
    /// An adapter for stores under `lod_cache_dir`, the install layout's distant terrain
    /// directory, with the default quotas.
    #[must_use]
    pub fn new(lod_cache_dir: &Path) -> Self {
        Self::with_quotas(lod_cache_dir, Quotas::default())
    }

    /// [`Self::new`] with `quotas`.
    #[must_use]
    pub fn with_quotas(lod_cache_dir: &Path, quotas: Quotas) -> Self {
        Self {
            root: StoreRoot::new(lod_cache_dir),
            quotas,
            state: State::Off,
            finishing: Vec::new(),
            closing: Vec::new(),
            dropped_closed: 0,
            report: DistantTerrainReport::default(),
        }
    }

    /// The latest report; [`Self::update`] refreshes it.
    #[must_use]
    pub fn report(&self) -> DistantTerrainReport {
        self.report
    }

    /// Dimensions with a store session begun and not yet ended.
    #[must_use]
    pub fn session_dimensions(&self) -> Vec<i32> {
        match &self.state {
            State::Open(open) => open
                .sessions
                .dimensions()
                .map(|dimension| dimension.0)
                .collect(),
            _ => Vec::new(),
        }
    }

    /// True while an earlier store still waits for final captures or is closing.
    #[must_use]
    pub fn is_closing(&self) -> bool {
        !self.finishing.is_empty() || !self.closing.is_empty()
    }

    /// True when no store is wanted, open or on its way down, so a frame without a
    /// target has nothing to do and needs no minute.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Off) && !self.is_closing()
    }

    /// Reconciles the store with `frame`: opens, attaches, paces and closes it. Never
    /// blocks on the store.
    pub fn update(&mut self, mut frame: DistantTerrainFrame<'_>) {
        self.poll_finishing(frame.now, frame.uptime);
        self.poll_closing();
        self.reconcile(&mut frame);
        let mut dropped = self.dropped_closed;
        if let State::Open(open) = &mut self.state {
            open.drive(&mut frame, &mut self.report);
            dropped += open.actor.dropped_events();
        }
        self.report.dropped_events = dropped;
        self.report.state = match &self.state {
            State::Off => StoreState::Off,
            State::Opening { .. } => StoreState::Opening,
            State::Failed(_) => StoreState::Failed,
            State::Open(open) => match open.ingest {
                Ingest::Writable => StoreState::Open { read_only: false },
                Ingest::ReadOnly => StoreState::Open { read_only: true },
                Ingest::WriterUnavailable => StoreState::WriterUnavailable,
            },
        };
        self.report.ingest = frame
            .stream
            .as_deref()
            .and_then(WorldStream::distant_terrain_stats);
    }

    /// Closes any open store without waiting: `stream` stops ingesting after capturing
    /// its resident columns over its next polls, and the store closes once the final
    /// captures of every stream that wrote into it are written, or [`FINISH_LIMIT`] after
    /// `uptime`.
    pub fn close(&mut self, stream: Option<&mut WorldStream>, uptime: Duration) {
        if let Some(stream) = stream {
            stream.finish_distant_terrain();
        }
        match std::mem::replace(&mut self.state, State::Off) {
            State::Open(open) => self.finishing.push(open.finish(uptime)),
            // An open in progress finishes, then closes in `reconcile`.
            opening @ State::Opening { .. } => self.state = opening,
            State::Off | State::Failed(_) => {}
        }
    }
}
