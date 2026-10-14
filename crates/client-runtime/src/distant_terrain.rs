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
pub mod drawing;
#[cfg(test)]
mod drawing_tests;
mod lifecycle;
mod report;
#[cfg(test)]
mod tests;

use std::{collections::VecDeque, path::Path, sync::mpsc::Receiver, time::Duration};

use chunk_pipeline::{DistantTerrainLink, WorldStream};

pub use self::report::{DistantTerrainReport, StoreState};
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
/// Minutes a smaller cap must stay unchanged before an open store applies it, so passing
/// through small values on the way to another never evicts.
pub const QUOTA_SETTLE_MINUTES: u32 = 1;
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
    /// Distant terrain is drawn: an open store meshes for the renderer.
    drawing: bool,
}

/// The adapter's store, from none to open.
enum State {
    Off,
    Opening {
        target: DistantTerrainTarget,
        opened: Receiver<Result<StoreActor, String>>,
        /// The quotas the store's thread starts with.
        quotas: Quotas,
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
    /// The quotas the store's thread applies.
    quotas: Quotas,
    /// Smaller quotas waiting to settle, and the minute they were first asked for.
    settling: Option<(Quotas, Minute)>,
    /// Smaller quotas were applied: a quota pass runs now rather than at the next interval.
    quota_pass_due: bool,
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
            drawing: false,
        }
    }

    /// Meshes an open store's sections for the renderer while `drawing`, and stops meshing
    /// otherwise; the owner sets it every frame.
    pub fn set_drawing(&mut self, drawing: bool) {
        self.drawing = drawing;
    }

    /// Caps each store at `bytes`, within the cache's total cap. An open store applies a
    /// larger cap at once, and a smaller one with a quota pass once it stayed unchanged
    /// for [`QUOTA_SETTLE_MINUTES`]; later stores open with it.
    pub fn set_store_cap(&mut self, bytes: u64) {
        let bytes = bytes.min(self.quotas.global_bytes);
        if self.quotas.store_bytes == bytes {
            return;
        }
        self.quotas.store_bytes = bytes;
    }

    /// The quotas stores open with.
    #[must_use]
    pub fn quotas(&self) -> Quotas {
        self.quotas
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
            open.drive(&mut frame, &mut self.report, self.drawing, self.quotas);
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
            stream.disable_distant_terrain_meshes();
        }
        match std::mem::replace(&mut self.state, State::Off) {
            State::Open(open) => self.finishing.push(open.finish(uptime)),
            // An open in progress finishes, then closes in `reconcile`.
            opening @ State::Opening { .. } => self.state = opening,
            State::Off | State::Failed(_) => {}
        }
    }
}
