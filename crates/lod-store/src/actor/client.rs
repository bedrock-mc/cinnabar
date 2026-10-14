//! Requests and reads from threads other than the handle's owner.

use std::sync::{
    Arc, Mutex, PoisonError,
    atomic::{AtomicU64, Ordering},
    mpsc::{self, SyncSender, TrySendError},
};

use super::{
    Command, EpochWrite, ReadOutcome, ReadTarget, Request, SessionEpoch, SubmitError, ViewChange,
    ViewEpoch, view::HiddenRegions,
};
use crate::{identity::Dimension, minute::Minute, region_file::Access, store::SharedRegistry};

/// A count the handle, its clients and its thread share.
pub(super) type SharedCount = Arc<AtomicU64>;

/// The request sender clients share, cleared when the handle closes so that only the
/// handle's own sender decides when the thread's channel closes, the session change count
/// and the regions hidden from read-through.
#[derive(Debug)]
pub(super) struct ClientSlot {
    commands: Mutex<Option<SyncSender<Command>>>,
    changes: SharedCount,
    hidden: Arc<HiddenRegions>,
}

impl ClientSlot {
    /// A slot holding a clone of the handle's sender.
    pub(super) fn new(commands: SyncSender<Command>) -> Self {
        Self {
            commands: Mutex::new(Some(commands)),
            changes: SharedCount::default(),
            hidden: Arc::default(),
        }
    }

    /// Stops every client; later calls fail as stopped.
    pub(super) fn detach(&self) {
        self.commands
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
    }

    /// The session change count, for the thread to bump with the changes it decides.
    pub(super) fn changes(&self) -> SharedCount {
        Arc::clone(&self.changes)
    }

    /// The regions hidden from read-through, for the thread to publish before it answers
    /// reads that no longer see them.
    pub(super) fn hidden(&self) -> Arc<HiddenRegions> {
        Arc::clone(&self.hidden)
    }

    /// The session changes queued or decided so far.
    pub(super) fn session_epoch(&self) -> SessionEpoch {
        SessionEpoch(self.changes.load(Ordering::Acquire))
    }

    /// The changes to what read-through reads show so far; see [`ViewEpoch`].
    pub(super) fn view_epoch(&self) -> ViewEpoch {
        ViewEpoch {
            sessions: self.changes.load(Ordering::Acquire),
            leading: self.hidden.leading(),
            hidden: self.hidden.count(),
        }
    }

    /// The current view epoch and what changed since `since`; see [`ViewChange`].
    pub(super) fn view_changes(&self, since: ViewEpoch) -> (ViewEpoch, ViewChange) {
        self.hidden
            .changes(self.changes.load(Ordering::Acquire), since)
    }

    /// Counts a session change just queued.
    pub(super) fn bump_session_epoch(&self) {
        self.changes.fetch_add(1, Ordering::AcqRel);
    }

    /// Queues `command` without blocking; hands it back when full or detached.
    fn try_send(&self, command: Command) -> Result<(), TrySendError<Command>> {
        match &*self.commands.lock().unwrap_or_else(PoisonError::into_inner) {
            Some(commands) => commands.try_send(command),
            None => Err(TrySendError::Disconnected(command)),
        }
    }
}

/// A cloneable, thread-safe client of a running [`super::StoreActor`]. Submitting never
/// blocks; a read blocks its caller until the thread answers, after every write queued
/// before it. Every call fails as stopped once the handle closes or drops, so a client
/// never keeps the thread alive.
#[derive(Debug, Clone)]
pub struct StoreClient {
    slot: Arc<ClientSlot>,
    registry: SharedRegistry,
    access: Access,
}

