//! The actor thread: batches, coalescing and the order writes reach the store.

use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{Receiver, SyncSender},
    },
};

use lod::{
    epoch::ChunkSurface,
    geometry::{LEVEL_COUNT, SectionKey},
};

use super::{
    ActorEvent, Command, EpochWrite, ReadOutcome, ReadResult, ReadTarget, ReadTicket, Request,
    SessionEpoch, ShutdownTime, client::SessionChanges,
};
use crate::{
    error::{RegionError, StoreError},
    identity::Dimension,
    minute::Minute,
    root::{Quotas, StoreRoot},
    store::Store,
};

/// The thread's state.
pub(super) struct Worker {
    /// `None` once closed.
    pub(super) store: Option<Store>,
    pub(super) root: StoreRoot,
    pub(super) quotas: Quotas,
    pub(super) commands: Receiver<Command>,
    pub(super) results: SyncSender<ReadResult>,
    pub(super) events: SyncSender<ActorEvent>,
    pub(super) dropped_events: Arc<AtomicU64>,
    /// The close time of a handle that closed the channel instead of queueing a shutdown.
    pub(super) shutdown_time: Arc<ShutdownTime>,
    pub(super) batch: usize,
    /// Footprints of section writes that failed in each dimension's current session; a
    /// chunk hash inside one is stored cleared, so it never vouches for a lost section.
    pub(super) unwritten: HashSet<Footprint>,
    /// The count submitters bump as they queue a begin or end, and this thread as it
    /// decides a change.
    pub(super) changes: SessionChanges,
    /// The session changes this thread has served or decided, in queue order: what an
    /// epoch write sent behind them must have been prepared in.
    pub(super) seen: SessionEpoch,
}

/// The chunk columns one section covers: its dimension, level and section `x`, `z`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct Footprint {
    dimension: Dimension,
    level: u8,
    x: i32,
    z: i32,
}

impl Footprint {
    /// The columns under `key` in `dimension`.
    fn of(dimension: Dimension, key: SectionKey) -> Self {
        let pos = key.pos();
        Self {
            dimension,
            level: pos.level(),
            x: pos.x(),
            z: pos.z(),
        }
    }

    /// The footprint at `level` holding chunk `(x, z)` of `dimension`; a level-0 section
    /// spans two chunks a side and each level doubles that.
    fn containing(dimension: Dimension, level: u8, x: i32, z: i32) -> Self {
        let at_level = |chunk: i32| chunk.div_euclid(2) >> level;
        Self {
            dimension,
            level,
            x: at_level(x),
            z: at_level(z),
        }
    }
}

/// A section write after coalescing.
enum SectionWrite {
    Put(Arc<[u8]>, Minute),
    Remove,
}

/// Where a read's answer goes.
enum Answer {
    /// The handle's result queue.
    Ticket(ReadTicket),
    /// A client's own channel.
    Reply(SyncSender<ReadOutcome>),
}

/// A read waiting for the writes before it.
struct PendingRead {
    answer: Answer,
    dimension: Dimension,
    target: ReadTarget,
    now: Minute,
}

/// Writes of one batch segment, the last per key, chunk and surface winning.
#[derive(Default)]
struct Coalesced {
    sections: BTreeMap<(Dimension, SectionKey), SectionWrite>,
    hashes: BTreeMap<(Dimension, i32, i32), Option<u64>>,
    surfaces: BTreeMap<(Dimension, i32, i32), (Box<ChunkSurface>, Minute)>,
    reads: Vec<PendingRead>,
}

