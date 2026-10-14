//! Opening, driving and closing one store.

use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::Duration,
};

use bevy::log::{debug, warn};
use chunk_pipeline::{DistantTerrainLink, DistantTerrainStats, DistantTerrainUnavailable};
use lod_store::{
    actor::{ActorConfig, ActorEvent, Request, StoreActor, SubmitError},
    identity::{Dimension, StoreKey},
    minute::Minute,
    root::{Quotas, StoreRoot},
    store::{Focus, StoreEvent, StoreOptions},
};

use super::{
    DimensionSessions, DistantTerrainFrame, DistantTerrainReport, DistantTerrainSessions,
    DistantTerrainTarget, FINISH_LIMIT, FLUSH_INTERVAL_MINUTES, Ingest, OpenStore,
    QUOTA_INTERVAL_MINUTES, QUOTA_SETTLE_MINUTES, State,
};

/// A store waiting for the final captures of the streams that wrote into it.
pub(super) struct Finishing {
    actor: StoreActor,
    links: Vec<DistantTerrainLink>,
    /// The uptime it started waiting at.
    since: Duration,
}

impl DistantTerrainSessions {
    /// Closes the stores whose streams' final captures are written, or that have waited
    /// [`FINISH_LIMIT`] by `uptime`, at minute `now`; the links of a store closed early
    /// drop what was still queued.
    pub(super) fn poll_finishing(&mut self, now: Minute, uptime: Duration) {
        let mut index = 0;
        while index < self.finishing.len() {
            let finishing = &mut self.finishing[index];
            finishing.links.retain(|link| !link.poll_written());
            if finishing.links.is_empty() || uptime.saturating_sub(finishing.since) >= FINISH_LIMIT
            {
                let Finishing { actor, links, .. } = self.finishing.swap_remove(index);
                drop(links);
                self.dropped_closed += actor.dropped_events();
                self.closing.push(actor.close(now));
            } else {
                index += 1;
            }
        }
    }

    /// Counts the events of closing stores and forgets those that released their locks.
    pub(super) fn poll_closing(&mut self) {
        let mut events = Vec::new();
        for closing in &mut self.closing {
            closing.poll_events(&mut events);
        }
        self.closing.retain(|closing| !closing.is_finished());
        for event in events {
            count_event(event, &mut self.report);
        }
    }

    /// Moves toward the store `frame` wants: finishes an open in progress, closes a store
    /// of another target, and opens the wanted one once earlier stores have closed, so
    /// it takes the writer lock again.
    pub(super) fn reconcile(&mut self, frame: &mut DistantTerrainFrame<'_>) {
        let wanted = frame.target.as_ref();
        if let State::Opening {
            target,
            opened,
            quotas,
        } = &self.state
        {
            let quotas = *quotas;
            let result = match opened.try_recv() {
                Ok(result) => result,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err("the opening thread stopped".to_owned()),
            };
            let target = target.clone();
            self.state = match result {
                Ok(actor) if wanted == Some(&target) => {
                    State::Open(Box::new(OpenStore::new(target, actor, frame.now, quotas)))
                }
                Ok(actor) => {
                    self.closing.push(actor.close(frame.now));
                    State::Off
                }
                Err(error) => {
                    warn!(%error, "distant terrain store could not be opened");
                    self.report.store_errors += 1;
                    State::Failed(target)
                }
            };
        }
        let stale = match &self.state {
            State::Open(open) => wanted != Some(&open.target),
            State::Failed(target) => wanted != Some(target),
            State::Off | State::Opening { .. } => false,
        };
        if stale {
            self.close(frame.stream.as_deref_mut(), frame.uptime);
        }
        if let (State::Off, Some(target)) = (&self.state, wanted)
            && !self.is_closing()
        {
            self.state = State::Opening {
                target: target.clone(),
                quotas: self.quotas,
                opened: spawn_open(
                    self.root.clone(),
                    self.quotas,
                    target.key.clone(),
                    frame.now,
                ),
            };
        }
    }
}

