//! State shared by the frame, the sample jobs and the writer thread.

use std::{
    collections::{BTreeMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use lod::{
    epoch::ChunkSurface,
    sampler::{BiomeSource, ColumnSample, ColumnSnapshot},
};
use lod_store::{
    actor::{SessionEpoch, StoreClient},
    identity::Dimension,
    minute::Minute,
};
use world::{BiomeStorage, Chunk, ChunkKey, LightStoreSnapshot, SubChunk};

use super::{
    DistantTerrainStats, MAX_LOD_INBOX_COLUMNS,
    blocks::{ClassifierCounts, SessionBlocks},
    book::ColumnBook,
    fingerprint::Fingerprint,
};

/// Lifetime counters, written from any thread.
#[derive(Debug, Default)]
pub(in crate::stream) struct Counters {
    pub(in crate::stream) sampled: AtomicU64,
    pub(in crate::stream) skipped_unchanged: AtomicU64,
    pub(in crate::stream) superseded: AtomicU64,
    pub(in crate::stream) dropped: AtomicU64,
    pub(in crate::stream) dropped_final_captures: AtomicU64,
    pub(in crate::stream) merged_columns: AtomicU64,
    pub(in crate::stream) sections_written: AtomicU64,
    pub(in crate::stream) store_errors: AtomicU64,
    pub(in crate::stream) store_queue_full: AtomicU64,
    pub(in crate::stream) stale_writes: AtomicU64,
    pub(in crate::stream) unkeyed_columns: AtomicU64,
    pub(in crate::stream) unknown_ids: AtomicU64,
    pub(in crate::stream) unkeyed_ids: AtomicU64,
    pub(in crate::stream) registry_full: AtomicU64,
    pub(in crate::stream) jobs_queued: AtomicU64,
    pub(in crate::stream) worker_nanos: AtomicU64,
}

impl Counters {
    /// Adds one to `counter`.
    pub(in crate::stream) fn bump(counter: &AtomicU64) {
        Self::add(counter, 1);
    }

    /// Adds `amount` to `counter`.
    pub(in crate::stream) fn add(counter: &AtomicU64, amount: u64) {
        counter.fetch_add(amount, Ordering::Relaxed);
    }

    /// Adds one sampled column's classifier counts.
    pub(in crate::stream) fn add_classified(&self, counts: ClassifierCounts) {
        Self::add(&self.unknown_ids, counts.unknown_ids);
        Self::add(&self.unkeyed_ids, counts.unkeyed_ids);
        Self::add(&self.registry_full, counts.registry_full);
    }

    /// Adds the time a Lod-lane job held its worker.
    pub(in crate::stream) fn add_worker_time(&self, held: Duration) {
        Self::add(
            &self.worker_nanos,
            u64::try_from(held.as_nanos()).unwrap_or(u64::MAX),
        );
    }
}

/// Why a column was captured. Changed columns are dropped and counted when ingestion has
/// no room for them; final captures never are, short of [`super::MAX_LOD_PENDING_FINAL_CAPTURES`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) enum CaptureKind {
    /// A resident column that settled after a change.
    Changed,
    /// A column's final state: retired by an eviction, or resident when the stream
    /// stopped ingesting.
    Final,
}

/// Counts one piece of ingestion work in one of [`LodShared`]'s gauges until dropped.
pub(in crate::stream) struct InFlight {
    shared: Arc<LodShared>,
    gauge: fn(&LodShared) -> &AtomicUsize,
}

impl InFlight {
    /// A capture sent on and not yet sampled, counted in [`LodShared::in_flight`].
    pub(in crate::stream) const SAMPLE: fn(&LodShared) -> &AtomicUsize = |shared| &shared.in_flight;
    /// A final capture not yet written, counted in [`LodShared::final_captures`].
    pub(in crate::stream) const FINAL: fn(&LodShared) -> &AtomicUsize =
        |shared| &shared.final_captures;

    /// Counts work about to start in `gauge`.
    pub(in crate::stream) fn start(
        shared: &Arc<LodShared>,
        gauge: fn(&LodShared) -> &AtomicUsize,
    ) -> Self {
        gauge(shared).fetch_add(1, Ordering::SeqCst);
        Self {
            shared: Arc::clone(shared),
            gauge,
        }
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        (self.gauge)(&self.shared).fetch_sub(1, Ordering::SeqCst);
    }
}

/// One captured column on its way to the writer, stamped with the sequence of its
/// capture so the writer never lets an older capture overwrite a newer one.
pub(in crate::stream) struct Capture {
    pub(in crate::stream) seq: u64,
    pub(in crate::stream) kind: CaptureKind,
    pub(in crate::stream) input: ColumnInput,
    /// The content hash, once the frame or the writer computed it.
    pub(in crate::stream) checked_hash: Option<u64>,
    /// The column's light changed since it was last captured, so it is sampled even when
    /// its content hash, which leaves light values out, matches the stored one.
    pub(in crate::stream) relit: bool,
    /// Counts the capture in [`LodShared::in_flight`] once sent to a job or the writer.
    pub(in crate::stream) job: Option<InFlight>,
    /// Counts a final capture in [`LodShared::final_captures`] until it is written.
    pub(in crate::stream) final_capture: Option<InFlight>,
}

/// One sampled column waiting for the writer.
pub(in crate::stream) struct SampledColumn {
    pub(in crate::stream) seq: u64,
    pub(in crate::stream) kind: CaptureKind,
    pub(in crate::stream) dimension: Dimension,
    pub(in crate::stream) chunk: ChunkKey,
    pub(in crate::stream) sample: ColumnSample,
    pub(in crate::stream) surface: Box<ChunkSurface>,
    pub(in crate::stream) hash: u64,
    /// The identities the column was classified with.
    pub(in crate::stream) blocks: Arc<SessionBlocks>,
    /// Keeps a final capture counted until it is written.
    pub(in crate::stream) _final: Option<InFlight>,
}

/// What the writer thread receives.
pub(in crate::stream) enum WriterInput {
    /// A capture whose hash the store must be asked for before it is sampled.
    Check(Box<Capture>),
    /// A sampled column to merge.
    Sampled(Box<SampledColumn>),
}

impl WriterInput {
    /// Why the column was captured.
    pub(in crate::stream) fn kind(&self) -> CaptureKind {
        match self {
            Self::Check(capture) => capture.kind,
            Self::Sampled(column) => column.kind,
        }
    }
}

/// The writer thread's inbox.
#[derive(Default)]
pub(in crate::stream) struct Inbox {
    pub(in crate::stream) items: VecDeque<WriterInput>,
    /// Sampled columns among `items`.
    pub(in crate::stream) sampled: usize,
    /// The frame asked the writer to retry requests a full store queue handed back.
    pub(in crate::stream) nudged: bool,
}

/// Where ingestion runs: the store client, the minute stamped on its requests, the writer
/// thread's inbox, the column book and the counters.
pub(in crate::stream) struct LodShared {
    pub(in crate::stream) client: StoreClient,
    now: AtomicU32,
    /// Bumped when a store request failed, so cached sections and hashes may no longer
    /// match the store; see [`Self::generation`].
    invalidations: AtomicU64,
    enabled: AtomicBool,
    /// The stream stopped capturing; what it captured is still written.
    released: AtomicBool,
    /// Fingerprints sample jobs computed, for the frame to keep for resident columns.
    hashed: Mutex<Vec<(ChunkKey, Fingerprint)>>,
    inbox: Mutex<Inbox>,
    /// Signalled when the inbox gains work, the frame nudges the writer, or ingestion stops.
    ready: Condvar,
    pub(in crate::stream) book: Mutex<ColumnBook>,
    /// The writer holds writes or columns back for the store's full queue and waits for a
    /// nudge; set before the writer stops being busy, so ingestion is not settled meanwhile.
    pub(in crate::stream) retry_due: AtomicBool,
    /// The writer is working on items it took from the inbox.
    pub(in crate::stream) writer_busy: AtomicBool,
    /// Captures sent to a job or the writer and not yet sampled.
    pub(in crate::stream) in_flight: AtomicUsize,
    /// Final captures not yet written or dropped.
    pub(in crate::stream) final_captures: AtomicUsize,
    pub(in crate::stream) counters: Counters,
}

impl LodShared {
    /// Shared state writing through `client`, stamped with `now`.
    pub(in crate::stream) fn new(client: StoreClient, now: Minute) -> Self {
        Self {
            client,
            now: AtomicU32::new(now.get()),
            invalidations: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
            released: AtomicBool::new(false),
            hashed: Mutex::new(Vec::new()),
            inbox: Mutex::new(Inbox::default()),
            ready: Condvar::new(),
            book: Mutex::new(ColumnBook::default()),
            retry_due: AtomicBool::new(false),
            writer_busy: AtomicBool::new(false),
            in_flight: AtomicUsize::new(0),
            final_captures: AtomicUsize::new(0),
            counters: Counters::default(),
        }
    }

    /// The minute stamped on store requests.
    pub(in crate::stream) fn now(&self) -> Minute {
        Minute::new(self.now.load(Ordering::Relaxed))
    }

    /// Sets the minute stamped on later store requests.
    pub(in crate::stream) fn set_now(&self, now: Minute) {
        self.now.store(now.get(), Ordering::Relaxed);
    }

    /// What cached sections and content hashes are valid for: it moves on with every store
    /// session change ([`SessionEpoch`]) and every failed request.
    pub(in crate::stream) fn generation(&self) -> u64 {
        self.generation_at(self.client.session_epoch())
    }

    /// [`Self::generation`] with the store's session epoch at `epoch`.
    pub(in crate::stream) fn generation_at(&self, epoch: SessionEpoch) -> u64 {
        self.invalidations
            .load(Ordering::Acquire)
            .wrapping_add(epoch.0)
    }

    /// Moves the generation on after a store request failed.
    pub(in crate::stream) fn invalidate(&self) {
        self.invalidations.fetch_add(1, Ordering::AcqRel);
    }

    /// False once ingestion was turned off; jobs and the writer then drop their work.
    pub(in crate::stream) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// True once the stream stopped capturing.
    pub(in crate::stream) fn released(&self) -> bool {
        self.released.load(Ordering::Acquire)
    }

    /// Records that the stream stopped capturing; queued work is still written.
    pub(in crate::stream) fn release(&self) {
        self.released.store(true, Ordering::Release);
        self.ready.notify_all();
    }

    /// Hands the frame the fingerprint of `column`'s hash.
    pub(in crate::stream) fn report_hashed(&self, column: ChunkKey, fingerprint: Fingerprint) {
        self.hashed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((column, fingerprint));
    }

    /// Takes the fingerprints sample jobs computed since the last call.
    pub(in crate::stream) fn take_hashed(&self) -> Vec<(ChunkKey, Fingerprint)> {
        std::mem::take(&mut *self.hashed.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Turns ingestion off, wakes the writer so it stops, and releases queued work, which
    /// may hold retired chunks, on a background thread.
    pub(in crate::stream) fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
        let dropped = std::mem::take(&mut *self.lock_inbox());
        self.ready.notify_all();
        if !dropped.items.is_empty() {
            rayon::spawn(move || drop(dropped));
        }
    }

    /// The inbox, even after a panicking holder.
    fn lock_inbox(&self) -> MutexGuard<'_, Inbox> {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The column book, even after a panicking holder.
    pub(in crate::stream) fn lock_book(&self) -> MutexGuard<'_, ColumnBook> {
        self.book.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sampled columns waiting for the writer.
    pub(in crate::stream) fn queued(&self) -> usize {
        self.lock_inbox().sampled
    }

    /// Queues `input` for the writer without ever waiting. A changed column finding the
    /// inbox full, or any input once ingestion is off, is handed back; final captures are
    /// bounded where they are taken instead.
    pub(in crate::stream) fn push(&self, input: WriterInput) -> Result<(), WriterInput> {
        let mut inbox = self.lock_inbox();
        let full = inbox.items.len() >= MAX_LOD_INBOX_COLUMNS;
        if !self.enabled() || (full && input.kind() == CaptureKind::Changed) {
            return Err(input);
        }
        inbox.sampled += usize::from(matches!(input, WriterInput::Sampled(_)));
        inbox.items.push_back(input);
        drop(inbox);
        self.ready.notify_all();
        Ok(())
    }

    /// Wakes a writer waiting to retry a full store queue; the frame calls it each poll.
    pub(in crate::stream) fn nudge_writer(&self) {
        if self.retry_due.load(Ordering::Acquire) {
            self.lock_inbox().nudged = true;
            self.ready.notify_all();
        }
    }

    /// Blocks the writer thread until it has work: up to `limit` items, or, while
    /// `waiting_for_store`, only a nudge, which returns no items. Marks the writer busy
    /// before returning; `None` once ingestion is off.
    pub(in crate::stream) fn wait_for_work(
        &self,
        limit: usize,
        waiting_for_store: bool,
    ) -> Option<Vec<WriterInput>> {
        let mut inbox = self.lock_inbox();
        self.writer_busy.store(false, Ordering::Release);
        loop {
            if !self.enabled() {
                return None;
            }
            if std::mem::take(&mut inbox.nudged) && waiting_for_store {
                self.writer_busy.store(true, Ordering::Release);
                return Some(Vec::new());
            }
            if !waiting_for_store && !inbox.items.is_empty() {
                let take = inbox.items.len().min(limit);
                let batch: Vec<WriterInput> = inbox.items.drain(..take).collect();
                inbox.sampled -= batch
                    .iter()
                    .filter(|input| matches!(input, WriterInput::Sampled(_)))
                    .count();
                self.writer_busy.store(true, Ordering::Release);
                return Some(batch);
            }
            inbox = self
                .ready
                .wait(inbox)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// True when no capture is in flight, unwritten or queued, so every capture to come is
    /// newer than every one the writer has seen.
    pub(in crate::stream) fn nothing_outstanding(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst) == 0
            && self.final_captures.load(Ordering::SeqCst) == 0
            && self.lock_inbox().items.is_empty()
    }

    /// The counters with the frame's own figures.
    pub(in crate::stream) fn stats(&self, dirty_columns: usize) -> DistantTerrainStats {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        let counters = &self.counters;
        DistantTerrainStats {
            dirty_columns,
            in_flight: self.in_flight.load(Ordering::Relaxed),
            final_captures: self.final_captures.load(Ordering::Relaxed),
            queued: self.queued(),
            writer_busy: self.writer_busy.load(Ordering::Acquire)
                || self.retry_due.load(Ordering::Acquire),
            sampled: load(&counters.sampled),
            skipped_unchanged: load(&counters.skipped_unchanged),
            superseded: load(&counters.superseded),
            dropped: load(&counters.dropped),
            dropped_final_captures: load(&counters.dropped_final_captures),
            merged_columns: load(&counters.merged_columns),
            sections_written: load(&counters.sections_written),
            store_errors: load(&counters.store_errors),
            store_queue_full: load(&counters.store_queue_full),
            stale_writes: load(&counters.stale_writes),
            unkeyed_columns: load(&counters.unkeyed_columns),
            unknown_ids: load(&counters.unknown_ids),
            unkeyed_ids: load(&counters.unkeyed_ids),
            registry_full: load(&counters.registry_full),
            jobs_queued: load(&counters.jobs_queued),
            worker_time: Duration::from_nanos(load(&counters.worker_nanos)),
        }
    }
}

/// A column's biome storages, captured from a resident column or owned by a retired one,
/// whose memory is released when its sample job ends.
pub(in crate::stream) enum ColumnBiomes {
    Captured(BTreeMap<i32, Arc<BiomeStorage>>),
    Retired(Chunk),
}

impl BiomeSource for ColumnBiomes {
    fn biome_storage(&self, sub_chunk_y: i32) -> Option<Arc<BiomeStorage>> {
        match self {
            Self::Captured(storages) => storages.get(&sub_chunk_y).cloned(),
            Self::Retired(chunk) => chunk.biome_storage(sub_chunk_y),
        }
    }
}

/// Everything one sample job reads, owned so it outlives the column's residency.
pub(in crate::stream) struct ColumnInput {
    pub(in crate::stream) chunk: ChunkKey,
    pub(in crate::stream) sub_chunks: Arc<BTreeMap<i32, Arc<SubChunk>>>,
    pub(in crate::stream) biomes: ColumnBiomes,
    pub(in crate::stream) default_biome: u32,
    /// Light captured with the column, possibly shared with other columns captured at once.
    pub(in crate::stream) light: Arc<LightStoreSnapshot>,
    pub(in crate::stream) blocks: Arc<SessionBlocks>,
}

impl ColumnInput {
    /// The sampler's view of this column, with its captured light.
    pub(in crate::stream) fn snapshot(&self) -> ColumnSnapshot<'_> {
        ColumnSnapshot {
            chunk: self.chunk,
            sub_chunks: &self.sub_chunks,
            biomes: &self.biomes,
            default_biome: self.default_biome,
            light: Some(&*self.light),
        }
    }
}
