//! What the handle, its clients and the thread send each other.

use std::{
    num::NonZeroUsize,
    sync::{Arc, mpsc::SyncSender},
};

use lod::{epoch::ChunkSurface, geometry::SectionKey};

use crate::{
    error::StoreError,
    identity::Dimension,
    minute::Minute,
    root::{QuotaReport, Quotas},
    store::{Focus, StoreEvent},
};

/// Bounds of one actor's queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorConfig {
    /// Requests and reads waiting for the thread.
    pub requests: NonZeroUsize,
    /// Reads sent but not yet polled; the result queue's capacity.
    pub reads_in_flight: NonZeroUsize,
    /// Events waiting to be polled; more are counted and dropped.
    pub events: NonZeroUsize,
    /// Requests the thread drains and coalesces at once.
    pub batch: NonZeroUsize,
}

impl Default for ActorConfig {
    fn default() -> Self {
        Self {
            requests: NonZeroUsize::new(1024).expect("non-zero"),
            reads_in_flight: NonZeroUsize::new(256).expect("non-zero"),
            events: NonZeroUsize::new(256).expect("non-zero"),
            batch: NonZeroUsize::new(256).expect("non-zero"),
        }
    }
}

/// A write or control request.
#[derive(Debug, Clone)]
pub enum Request {
    /// Begins a session in a dimension; see [`Store::begin_session`].
    Begin { dimension: Dimension, now: Minute },
    /// Ends a dimension's session; see [`Store::end_session`].
    End { dimension: Dimension },
    /// Stores a section record.
    Put {
        dimension: Dimension,
        record: Arc<[u8]>,
        now: Minute,
    },
    /// Removes a section record.
    Remove {
        dimension: Dimension,
        key: SectionKey,
    },
    /// Stores or clears a chunk's content hash.
    ChunkHash {
        dimension: Dimension,
        x: i32,
        z: i32,
        hash: Option<u64>,
    },
    /// Records a chunk's surface for epoch voting.
    Surface {
        dimension: Dimension,
        chunk_x: i32,
        chunk_z: i32,
        surface: Box<ChunkSurface>,
        now: Minute,
    },
    /// Commits everything written so far; see [`Store::flush`].
    Flush { now: Minute },
    /// Commits everything written so far and flushes it to disk, for an idle client;
    /// see [`Store::sync`].
    Sync { now: Minute },
    /// Flushes, then applies the quotas around `focus`.
    EnforceQuotas { now: Minute, focus: Option<Focus> },
    /// Replaces the quotas later flushes, quota passes and the close apply.
    SetQuotas(Quotas),
}

/// Why a request was not queued; it is handed back for a later retry.
#[derive(Debug)]
pub enum SubmitError<T> {
    /// The queue, or the read slots, are full.
    Full(T),
    /// The thread has stopped.
    Stopped(T),
}

/// How many session changes a store has seen: a count that only grows. See the module
/// documentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionEpoch(pub u64);

/// What the thread did with writes sent in a [`SessionEpoch`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpochWrite {
    /// Queued behind every earlier write, in the epoch they were prepared in.
    Taken,
    /// Refused: a session changed after they were prepared; nothing was written.
    Stale,
}

/// Identifies one read's result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadTicket(pub(super) u64);

/// What one read asks for, in a dimension's session epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadTarget {
    /// A section record, read through to a stored epoch while the session is undecided.
    Section(SectionKey),
    /// A section record of the session's own epoch only, as ingestion merges into it, so
    /// an undecided candidate never copies a stored epoch's terrain into its own.
    OwnSection(SectionKey),
    /// A level-0 chunk's content hash, which ingestion compares before sampling.
    ChunkHash { x: i32, z: i32 },
}

/// The answer to one read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadResult {
    pub ticket: ReadTicket,
    pub dimension: Dimension,
    pub target: ReadTarget,
    pub outcome: ReadOutcome,
}

/// What a read found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// The stored record.
    Found(Vec<u8>),
    /// The stored chunk hash, `None` when none is stored.
    ChunkHash(Option<u64>),
    /// No record is stored for the key.
    Missing,
    /// The stored record is corrupt, and the hashes of the chunks it covered are cleared;
    /// the caller rebuilds the section.
    Corrupt,
    /// The read failed, or the thread stopped before answering.
    Failed(String),
}

/// Something the thread reports.
#[derive(Debug)]
pub enum ActorEvent {
    Store(StoreEvent),
    Quota(QuotaReport),
    /// A request failed; its write is lost and is sampled again later.
    RequestFailed {
        request: &'static str,
        error: StoreError,
    },
}

/// What the thread receives.
#[derive(Debug)]
pub(super) enum Command {
    Request(Request),
    Read {
        ticket: ReadTicket,
        dimension: Dimension,
        target: ReadTarget,
        now: Minute,
    },
    /// Writes applied only in the session epoch they were prepared in; see
    /// [`StoreClient::write_in_epoch`].
    EpochWrites {
        epoch: SessionEpoch,
        requests: Vec<Request>,
        reply: SyncSender<EpochWrite>,
    },
    /// Holds the thread until `resume` disconnects, after acknowledging on `paused`.
    #[cfg(any(test, feature = "test-support"))]
    Pause {
        paused: SyncSender<()>,
        resume: std::sync::mpsc::Receiver<()>,
    },
    /// A [`StoreClient`] read answered on its own channel.
    ReadReply {
        dimension: Dimension,
        target: ReadTarget,
        now: Minute,
        reply: SyncSender<ReadOutcome>,
    },
    /// A [`super::StoreClient`] batch of reads answered together on its own channel, in
    /// order.
    ReadBatchReply {
        dimension: Dimension,
        targets: Vec<ReadTarget>,
        now: Minute,
        reply: SyncSender<Vec<ReadOutcome>>,
    },
    Shutdown {
        now: Option<Minute>,
    },
}

impl Request {
    /// True for requests that begin or end a session.
    pub(super) fn changes_session(&self) -> bool {
        matches!(self, Self::Begin { .. } | Self::End { .. })
    }

    /// True for requests that only write data: sections, chunk hashes and surfaces.
    pub(super) fn writes_data(&self) -> bool {
        matches!(
            self,
            Self::Put { .. } | Self::Remove { .. } | Self::ChunkHash { .. } | Self::Surface { .. }
        )
    }
}
