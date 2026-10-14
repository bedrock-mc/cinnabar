//! The write-back actor: one background thread that owns a [`Store`] and is its only
//! writer.
//!
//! Requests travel through a bounded channel and never block the sender: a full channel
//! hands the request back. The thread drains requests in batches and coalesces writes per
//! key, chunk and surface, so the last write of a batch wins. Reads of section records and
//! chunk content hashes see every write sent before them and are answered through a
//! bounded result queue; each read takes a slot until its result is polled, and every
//! read gets exactly one result, a failure included, even when the thread stops. Flushes
//! run when the caller sends [`Request::Flush`] on its periodic cadence, each compacting
//! within a byte budget so queued reads wait little; syncs to disk when it sends
//! [`Request::Sync`] as the client goes idle; and quota enforcement, which walks every
//! store, when it sends [`Request::EnforceQuotas`], each with the caller's time. Closing
//! flushes, enforces the quotas and syncs; [`Closing`] reports when the store's lock is
//! released, so the same store can be opened again as its writer. Neither closing nor
//! dropping the handle ever blocks its thread. A [`StoreClient`] lets background threads
//! submit requests and wait for reads of their own until the handle closes.
//!
//! Every session change bumps a [`SessionEpoch`]: a begun or ended session as soon as its
//! request is queued, and a candidate that merges, becomes established or is dropped when
//! the thread decides it. Writes prepared from what one epoch held are sent with it
//! ([`StoreClient::write_in_epoch`]) and refused when the thread has seen a change since, so
//! terrain merged from one epoch's sections never lands in another.

mod client;
mod closing;
#[cfg(test)]
mod tests;
mod worker;

use std::{
    collections::HashMap,
    io,
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
};

use foldhash::fast::FixedState;

pub use self::{client::StoreClient, closing::Closing};
use lod::{epoch::ChunkSurface, geometry::SectionKey};

use crate::{
    error::StoreError,
    identity::Dimension,
    minute::Minute,
    region_file::Access,
    root::{QuotaReport, Quotas, StoreRoot},
    store::{Focus, SharedRegistry, Store, StoreEvent},
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
pub struct ReadTicket(u64);

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
enum Command {
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
        resume: Receiver<()>,
    },
    /// A [`StoreClient`] read answered on its own channel.
    ReadReply {
        dimension: Dimension,
        target: ReadTarget,
        now: Minute,
        reply: SyncSender<ReadOutcome>,
    },
    Shutdown {
        now: Option<Minute>,
    },
}

/// The minute a closing handle passes to its thread when the request queue was full and
/// it closed the channel instead of queueing [`Command::Shutdown`].
#[derive(Debug, Default)]
struct ShutdownTime(AtomicU64);

impl ShutdownTime {
    /// Records `now`, or that the handle was dropped without a time.
    fn set(&self, now: Option<Minute>) {
        let encoded = now.map_or(0, |now| u64::from(now.get()) + 1);
        self.0.store(encoded, Ordering::Release);
    }

    /// The recorded minute; `None` when none was recorded.
    fn get(&self) -> Option<Minute> {
        match self.0.load(Ordering::Acquire) {
            0 => None,
            encoded => u32::try_from(encoded - 1).ok().map(Minute::new),
        }
    }
}

/// Asks the thread behind `commands` to close the store at `now` without blocking. When
/// the queue is full the sender is dropped instead; the thread then serves what is
/// queued, sees the channel close, and closes the store with the time left in `time`.
fn request_shutdown(commands: SyncSender<Command>, now: Option<Minute>, time: &ShutdownTime) {
    time.set(now);
    let _ = commands.try_send(Command::Shutdown { now });
}

/// The handle of a running write-back actor. Dropping it asks the thread to flush and
/// stop without waiting or any signal of when the store's lock is free; owners that may
/// open the same store again use [`Self::close`] and wait for [`Closing::is_finished`].
#[derive(Debug)]
pub struct StoreActor {
    commands: Option<SyncSender<Command>>,
    clients: Arc<client::ClientSlot>,
    shutdown_time: Arc<ShutdownTime>,
    results: Receiver<ReadResult>,
    events: Receiver<ActorEvent>,
    dropped_events: Arc<AtomicU64>,
    outstanding: HashMap<ReadTicket, (Dimension, ReadTarget), FixedState>,
    reads_in_flight: usize,
    next_ticket: u64,
    registry: SharedRegistry,
    access: Access,
    thread: Option<JoinHandle<()>>,
}