/// Opens `key`'s store and starts its thread on a thread of its own, so the frame never
/// waits for the disk.
fn spawn_open(
    root: StoreRoot,
    quotas: Quotas,
    key: StoreKey,
    now: Minute,
) -> Receiver<Result<StoreActor, String>> {
    let (sender, opened) = mpsc::sync_channel(1);
    let spawned = std::thread::Builder::new()
        .name("lod-store-open".to_owned())
        .spawn(move || {
            let result = root
                .open(&key, now, StoreOptions::default())
                .map_err(|error| error.to_string())
                .and_then(|store| {
                    StoreActor::spawn(store, root, quotas, ActorConfig::default())
                        .map_err(|error| error.to_string())
                });
            // An abandoned open drops its actor, which closes the store.
            let _ = sender.send(result);
        });
    if let Err(error) = spawned {
        // The sender went with the closure, so the open reads as failed.
        warn!(%error, "distant terrain store thread unavailable");
    }
    opened
}

impl OpenStore {
    fn new(target: DistantTerrainTarget, actor: StoreActor, now: Minute, quotas: Quotas) -> Self {
        Self {
            target,
            actor,
            ingest: Ingest::Writable,
            links: Vec::new(),
            sessions: DimensionSessions::default(),
            pending: Default::default(),
            last_flush: now,
            last_quotas: now,
            synced_sections: 0,
            invalidate_due: false,
            quotas,
            settling: None,
            quota_pass_due: false,
        }
    }

    /// The store waiting from `uptime` for the final captures of its streams.
    pub(super) fn finish(self, uptime: Duration) -> Finishing {
        Finishing {
            actor: self.actor,
            links: self.links,
            since: uptime,
        }
    }

    /// Begins the session of the stream's dimension, ends sessions left behind once
    /// their writes are queued, attaches the stream and paces the store's upkeep. A failed
    /// request makes the stream forget its cached sections and hashes, so it never skips
    /// a column whose write was lost; session changes need nothing, as the store's session
    /// epoch moves on the moment their requests are queued.
    pub(super) fn drive(
        &mut self,
        frame: &mut DistantTerrainFrame<'_>,
        report: &mut DistantTerrainReport,
        drawing: bool,
        quotas: Quotas,
    ) {
        let mut events = Vec::new();
        self.actor.poll_events(&mut events);
        for event in events {
            if matches!(event, ActorEvent::RequestFailed { .. }) {
                self.invalidate_due = true;
            }
            count_event(event, report);
        }
        self.links
            .retain(|link| !(link.is_released() && link.poll_written()));
        let Some(stream) = frame.stream.as_deref_mut() else {
            return;
        };
        let now = frame.now;
        if std::mem::take(&mut self.invalidate_due) {
            stream.invalidate_distant_terrain_sections();
        }
        let stats = stream.distant_terrain_stats();
        // Evictions at a dimension change still write into the session left behind.
        let settled = stats.is_none_or(|stats| stats.is_settled());
        self.pending.extend(self.sessions.follow(
            Dimension(stream.current_dimension()),
            stream.form_dimension_epoch(),
            settled,
            now,
        ));
        if !self.send_pending(report) {
            return;
        }
        if self.ingest == Ingest::Writable && !stream.distant_terrain_enabled() {
            match stream.enable_distant_terrain(self.actor.client(), now) {
                Ok(link) => self.links.push(link),
                Err(error) => self.ingest = unavailable(&error, report),
            }
        }
        stream.set_distant_terrain_minute(now);
        let meshing = stream.distant_terrain_mesh_stats().is_some();
        if drawing && !meshing {
            if let Err(error) = stream.enable_distant_terrain_meshes(self.actor.client(), now) {
                warn!(%error, "distant terrain meshing unavailable");
                report.store_errors += 1;
            }
        } else if !drawing && meshing {
            stream.disable_distant_terrain_meshes();
        }
        self.follow_quotas(quotas, now, report);
        if self.ingest == Ingest::Writable {
            self.pace(now, stats, frame.player_block, report);
        }
    }