impl StoreClient {
    /// A client sending through `slot`.
    pub(super) fn new(slot: Arc<ClientSlot>, registry: SharedRegistry, access: Access) -> Self {
        Self {
            slot,
            registry,
            access,
        }
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

    /// Queues `request` without blocking, as [`super::StoreActor::try_submit`].
    pub fn try_submit(&self, request: Request) -> Result<(), SubmitError<Request>> {
        let changes_session = request.changes_session();
        self.slot
            .try_send(Command::Request(request))
            .map_err(|error| match error {
                TrySendError::Full(Command::Request(request)) => SubmitError::Full(request),
                TrySendError::Disconnected(Command::Request(request)) => {
                    SubmitError::Stopped(request)
                }
                _ => unreachable!("a request was sent"),
            })?;
        if changes_session {
            self.slot.bump_session_epoch();
        }
        Ok(())
    }

    /// The session changes queued or decided so far. Load it before reading what a write
    /// is prepared from, and send the write with it ([`Self::write_in_epoch`]).
    #[must_use]
    pub fn session_epoch(&self) -> SessionEpoch {
        self.slot.session_epoch()
    }

    /// The changes to what [`ReadTarget::Section`] reads show other than by writes so far.
    /// Load it before reading, and read again once it moved on.
    #[must_use]
    pub fn view_epoch(&self) -> ViewEpoch {
        self.slot.view_epoch()
    }

    /// The current view epoch and what [`ReadTarget::Section`] reads show differently since
    /// `since`: nothing, only the sections overlapping some hidden regions, or anything.
    #[must_use]
    pub fn view_changes(&self, since: ViewEpoch) -> (ViewEpoch, ViewChange) {
        self.slot.view_changes(since)
    }

    /// Queues data `requests` (section puts and removes, chunk hashes and surfaces) to be
    /// written only if no session changed since `epoch`, without waiting for room, then
    /// waits for the thread's verdict, given once it has served every earlier request.
    /// A full queue or a stopped client hands the requests back; a thread that stops
    /// before answering yields [`SubmitError::Stopped`] without them.
    pub fn write_in_epoch(
        &self,
        epoch: SessionEpoch,
        requests: Vec<Request>,
    ) -> Result<EpochWrite, SubmitError<Vec<Request>>> {
        debug_assert!(requests.iter().all(Request::writes_data));
        let (reply, answer) = mpsc::sync_channel(1);
        let command = Command::EpochWrites {
            epoch,
            requests,
            reply,
        };
        match self.slot.try_send(command) {
            Ok(()) => answer.recv().map_err(|_| SubmitError::Stopped(Vec::new())),
            Err(TrySendError::Full(Command::EpochWrites { requests, .. })) => {
                Err(SubmitError::Full(requests))
            }
            Err(TrySendError::Disconnected(Command::EpochWrites { requests, .. })) => {
                Err(SubmitError::Stopped(requests))
            }
            Err(_) => unreachable!("epoch writes were sent"),
        }
    }

    /// Reads `target` in `dimension`'s session epoch, waiting for the answer. Fails without
    /// waiting when the request queue is full or the client stopped; a thread that stops
    /// before answering yields [`ReadOutcome::Failed`].
    pub fn read(
        &self,
        dimension: Dimension,
        target: ReadTarget,
        now: Minute,
    ) -> Result<ReadOutcome, SubmitError<()>> {
        let (reply, answer) = mpsc::sync_channel(1);
        let command = Command::ReadReply {
            dimension,
            target,
            now,
            reply,
        };
        match self.slot.try_send(command) {
            Ok(()) => Ok(answer
                .recv()
                .unwrap_or_else(|_| ReadOutcome::Failed("the store thread stopped".into()))),
            Err(TrySendError::Full(_)) => Err(SubmitError::Full(())),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Stopped(())),
        }
    }

    /// Reads every target of `targets` in `dimension`'s session epoch as one request,
    /// waiting for all answers, in order; each sees every write sent before the batch. A
    /// mesh job's snapshot reads its section and neighbours this way, in one round trip.
    /// Fails without waiting as [`Self::read`]; a thread that stops before answering
    /// yields [`ReadOutcome::Failed`] for every target.
    pub fn read_batch(
        &self,
        dimension: Dimension,
        targets: Vec<ReadTarget>,
        now: Minute,
    ) -> Result<Vec<ReadOutcome>, SubmitError<()>> {
        let count = targets.len();
        let (reply, answer) = mpsc::sync_channel(1);
        let command = Command::ReadBatchReply {
            dimension,
            targets,
            now,
            reply,
        };
        match self.slot.try_send(command) {
            Ok(()) => Ok(answer.recv().unwrap_or_else(|_| {
                vec![ReadOutcome::Failed("the store thread stopped".into()); count]
            })),
            Err(TrySendError::Full(_)) => Err(SubmitError::Full(())),
            Err(TrySendError::Disconnected(_)) => Err(SubmitError::Stopped(())),
        }
    }
}