impl StoreActor {
    /// Starts the thread that owns `store`. `root` and `quotas` apply on each flush.
    pub fn spawn(
        store: Store,
        root: StoreRoot,
        quotas: Quotas,
        config: ActorConfig,
    ) -> io::Result<Self> {
        let (commands, command_rx) = mpsc::sync_channel(config.requests.get());
        let (result_tx, results) = mpsc::sync_channel(config.reads_in_flight.get());
        let (event_tx, events) = mpsc::sync_channel(config.events.get());
        let dropped_events = Arc::new(AtomicU64::new(0));
        let shutdown_time = Arc::new(ShutdownTime::default());
        let (registry, access) = (store.registry(), store.access());
        let clients = Arc::new(client::ClientSlot::new(commands.clone()));
        let worker = worker::Worker {
            changes: clients.changes(),
            seen: SessionEpoch(0),
            store: Some(store),
            root,
            quotas,
            commands: command_rx,
            results: result_tx,
            events: event_tx,
            dropped_events: Arc::clone(&dropped_events),
            shutdown_time: Arc::clone(&shutdown_time),
            batch: config.batch.get(),
            unwritten: Default::default(),
        };
        let thread = std::thread::Builder::new()
            .name("lod-store".into())
            .spawn(move || worker.run())?;
        Ok(Self {
            commands: Some(commands),
            clients,
            shutdown_time,
            results,
            events,
            dropped_events,
            outstanding: HashMap::default(),
            reads_in_flight: config.reads_in_flight.get(),
            next_ticket: 0,
            registry,
            access,
            thread: Some(thread),
        })
    }

    /// The store's material registry.
    #[must_use]
    pub fn registry(&self) -> SharedRegistry {
        Arc::clone(&self.registry)
    }

    /// Whether the store may write; a read-only store fails every write request.
    #[must_use]
    pub fn access(&self) -> Access {
        self.access
    }

    /// A client for other threads, which stops working once this handle closes or drops.
    #[must_use]
    pub fn client(&self) -> StoreClient {
        StoreClient::new(Arc::clone(&self.clients), self.registry(), self.access)
    }

    /// Queues `request` without blocking. A queued begin or end bumps the
    /// [`SessionEpoch`].
    pub fn try_submit(&mut self, request: Request) -> Result<(), SubmitError<Request>> {
        let Some(commands) = &self.commands else {
            return Err(SubmitError::Stopped(request));
        };
        let changes_session = request.changes_session();
        commands
            .try_send(Command::Request(request))
            .map_err(|error| match error {
                TrySendError::Full(Command::Request(request)) => SubmitError::Full(request),
                TrySendError::Disconnected(Command::Request(request)) => {
                    SubmitError::Stopped(request)
                }
                _ => unreachable!("a request was sent"),
            })?;
        if changes_session {
            self.clients.bump_session_epoch();
        }
        Ok(())
    }

    /// The session changes queued or decided so far.
    #[must_use]
    pub fn session_epoch(&self) -> SessionEpoch {
        self.clients.session_epoch()
    }

