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
//!   writer threads ask it for hashes the book does not know. Hashes the book learned
//!   before a store session changed are unknown afterwards.
//! - Light values are left out of the hash, so a light job that changes a captured
//!   column's light marks it dirty and relit: once settled it is sampled whatever its
//!   hash.
//! - Every capture carries a sequence number; the writer never lets an older capture of a
//!   column replace a newer one.
//! - Up to [`MAX_LOD_WRITERS`] writer threads merge samples into the sections they share
//!   ([`lod::ingest`]), each owning whole writer regions, reading only the session's own
//!   epoch, and submit sections, surfaces and content hashes without blocking. They and the
//!   mesher thread ([`meshes`]) are the only threads that talk to the store, so world and
//!   mesh workers never wait for disk.
//! - The writers' section caches, the mesher's snapshots and the meshes it keeps are bounded
//!   in bytes within [`LOD_CPU_MEMORY_BYTES`].

pub(in crate::stream) mod blocks;
pub(in crate::stream) mod book;
pub(in crate::stream) mod colours;
mod fingerprint;
mod hash;
pub(in crate::stream) mod hooks;
pub(in crate::stream) mod jobs;
mod link;
mod mesher;
pub(in crate::stream) mod meshes;
mod sections;
pub(in crate::stream) mod shared;
mod snapshots;
mod stats;
pub(in crate::stream) mod writer;

use std::{collections::VecDeque, sync::Arc};

use hashbrown::{HashMap, HashSet};

use lod_store::{actor::StoreClient, minute::Minute, region_file::Access};
use world::ChunkKey;

use self::{
    blocks::SessionBlocks,
    fingerprint::Fingerprint,
    shared::{LodShared, Writers},
};
pub use self::{
    link::DistantTerrainLink,
    meshes::{
        DistantTerrainColours, DistantTerrainMeshStats, LOD_MESH_CACHE_BYTES, MAX_LOD_MESH_JOBS,
    },
    stats::DistantTerrainStats,
};
use super::{WorldStream, workers::WORKERS};

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
/// Distant terrain's CPU memory for decoded sections, store records and meshes.
pub const LOD_CPU_MEMORY_BYTES: usize = 128 << 20;
/// Bytes of decoded sections the writers keep cached, split evenly between them.
const LOD_SECTION_CACHE_BYTES: usize = 32 << 20;
/// Columns the column book holds before it is emptied at the next idle moment.
const MAX_LOD_BOOK_COLUMNS: usize = 1 << 16;
/// Most writer threads ingestion runs; fewer when the world pool has fewer background
/// threads, which the writers share the machine with.
pub const MAX_LOD_WRITERS: usize = 4;

// Dispatch keeps changed columns from overflowing the inbox.
const _: () = assert!(MAX_LOD_BACKLOG <= MAX_LOD_INBOX_COLUMNS);
// The writers' caches, the mesher's snapshots and the meshes share the CPU memory cap.
const _: () = assert!(
    LOD_SECTION_CACHE_BYTES + meshes::LOD_MESH_SNAPSHOT_BYTES + LOD_MESH_CACHE_BYTES
        <= LOD_CPU_MEMORY_BYTES
);

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
    /// A writer thread could not start.
    #[error("a distant terrain writer thread could not start: {0}")]
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
        let writers = WORKERS.size().background.min(MAX_LOD_WRITERS);
        self.enable_distant_terrain_with(client, now, Writers::new(writers))
    }

    /// [`Self::enable_distant_terrain`] with `writers` writer threads.
    pub(in crate::stream) fn enable_distant_terrain_with(
        &mut self,
        client: StoreClient,
        now: Minute,
        writers: Writers,
    ) -> Result<DistantTerrainLink, DistantTerrainUnavailable> {
        self.capture_final_columns(usize::MAX);
        self.disable_distant_terrain();
        if client.access() == Access::ReadOnly {
            return Err(DistantTerrainUnavailable::ReadOnly);
        }
        let shared = Arc::new(LodShared::new(client, now, writers));
        shared.set_meshes(self.lod_mesh_shared());
        writer::spawn(&shared).map_err(DistantTerrainUnavailable::WriterThread)?;
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

    /// Sets the minute stamped on later store requests and mesh reads; edge code reads the
    /// clock.
    pub fn set_distant_terrain_minute(&mut self, now: Minute) {
        if let Some(lod) = &self.lod {
            lod.shared.set_now(now);
        }
        self.set_distant_terrain_mesh_minute(now);
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
