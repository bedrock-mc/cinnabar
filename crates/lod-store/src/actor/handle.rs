//! The handle that owns the actor thread.

use std::{
    collections::HashMap,
    io,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, SyncSender, TrySendError},
    },
    thread::JoinHandle,
};

use foldhash::fast::FixedState;
use lod::geometry::SectionKey;

use super::{
    ActorConfig, ActorEvent, Closing, Command, ReadOutcome, ReadResult, ReadTarget, ReadTicket,
    Request, SessionEpoch, StoreClient, SubmitError, ViewChange, ViewEpoch, client, worker,
};
use crate::{
    identity::Dimension,
    minute::Minute,
    region_file::Access,
    root::{Quotas, StoreRoot},
    store::{SharedRegistry, Store},
};

/// The minute a closing handle passes to its thread when the request queue was full and
/// it closed the channel instead of queueing [`Command::Shutdown`].
#[derive(Debug, Default)]
pub(super) struct ShutdownTime(AtomicU64);

impl ShutdownTime {
    /// Records `now`, or that the handle was dropped without a time.
    pub(super) fn set(&self, now: Option<Minute>) {
        let encoded = now.map_or(0, |now| u64::from(now.get()) + 1);
        self.0.store(encoded, Ordering::Release);
    }

    /// The recorded minute; `None` when none was recorded.
    pub(super) fn get(&self) -> Option<Minute> {
        match self.0.load(Ordering::Acquire) {
            0 => None,
            encoded => u32::try_from(encoded - 1).ok().map(Minute::new),
        }
    }
}

/// Asks the thread behind `commands` to close the store at `now` without blocking. When
/// the queue is full the sender is dropped instead; the thread then serves what is
/// queued, sees the channel close, and closes the store with the time left in `time`.
pub(super) fn request_shutdown(
    commands: SyncSender<Command>,
    now: Option<Minute>,
    time: &ShutdownTime,
) {
    time.set(now);
    let _ = commands.try_send(Command::Shutdown { now });
}

/// The handle of a running write-back actor. Dropping it asks the thread to flush and
/// stop without waiting or any signal of when the store's lock is free; owners that may
/// open the same store again use [`Self::close`] and wait for [`Closing::is_finished`].
#[derive(Debug)]
pub struct StoreActor {
    pub(super) commands: Option<SyncSender<Command>>,
    pub(super) clients: Arc<client::ClientSlot>,
    pub(super) shutdown_time: Arc<ShutdownTime>,
    pub(super) results: Receiver<ReadResult>,
    pub(super) events: Receiver<ActorEvent>,
    pub(super) dropped_events: Arc<AtomicU64>,
    pub(super) outstanding: HashMap<ReadTicket, (Dimension, ReadTarget), FixedState>,
    pub(super) reads_in_flight: usize,
    pub(super) next_ticket: u64,
    pub(super) registry: SharedRegistry,
    pub(super) access: Access,
    pub(super) thread: Option<JoinHandle<()>>,
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
            hidden: clients.hidden(),
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

    /// The changes to what read-through reads show so far; see [`ViewEpoch`].
    #[must_use]
    pub fn view_epoch(&self) -> ViewEpoch {
        self.clients.view_epoch()
    }

    /// The current view epoch and what read-through reads show differently since `since`.
    #[must_use]
    pub fn view_changes(&self, since: ViewEpoch) -> (ViewEpoch, ViewChange) {
        self.clients.view_changes(since)
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

impl Drop for StoreActor {
    fn drop(&mut self) {
        self.clients.detach();
        if let Some(commands) = self.commands.take() {
            request_shutdown(commands, None, &self.shutdown_time);
        }
    }
}