    /// Holds the thread once it has served everything queued before, until the returned
    /// guard drops, so tests can fill its queue. Blocks until the thread holds.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pause(&mut self) -> Paused {
        let (paused, acknowledged) = mpsc::sync_channel(1);
        let (resume, held) = mpsc::sync_channel(1);
        if let Some(commands) = &self.commands {
            commands
                .send(Command::Pause {
                    paused,
                    resume: held,
                })
                .expect("the thread runs");
            acknowledged.recv().expect("the thread pauses");
        }
        Paused { _resume: resume }
    }

    /// Queues a read of `key` in `dimension`'s session epoch without blocking. Fails when
    /// every read slot awaits polling or the queue is full.
    pub fn try_read(
        &mut self,
        dimension: Dimension,
        key: SectionKey,
        now: Minute,
    ) -> Result<ReadTicket, SubmitError<()>> {
        self.try_read_target(dimension, ReadTarget::Section(key), now)
    }

    /// Queues a read of chunk `(x, z)`'s content hash in `dimension`'s session epoch, as
    /// [`Self::try_read`].
    pub fn try_read_chunk_hash(
        &mut self,
        dimension: Dimension,
        x: i32,
        z: i32,
        now: Minute,
    ) -> Result<ReadTicket, SubmitError<()>> {
        self.try_read_target(dimension, ReadTarget::ChunkHash { x, z }, now)
    }

    /// Queues a read of `target` without blocking, taking a read slot until its result
    /// is polled.
    fn try_read_target(
        &mut self,
        dimension: Dimension,
        target: ReadTarget,
        now: Minute,
    ) -> Result<ReadTicket, SubmitError<()>> {
        let Some(commands) = &self.commands else {
            return Err(SubmitError::Stopped(()));
        };
        if self.outstanding.len() >= self.reads_in_flight {
            return Err(SubmitError::Full(()));
        }
        let ticket = ReadTicket(self.next_ticket);
        let command = Command::Read {
            ticket,
            dimension,
            target,
            now,
        };
        match commands.try_send(command) {
            Ok(()) => {
                self.next_ticket += 1;
                self.outstanding.insert(ticket, (dimension, target));
                Ok(ticket)
            }
            Err(TrySendError::Full(_)) => Err(SubmitError::Full(())),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Stopped(())),
        }
    }

    /// Reads still awaiting their results.
    #[must_use]
    pub fn reads_outstanding(&self) -> usize {
        self.outstanding.len()
    }

    /// Moves every available read result into `out`. When the thread has stopped, every
    /// read still outstanding completes as [`ReadOutcome::Failed`].
    pub fn poll_results(&mut self, out: &mut Vec<ReadResult>) {
        loop {
            match self.results.try_recv() {
                Ok(result) => {
                    self.outstanding.remove(&result.ticket);
                    out.push(result);
                }
                Err(mpsc::TryRecvError::Empty) => return,
                Err(mpsc::TryRecvError::Disconnected) => break,
            }
        }
        out.extend(
            self.outstanding
                .drain()
                .map(|(ticket, (dimension, target))| ReadResult {
                    ticket,
                    dimension,
                    target,
                    outcome: ReadOutcome::Failed("the store thread stopped".into()),
                }),
        );
    }

    /// Moves every queued event into `out`.
    pub fn poll_events(&mut self, out: &mut Vec<ActorEvent>) {
        out.extend(self.events.try_iter());
    }

    /// Events dropped because the event queue was full.
    #[must_use]
    pub fn dropped_events(&self) -> u64 {
        self.dropped_events.load(Ordering::Relaxed)
    }

    /// Asks the thread to flush, enforce the quotas, sync and close the store, without
    /// waiting, even while the request queue is full. Outstanding reads are answered
    /// first; their results are dropped.
    pub fn close(mut self, now: Minute) -> Closing {
        self.clients.detach();
        if let Some(commands) = self.commands.take() {
            request_shutdown(commands, Some(now), &self.shutdown_time);
        }
        Closing {
            thread: self.thread.take(),
            events: std::mem::replace(&mut self.events, mpsc::sync_channel(0).1),
        }
    }

    /// [`Self::close`], then waits for the thread and returns the events not yet polled.
    pub fn shutdown(self, now: Minute) -> Vec<ActorEvent> {
        self.close(now).wait()
    }
}

/// Keeps the thread paused by [`StoreActor::pause`] until dropped.
#[cfg(any(test, feature = "test-support"))]
#[derive(Debug)]
pub struct Paused {
    _resume: SyncSender<()>,
}

impl Request {
    /// True for requests that begin or end a session.
    fn changes_session(&self) -> bool {
        matches!(self, Self::Begin { .. } | Self::End { .. })
    }

    /// True for requests that only write data: sections, chunk hashes and surfaces.
    fn writes_data(&self) -> bool {
        matches!(
            self,
            Self::Put { .. } | Self::Remove { .. } | Self::ChunkHash { .. } | Self::Surface { .. }
        )
    }
}

impl Drop for StoreActor {
    fn drop(&mut self) {
        self.clients.detach();
        if let Some(commands) = self.commands.take() {
            request_shutdown(commands, None, &self.shutdown_time);
        }
    }
}
