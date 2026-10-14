//! Distant terrain ingestion: an opt-in Cinnabar extension, not vanilla behaviour, that
//! samples explored terrain into a [`lod_store`] store. While it is off the hooks cost one
//! branch and nothing else runs.
//!
//! - Every block change marks its column dirty. A dirty column is due once it has gone
//!   [`LOD_SETTLE_POLLS`] polls without another change and no light work is pending for
//!   it. Each poll within its budget takes at most [`MAX_LOD_COLUMNS_PER_POLL`] due
//!   columns and queues them as sample jobs on the background-only Lod lane while fewer
//!   than [`MAX_LOD_BACKLOG`] samples are in flight or wait for the writer; due columns
//!   that find the backlog full are dropped and counted, as a final capture keeps them.
//! - Final captures keep a column's last state: an eviction captures each retired column
//!   with its light before the light is removed, and a stream that stops ingesting
//!   captures every resident column, [`MAX_LOD_FINAL_CAPTURES_PER_POLL`] a poll (all at
//!   once when it is dropped). They are never dropped for a full backlog or inbox; only
//!   eviction captures beyond [`MAX_LOD_PENDING_FINAL_CAPTURES`] unwritten ones are
//!   dropped and counted, which bounds the retired chunk memory they hold.
//! - A column's content hash covers its blocks, biomes and which sub-chunks had solved
//!   light. The frame reuses a hash for a column whose sub-chunk and biome allocations are
//!   unchanged since a job hashed them ([`fingerprint`]), and skips a column whose hash the
//!   column book shows stored in the session's own epoch without queueing any job. Sample
//!   jobs hash the rest and skip them the same way. Nothing waits for the store: the
//!   writer thread asks it for hashes the book does not know. Hashes the book learned
//!   before a store session changed are unknown afterwards.
//! - Light values are left out of the hash, so a light job that changes a captured
//!   column's light marks it dirty and relit: once settled it is sampled whatever its
//!   hash.
//! - Every capture carries a sequence number; the writer never lets an older capture of a
//!   column replace a newer one.
//! - The writer thread, the only thread that talks to the store, merges samples into the
//!   sections they share ([`lod::ingest`]), reading only the session's own epoch, and
//!   submits sections, surfaces and content hashes without blocking.

mod blocks;
pub(in crate::stream) mod book;
mod fingerprint;
mod hash;
pub(in crate::stream) mod hooks;
pub(in crate::stream) mod jobs;
mod link;
pub(in crate::stream) mod shared;
pub(in crate::stream) mod writer;

use std::{collections::VecDeque, sync::Arc, time::Duration};

use hashbrown::{HashMap, HashSet};

use lod_store::{actor::StoreClient, minute::Minute, region_file::Access};
use world::ChunkKey;

pub use self::link::DistantTerrainLink;
use self::{blocks::SessionBlocks, fingerprint::Fingerprint, shared::LodShared};
use super::WorldStream;

/// Polls a dirty column waits without another change before it is sampled, so bursts of
/// block changes are sampled once.
pub const LOD_SETTLE_POLLS: u64 = 20;
/// Due columns one poll takes at most.
pub const MAX_LOD_COLUMNS_PER_POLL: usize = 8;
/// Settled columns one poll checks for pending light at most, so columns waiting for light
/// never make a poll scan every dirty column.
const MAX_LOD_COLUMNS_EXAMINED_PER_POLL: usize = 4 * MAX_LOD_COLUMNS_PER_POLL;
/// Samples in flight or waiting for the writer at which changed columns are dropped.
pub const MAX_LOD_BACKLOG: usize = 16;
/// Unwritten final captures beyond which an eviction drops its columns and counts them.
pub const MAX_LOD_PENDING_FINAL_CAPTURES: usize = 1024;
/// Resident columns a stream that stops ingesting captures per poll.
pub const MAX_LOD_FINAL_CAPTURES_PER_POLL: usize = 32;
/// Items waiting for the writer beyond which changed columns are dropped and counted.
pub(in crate::stream) const MAX_LOD_INBOX_COLUMNS: usize = 32;
/// Columns one merge takes.
const MERGE_BATCH_COLUMNS: usize = 8;
/// Sections the writer keeps cached.
const LOD_SECTION_CACHE_ENTRIES: usize = 2048;
/// Columns the column book holds before it is emptied at the next idle moment.
const MAX_LOD_BOOK_COLUMNS: usize = 1 << 16;

// Dispatch keeps changed columns from overflowing the inbox.
const _: () = assert!(MAX_LOD_BACKLOG <= MAX_LOD_INBOX_COLUMNS);

/// Distant terrain ingestion counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistantTerrainStats {
    /// Columns waiting to settle.
    pub dirty_columns: usize,
    /// Captures queued and not yet sampled.
    pub in_flight: usize,
    /// Final captures not yet written.
    pub final_captures: usize,
    /// Samples waiting for the writer.
    pub queued: usize,
    /// The writer is working, or holds requests back for a full store queue.
    pub writer_busy: bool,
    /// Jobs queued on the Lod lane.
    pub jobs_queued: u64,
    /// Columns sampled.
    pub sampled: u64,
    /// Columns skipped because the store holds their content hash.
    pub skipped_unchanged: u64,
    /// Captures dropped because a newer capture of their column was taken.
    pub superseded: u64,
    /// Changed columns dropped for a full backlog or writer inbox.
    pub dropped: u64,
    /// Evicted columns dropped beyond [`MAX_LOD_PENDING_FINAL_CAPTURES`].
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
    /// dirty columns that have not settled do not count.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        self.in_flight == 0 && self.final_captures == 0 && self.queued == 0 && !self.writer_busy
    }
}