    /// Applies `quotas` when they differ from the store's: at once when no cap shrinks,
    /// otherwise once they stayed the same for [`QUOTA_SETTLE_MINUTES`], with a quota pass
    /// right away rather than at the next interval.
    fn follow_quotas(&mut self, quotas: Quotas, now: Minute, report: &mut DistantTerrainReport) {
        if self.quotas == quotas {
            self.settling = None;
            return;
        }
        let shrinks = quotas.store_bytes < self.quotas.store_bytes
            || quotas.global_bytes < self.quotas.global_bytes
            || quotas.expiry_minutes < self.quotas.expiry_minutes;
        if shrinks {
            let since = match self.settling {
                Some((settling, since)) if settling == quotas => since,
                _ => now,
            };
            self.settling = Some((quotas, since));
            if now.minutes_since(since) < QUOTA_SETTLE_MINUTES {
                return;
            }
        }
        if self.submit(Request::SetQuotas(quotas), report) {
            self.quotas = quotas;
            self.settling = None;
            self.quota_pass_due |= shrinks;
        }
    }

    /// Sends held-back session requests in order; false while the queue is still full.
    fn send_pending(&mut self, report: &mut DistantTerrainReport) -> bool {
        while let Some(request) = self.pending.pop_front() {
            match self.actor.try_submit(request) {
                Ok(()) => {}
                Err(SubmitError::Full(request)) => {
                    self.pending.push_front(request);
                    return false;
                }
                Err(SubmitError::Stopped(_)) => report.store_errors += 1,
            }
        }
        true
    }

    /// Queues a flush every [`FLUSH_INTERVAL_MINUTES`], quotas every
    /// [`QUOTA_INTERVAL_MINUTES`] around the player, and a sync once ingestion is idle
    /// after writing; a full queue retries them on a later frame.
    fn pace(
        &mut self,
        now: Minute,
        stats: Option<DistantTerrainStats>,
        player_block: Option<[i64; 2]>,
        report: &mut DistantTerrainReport,
    ) {
        if now.minutes_since(self.last_flush) >= FLUSH_INTERVAL_MINUTES
            && self.submit(Request::Flush { now }, report)
        {
            self.last_flush = now;
        }
        if self.quota_pass_due || now.minutes_since(self.last_quotas) >= QUOTA_INTERVAL_MINUTES {
            let focus = self
                .sessions
                .current()
                .zip(player_block)
                .map(|(dimension, block)| Focus {
                    dimension,
                    block_x: block[0],
                    block_z: block[1],
                });
            if self.submit(Request::EnforceQuotas { now, focus }, report) {
                self.last_quotas = now;
                self.quota_pass_due = false;
            }
        }
        if let Some(stats) = stats
            && stats.is_settled()
            && stats.dirty_columns == 0
            && stats.sections_written != self.synced_sections
            && self.submit(Request::Sync { now }, report)
        {
            self.synced_sections = stats.sections_written;
        }
    }

    /// Queues `request`; false when the queue is full or the thread stopped.
    fn submit(&mut self, request: Request, report: &mut DistantTerrainReport) -> bool {
        match self.actor.try_submit(request) {
            Ok(()) => true,
            Err(SubmitError::Full(_)) => false,
            Err(SubmitError::Stopped(_)) => {
                report.store_errors += 1;
                false
            }
        }
    }
}

/// What an open store's ingestion becomes when the stream refuses it for `error`; a
/// writer thread that cannot start counts as a store error.
pub(super) fn unavailable(
    error: &DistantTerrainUnavailable,
    report: &mut DistantTerrainReport,
) -> Ingest {
    match error {
        DistantTerrainUnavailable::ReadOnly => Ingest::ReadOnly,
        DistantTerrainUnavailable::WriterThread(_) => {
            warn!(%error, "distant terrain ingestion unavailable");
            report.store_errors += 1;
            Ingest::WriterUnavailable
        }
    }
}

/// Counts and logs one store event.
fn count_event(event: ActorEvent, report: &mut DistantTerrainReport) {
    match event {
        ActorEvent::Store(StoreEvent::Epoch { dimension, status }) => {
            report.epoch_changes += 1;
            debug!(dimension = dimension.0, ?status, "distant terrain epoch");
        }
        ActorEvent::Store(StoreEvent::Io { path, error }) => {
            report.store_errors += 1;
            warn!(path = %path.display(), %error, "distant terrain store IO failed");
        }
        ActorEvent::RequestFailed { request, error } => {
            report.store_errors += 1;
            warn!(request, %error, "distant terrain store request failed");
        }
        other => debug!(event = ?other, "distant terrain store event"),
    }
}