impl Worker {
    /// Serves commands until shutdown or until the handle is gone, then closes the
    /// store with the shutdown's time, else the time the handle left, else the last time
    /// a request gave; with no time at all the quotas are not enforced.
    pub(super) fn run(mut self) {
        let mut pending = Coalesced::default();
        let mut last_now = None;
        while let Ok(first) = self.commands.recv() {
            let mut batch = vec![first];
            batch.extend(self.commands.try_iter().take(self.batch - 1));
            for command in batch {
                match command {
                    Command::Request(request) => {
                        self.request(request, &mut pending, &mut last_now);
                    }
                    Command::Read {
                        ticket,
                        dimension,
                        target,
                        now,
                    } => pending.reads.push(PendingRead {
                        answer: Answer::Ticket(ticket),
                        dimension,
                        target,
                        now,
                    }),
                    Command::EpochWrites {
                        epoch,
                        requests,
                        reply,
                    } => {
                        let verdict = if epoch == self.seen {
                            for request in requests {
                                self.request(request, &mut pending, &mut last_now);
                            }
                            EpochWrite::Taken
                        } else {
                            EpochWrite::Stale
                        };
                        // Never blocks: each reply channel holds its one answer.
                        let _ = reply.send(verdict);
                    }
                    #[cfg(any(test, feature = "test-support"))]
                    Command::Pause { paused, resume } => {
                        self.apply(&mut pending);
                        let _ = paused.send(());
                        let _ = resume.recv();
                    }
                    Command::ReadReply {
                        dimension,
                        target,
                        now,
                        reply,
                    } => pending.reads.push(PendingRead {
                        answer: Answer::Reply(reply),
                        dimension,
                        target,
                        now,
                    }),
                    Command::Shutdown { now } => {
                        self.apply(&mut pending);
                        self.close(now.or(last_now));
                        return;
                    }
                }
            }
            self.apply(&mut pending);
            self.forward_store_events();
        }
        self.apply(&mut pending);
        self.close(self.shutdown_time.get().or(last_now));
    }

    /// Coalesces a write, or applies earlier writes and then a control request.
    fn request(
        &mut self,
        request: Request,
        pending: &mut Coalesced,
        last_now: &mut Option<Minute>,
    ) {
        match request {
            Request::Put {
                dimension,
                record,
                now,
            } => match lod::codec::RecordHeader::read(&record) {
                Ok(header) => {
                    pending
                        .sections
                        .insert((dimension, header.key), SectionWrite::Put(record, now));
                }
                Err(error) => self.failed("put", RegionError::Record(error).into()),
            },
            Request::Remove { dimension, key } => {
                pending
                    .sections
                    .insert((dimension, key), SectionWrite::Remove);
            }
            Request::ChunkHash {
                dimension,
                x,
                z,
                hash,
            } => {
                pending.hashes.insert((dimension, x, z), hash);
            }
            Request::Surface {
                dimension,
                chunk_x,
                chunk_z,
                surface,
                now,
            } => {
                pending
                    .surfaces
                    .insert((dimension, chunk_x, chunk_z), (surface, now));
            }
            Request::Begin { dimension, now } => {
                *last_now = Some(now);
                self.apply(pending);
                self.forget_unwritten(dimension);
                let result = self.store().begin_session(dimension, now);
                self.seen.0 += 1;
                if let Err(error) = result {
                    self.failed("begin session", error);
                }
            }
            Request::End { dimension } => {
                self.apply(pending);
                self.forget_unwritten(dimension);
                self.store().end_session(dimension);
                self.seen.0 += 1;
            }
            Request::Flush { now } => {
                *last_now = Some(now);
                self.apply(pending);
                self.store().flush();
                self.forward_store_events();
            }
            Request::Sync { now } => {
                *last_now = Some(now);
                self.apply(pending);
                self.store().sync();
                self.forward_store_events();
            }
            Request::EnforceQuotas { now, focus } => {
                *last_now = Some(now);
                self.apply(pending);
                let store = self.store.as_mut().expect("open until shutdown");
                store.flush();
                let report = self.root.enforce(self.quotas, now, Some(store), focus);
                self.forward_store_events();
                match report {
                    Ok(report) => self.send_event(ActorEvent::Quota(report)),
                    Err(error) => self.failed("quota", error),
                }
            }
        }
    }

    /// Forgets the failed section writes of `dimension`'s session, which is ending or
    /// beginning again.
    fn forget_unwritten(&mut self, dimension: Dimension) {
        self.unwritten
            .retain(|footprint| footprint.dimension != dimension);
    }