/// The frame's ingestion state.
pub(super) struct LodIngest {
    pub(in crate::stream) shared: Arc<LodShared>,
    /// Dirty columns with the poll of their last change.
    dirty: HashMap<ChunkKey, u64>,
    /// Dirty columns by the poll they were marked at, oldest first; an entry whose poll no
    /// longer matches `dirty` is stale.
    order: VecDeque<(u64, ChunkKey)>,
    polls: u64,
    /// The sequence number of the latest capture.
    seq: u64,
    blocks: Option<Arc<SessionBlocks>>,
    /// What the hashes of resident columns were computed from.
    fingerprints: HashMap<ChunkKey, Fingerprint>,
    /// Columns whose light changed since their last capture.
    relit: HashSet<ChunkKey>,
    /// Resident columns a stream that stops ingesting has still to capture; `None` while
    /// it ingests.
    finishing: Option<VecDeque<ChunkKey>>,
}

/// Why [`WorldStream::enable_distant_terrain`] left ingestion off.
#[derive(Debug, thiserror::Error)]
pub enum DistantTerrainUnavailable {
    /// The store is read-only: another client writes it.
    #[error("the distant terrain store is read-only")]
    ReadOnly,
    /// The writer thread could not start.
    #[error("the distant terrain writer thread could not start: {0}")]
    WriterThread(#[source] std::io::Error),
}

impl Drop for LodIngest {
    /// Tells the owner's link that the stream stopped capturing.
    fn drop(&mut self) {
        self.shared.release();
    }
}

impl Drop for WorldStream {
    /// A stream dropped while ingesting captures all its resident columns first.
    fn drop(&mut self) {
        self.finish_distant_terrain();
        self.capture_final_columns(usize::MAX);
    }
}

impl WorldStream {
    /// Starts distant terrain ingestion into the store behind `client`, stamping requests
    /// with `now`, and marks every resident column dirty. The caller has begun a store
    /// session in each dimension it will send terrain for, and keeps the returned link
    /// until the stream's captures are written. A read-only store, or a writer thread
    /// that cannot start, keeps ingestion off. A stream still finishing earlier ingestion
    /// captures the rest of its columns first.
    pub fn enable_distant_terrain(
        &mut self,
        client: StoreClient,
        now: Minute,
    ) -> Result<DistantTerrainLink, DistantTerrainUnavailable> {
        self.capture_final_columns(usize::MAX);
        self.disable_distant_terrain();
        if client.access() == Access::ReadOnly {
            return Err(DistantTerrainUnavailable::ReadOnly);
        }
        let shared = Arc::new(LodShared::new(client, now));
        writer::spawn(Arc::clone(&shared)).map_err(DistantTerrainUnavailable::WriterThread)?;
        let mut lod = LodIngest {
            shared: Arc::clone(&shared),
            dirty: HashMap::new(),
            order: VecDeque::new(),
            polls: 0,
            seq: 0,
            blocks: None,
            fingerprints: HashMap::new(),
            relit: HashSet::new(),
            finishing: None,
        };
        for column in self.resident.columns() {
            lod.mark(column);
        }
        self.lod = Some(Box::new(lod));
        Ok(DistantTerrainLink::new(shared))
    }

    /// Stops ingestion at once; queued and running work is dropped.
    pub fn disable_distant_terrain(&mut self) {
        if let Some(lod) = self.lod.take() {
            lod.shared.disable();
        }
    }

    /// Stops capturing changes and takes every resident column as a final capture with
    /// its light: [`MAX_LOD_FINAL_CAPTURES_PER_POLL`] now and as many each later poll, so
    /// a large view never stalls the frame. Ingestion stops once all are taken; the link
    /// reports when those captures and everything queued before them are written.
    pub fn finish_distant_terrain(&mut self) {
        let resident = &self.resident;
        let Some(lod) = self.lod.as_deref_mut() else {
            return;
        };
        if !lod.shared.enabled() {
            self.lod = None;
            return;
        }
        if lod.finishing.is_none() {
            lod.finishing = Some(resident.columns().collect());
            lod.dirty.clear();
            lod.order.clear();
        }
        self.capture_final_columns(MAX_LOD_FINAL_CAPTURES_PER_POLL);
    }

    /// True while distant terrain ingestion runs: started, and neither finishing, stopped
    /// by the stream nor by its owner dropping the link.
    #[must_use]
    pub fn distant_terrain_enabled(&self) -> bool {
        self.lod
            .as_ref()
            .is_some_and(|lod| lod.finishing.is_none() && lod.shared.enabled())
    }

    /// Sets the minute stamped on later store requests; edge code reads the clock.
    pub fn set_distant_terrain_minute(&mut self, now: Minute) {
        if let Some(lod) = &self.lod {
            lod.shared.set_now(now);
        }
    }

    /// Forgets cached sections and content hashes after a store request failed, so later
    /// merges and skips follow what the store now holds. Session changes need no call:
    /// the store's [`lod_store::actor::SessionEpoch`] moves on with them.
    pub fn invalidate_distant_terrain_sections(&mut self) {
        if let Some(lod) = &self.lod {
            lod.shared.invalidate();
        }
    }

    /// Ingestion counters, counting the columns a finishing stream has still to capture as
    /// dirty; `None` while ingestion is off.
    #[must_use]
    pub fn distant_terrain_stats(&self) -> Option<DistantTerrainStats> {
        self.lod.as_ref().map(|lod| {
            let finishing = lod.finishing.as_ref().map_or(0, VecDeque::len);
            lod.shared.stats(lod.dirty.len() + finishing)
        })
    }
}