    /// Applies coalesced writes in order (records, then hashes, then surfaces, whose
    /// votes may merge the records just written) and answers the reads behind them. A
    /// chunk hash under a section whose write failed this session is stored cleared.
    fn apply(&mut self, pending: &mut Coalesced) {
        for ((dimension, key), write) in std::mem::take(&mut pending.sections) {
            let result = match write {
                // The header was read when the request was coalesced.
                SectionWrite::Put(record, now) => {
                    let result = self.store().put_record(dimension, key, &record, now);
                    if result.is_err() {
                        self.unwritten.insert(Footprint::of(dimension, key));
                    }
                    result
                }
                SectionWrite::Remove => self.store().remove(dimension, key).map(drop),
            };
            if let Err(error) = result {
                self.failed("write section", error);
            }
        }
        for ((dimension, x, z), hash) in std::mem::take(&mut pending.hashes) {
            let unwritten = !self.unwritten.is_empty()
                && (0..LEVEL_COUNT as u8).any(|level| {
                    self.unwritten
                        .contains(&Footprint::containing(dimension, level, x, z))
                });
            let hash = hash.filter(|_| !unwritten);
            if let Err(error) = self.store().set_chunk_hash(dimension, x, z, hash) {
                self.failed("chunk hash", error);
            }
        }
        for ((dimension, x, z), (surface, now)) in std::mem::take(&mut pending.surfaces) {
            let before = self.store().epoch_status(dimension);
            match self.store().record_surface(dimension, x, z, *surface, now) {
                Ok(status) if Some(status) != before => self.decided_session_change(),
                Ok(_) => {}
                Err(error) => {
                    if self.store().epoch_status(dimension) != before {
                        self.decided_session_change();
                    }
                    self.failed("surface", error);
                }
            }
        }
        let mut record = Vec::new();
        for read in pending.reads.drain(..) {
            let store = self.store.as_mut().expect("open until shutdown");
            let outcome = match read.target {
                ReadTarget::Section(key) | ReadTarget::OwnSection(key) => {
                    let found = if matches!(read.target, ReadTarget::Section(_)) {
                        store.get_into(read.dimension, key, &mut record, read.now)
                    } else {
                        store.get_own_into(read.dimension, key, &mut record, read.now)
                    };
                    match found {
                        Ok(true) => ReadOutcome::Found(std::mem::take(&mut record)),
                        Ok(false) => ReadOutcome::Missing,
                        Err(StoreError::Region(RegionError::Corrupt { .. })) => {
                            ReadOutcome::Corrupt
                        }
                        Err(error) => ReadOutcome::Failed(error.to_string()),
                    }
                }
                ReadTarget::ChunkHash { x, z } => match store.chunk_hash(read.dimension, x, z) {
                    Ok(hash) => ReadOutcome::ChunkHash(hash),
                    Err(error) => ReadOutcome::Failed(error.to_string()),
                },
            };
            match read.answer {
                // Never blocks: the handle keeps reads in flight within the queue's capacity.
                Answer::Ticket(ticket) => {
                    let _ = self.results.send(ReadResult {
                        ticket,
                        dimension: read.dimension,
                        target: read.target,
                        outcome,
                    });
                }
                // Never blocks: each reply channel holds its one answer.
                Answer::Reply(reply) => {
                    let _ = reply.send(outcome);
                }
            }
        }
    }

    /// Counts a session change this thread decided: a candidate merged or established.
    fn decided_session_change(&mut self) {
        self.seen.0 += 1;
        self.changes.fetch_add(1, Ordering::AcqRel);
    }

    /// Closes the store and forwards its last events.
    fn close(&mut self, now: Option<Minute>) {
        let Some(mut store) = self.store.take() else {
            return;
        };
        if let Some(now) = now {
            store.flush();
            if let Err(error) = self.root.enforce(self.quotas, now, Some(&mut store), None) {
                self.failed("quota", error);
            }
        }
        for event in store.close() {
            self.send_event(ActorEvent::Store(event));
        }
    }

    /// The open store; only called before shutdown closes it.
    fn store(&mut self) -> &mut Store {
        self.store.as_mut().expect("open until shutdown")
    }

    /// Moves the store's pending events into the event queue.
    fn forward_store_events(&mut self) {
        let events = self.store().drain_events();
        for event in events {
            self.send_event(ActorEvent::Store(event));
        }
    }

    /// Reports that `request` failed with `error`; its write is lost.
    fn failed(&self, request: &'static str, error: StoreError) {
        self.send_event(ActorEvent::RequestFailed { request, error });
    }

    /// Queues an event, counting it as dropped when the queue is full.
    fn send_event(&self, event: ActorEvent) {
        if self.events.try_send(event).is_err() {
            self.dropped_events.fetch_add(1, Ordering::Relaxed);
        }
    }
}
