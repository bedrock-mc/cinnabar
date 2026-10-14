//! State shared by the frame, the sample jobs and the writer threads.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

use lod::{
    epoch::ChunkSurface,
    geometry::{SectionKey, SectionPos},
    ingest::{SlabWork, WriterRegion},
    sampler::{BiomeSource, ColumnSample, ColumnSnapshot},
};
use lod_store::{
    actor::{SessionEpoch, StoreClient},
    identity::Dimension,
    minute::Minute,
};
use world::{BiomeStorage, Chunk, ChunkKey, LightStoreSnapshot, SubChunk};

use super::{
    DistantTerrainStats, MAX_LOD_BACKLOG, MAX_LOD_INBOX_COLUMNS,
    blocks::{ClassifierCounts, SessionBlocks},
    book::ColumnBook,
    fingerprint::Fingerprint,
    meshes::MeshShared,
};

/// Lifetime counters, written from any thread.
#[derive(Debug, Default)]
pub(in crate::stream) struct Counters {
    pub(in crate::stream) sampled: AtomicU64,
    pub(in crate::stream) skipped_unchanged: AtomicU64,
    pub(in crate::stream) superseded: AtomicU64,
    pub(in crate::stream) deferred: AtomicU64,
    pub(in crate::stream) dropped_final_captures: AtomicU64,
    pub(in crate::stream) merged_columns: AtomicU64,
    pub(in crate::stream) sections_written: AtomicU64,
    pub(in crate::stream) store_errors: AtomicU64,
    pub(in crate::stream) store_queue_full: AtomicU64,
    pub(in crate::stream) stale_writes: AtomicU64,
    pub(in crate::stream) handed_slabs: AtomicU64,
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

/// Why a column was captured. Changed columns wait dirty when ingestion has no room for
/// them; final captures never wait and are dropped only beyond
/// [`super::MAX_LOD_PENDING_FINAL_CAPTURES`].
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

/// What a writer thread receives.
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

    /// The captured column.
    pub(in crate::stream) fn chunk(&self) -> ChunkKey {
        match self {
            Self::Check(capture) => capture.input.chunk,
            Self::Sampled(column) => column.chunk,
        }
    }
}

/// Which writer thread owns which sections: every [`WriterRegion`] belongs to one of
/// `writers` threads, so each section key has one writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::stream) struct Writers {
    count: usize,
}

impl Writers {
    /// `count` writers, at least one.
    pub(in crate::stream) fn new(count: usize) -> Self {
        Self {
            count: count.max(1),
        }
    }

    /// The number of writer threads.
    pub(in crate::stream) fn count(self) -> usize {
        self.count
    }

    /// The writer owning `region`: `(x + 2z) mod count`, so the four regions of any 2×2
    /// block, where a view usually crosses region borders, go to four writers when there
    /// are four.
    pub(in crate::stream) fn of_region(self, region: WriterRegion) -> usize {
        let index = (i64::from(region.x) + 2 * i64::from(region.z)).rem_euclid(self.count as i64);
        usize::try_from(index).expect("a remainder of a positive count")
    }

    /// The writer owning chunk column `chunk`'s sections.
    pub(in crate::stream) fn of_chunk(self, chunk: ChunkKey) -> usize {
        self.of_region(WriterRegion::of_chunk(chunk.x, chunk.z))
    }

    /// True when writer `index` owns the section at `pos`.
    pub(in crate::stream) fn owns(self, index: usize, pos: SectionPos) -> bool {
        WriterRegion::of(pos).is_some_and(|region| self.of_region(region) == index)
    }
}

/// What one writer thread takes from the inbox at once.
#[derive(Default)]
pub(in crate::stream) struct WriterBatch {
    pub(in crate::stream) items: Vec<WriterInput>,
    /// Slab re-mips other writers handed over, with their dimension.
    pub(in crate::stream) slabs: Vec<(Dimension, SlabWork)>,
}

/// One writer thread's share of the inbox.
#[derive(Default)]
struct WriterQueue {
    items: VecDeque<WriterInput>,
    /// Slab re-mips handed over by other writers, without repeats.
    slabs: BTreeSet<(Dimension, SlabWork)>,
    /// The frame asked the writer to retry requests a full store queue handed back.
    nudged: bool,
}

/// The writer threads' inbox, one queue each.
#[derive(Default)]
pub(in crate::stream) struct Inbox {
    queues: Vec<WriterQueue>,
    /// Items across every queue; changed columns are refused at
    /// [`MAX_LOD_INBOX_COLUMNS`].
    items: usize,
    /// Sampled columns among the items.
    pub(in crate::stream) sampled: usize,
    /// Handed-over slabs across every queue.
    slabs: usize,
}

impl Inbox {
    /// An empty inbox for `writers`.
    fn new(writers: Writers) -> Self {
        Self {
            queues: (0..writers.count())
                .map(|_| WriterQueue::default())
                .collect(),
            ..Self::default()
        }
    }

    /// True when no item or slab waits.
    fn is_empty(&self) -> bool {
        self.items == 0 && self.slabs == 0
    }
}

/// One writer thread's state the frame reads.
#[derive(Debug, Default)]
pub(in crate::stream) struct WriterFlags {
    /// The writer holds writes or columns back for the store's full queue and waits for a
    /// nudge; set before the writer stops being busy, so ingestion is not settled meanwhile.
    pub(in crate::stream) retry_due: AtomicBool,
    /// The writer is working on items it took from the inbox.
    pub(in crate::stream) busy: AtomicBool,
}

/// Where ingestion runs: the store client, the minute stamped on its requests, the writer
/// threads' inbox, the column book and the counters.
pub(in crate::stream) struct LodShared {
    pub(in crate::stream) client: StoreClient,
    pub(in crate::stream) writers: Writers,
    now: AtomicU32,
    /// Bumped when a store request failed, so cached sections and hashes may no longer
    /// match the store; see [`Self::generation`].
    invalidations: AtomicU64,
    enabled: AtomicBool,
    /// The stream stopped capturing; what it captured is still written.
    released: AtomicBool,
    /// Fingerprints sample jobs computed, for the frame to keep for resident columns.
    hashed: Mutex<Vec<(ChunkKey, Fingerprint)>>,
    /// Changed columns the full inbox handed back, with whether they must be sampled
    /// whatever their hash, for the frame to mark dirty again.
    deferred: Mutex<Vec<(ChunkKey, bool)>>,
    inbox: Mutex<Inbox>,
    /// Signalled when the inbox gains work, the frame nudges a writer, or ingestion stops.
    ready: Condvar,
    pub(in crate::stream) book: Mutex<ColumnBook>,
    /// Per writer thread, by index.
    pub(in crate::stream) flags: Box<[WriterFlags]>,
    /// The session identities of the latest merged columns, which classify materials for
    /// merges of handed-over slabs alone.
    blocks: Mutex<Option<Arc<SessionBlocks>>>,
    /// The mesher told about every stored section, while meshing is on.
    meshes: Mutex<Option<Arc<MeshShared>>>,
    /// Captures sent to a job or the writer and not yet sampled.
    pub(in crate::stream) in_flight: AtomicUsize,
    /// Final captures not yet written or dropped.
    pub(in crate::stream) final_captures: AtomicUsize,
    pub(in crate::stream) counters: Counters,
}

impl LodShared {
    /// Shared state writing through `client` with `writers` writer threads, stamped with
    /// `now`.
    pub(in crate::stream) fn new(client: StoreClient, now: Minute, writers: Writers) -> Self {
        Self {
            client,
            writers,
            now: AtomicU32::new(now.get()),
            invalidations: AtomicU64::new(0),
            enabled: AtomicBool::new(true),
            released: AtomicBool::new(false),
            hashed: Mutex::new(Vec::new()),
            deferred: Mutex::new(Vec::new()),
            inbox: Mutex::new(Inbox::new(writers)),
            ready: Condvar::new(),
            book: Mutex::new(ColumnBook::default()),
            flags: (0..writers.count())
                .map(|_| WriterFlags::default())
                .collect(),
            blocks: Mutex::new(None),
            meshes: Mutex::new(None),
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

    /// Hands changed `column` back to the frame, which marks it dirty again so it is
    /// captured once there is room, instead of losing its change; a `resample` column is
    /// then sampled whatever its hash. Counted in [`Counters::deferred`].
    pub(in crate::stream) fn defer(&self, column: ChunkKey, resample: bool) {
        Counters::bump(&self.counters.deferred);
        self.deferred
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((column, resample));
    }

    /// Takes the columns [`Self::defer`] handed back since the last call.
    pub(in crate::stream) fn take_deferred(&self) -> Vec<(ChunkKey, bool)> {
        std::mem::take(&mut *self.deferred.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Turns ingestion off, wakes the writers so they stop, and releases queued work, which
    /// may hold retired chunks, on a background thread.
    pub(in crate::stream) fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
        let dropped = std::mem::replace(&mut *self.lock_inbox(), Inbox::new(self.writers));
        self.ready.notify_all();
        if dropped.items > 0 {
            rayon::spawn(move || drop(dropped));
        }
    }

    /// Sets the mesher told about stored sections; `None` stops telling.
    pub(in crate::stream) fn set_meshes(&self, meshes: Option<Arc<MeshShared>>) {
        *self.meshes.lock().unwrap_or_else(PoisonError::into_inner) = meshes;
    }

    /// Tells the mesher, if any, that `keys` of `dimension` were stored.
    pub(in crate::stream) fn notify_written(&self, dimension: Dimension, keys: &[SectionKey]) {
        let meshes = self
            .meshes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(meshes) = meshes {
            meshes.written(dimension, keys);
        }
    }

    /// Records the session identities a writer merges columns with.
    pub(in crate::stream) fn note_blocks(&self, blocks: &Arc<SessionBlocks>) {
        let mut latest = self.blocks.lock().unwrap_or_else(PoisonError::into_inner);
        if !latest
            .as_ref()
            .is_some_and(|latest| Arc::ptr_eq(latest, blocks))
        {
            *latest = Some(Arc::clone(blocks));
        }
    }

    /// The session identities of the latest merged columns; `None` before any.
    pub(in crate::stream) fn latest_blocks(&self) -> Option<Arc<SessionBlocks>> {
        self.blocks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
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

    /// Changed captures the frame may send on now without one being refused: the room left
    /// under [`MAX_LOD_BACKLOG`] outstanding samples and in the writer inbox.
    pub(in crate::stream) fn room(&self) -> usize {
        let inbox = self.lock_inbox();
        let outstanding = self.in_flight.load(Ordering::Acquire) + inbox.sampled;
        MAX_LOD_BACKLOG
            .saturating_sub(outstanding)
            .min(MAX_LOD_INBOX_COLUMNS.saturating_sub(inbox.items))
    }

    /// Queues `input` for the writer owning its column without ever waiting. A changed
    /// column finding the inbox full, or any input once ingestion is off, is handed back;
    /// final captures are bounded where they are taken instead.
    pub(in crate::stream) fn push(&self, input: WriterInput) -> Result<(), WriterInput> {
        let writer = self.writers.of_chunk(input.chunk());
        let mut inbox = self.lock_inbox();
        let full = inbox.items >= MAX_LOD_INBOX_COLUMNS;
        if !self.enabled() || (full && input.kind() == CaptureKind::Changed) {
            return Err(input);
        }
        inbox.sampled += usize::from(matches!(input, WriterInput::Sampled(_)));
        inbox.items += 1;
        inbox.queues[writer].items.push_back(input);
        drop(inbox);
        self.ready.notify_all();
        Ok(())
    }

    /// Hands `slabs` of `dimension` to the writers owning them, never refusing them while
    /// ingestion runs. The handing writer is still busy, so ingestion stays unsettled
    /// throughout.
    pub(in crate::stream) fn hand_over(&self, dimension: Dimension, slabs: Vec<SlabWork>) {
        if slabs.is_empty() {
            return;
        }
        let mut inbox = self.lock_inbox();
        if !self.enabled() {
            return;
        }
        for slab in slabs {
            let Some(region) = WriterRegion::of(slab.pos) else {
                continue;
            };
            let writer = self.writers.of_region(region);
            if inbox.queues[writer].slabs.insert((dimension, slab)) {
                inbox.slabs += 1;
            }
            Counters::bump(&self.counters.handed_slabs);
        }
        drop(inbox);
        self.ready.notify_all();
    }

    /// Wakes the writers waiting to retry a full store queue; the frame calls it each poll.
    pub(in crate::stream) fn nudge_writer(&self) {
        let due: Vec<usize> = (0..self.writers.count())
            .filter(|&writer| self.flags[writer].retry_due.load(Ordering::Acquire))
            .collect();
        if due.is_empty() {
            return;
        }
        let mut inbox = self.lock_inbox();
        for writer in due {
            inbox.queues[writer].nudged = true;
        }
        drop(inbox);
        self.ready.notify_all();
    }

    /// Blocks writer thread `writer` until it has work: up to `limit` items and every slab
    /// handed to it, or, while `waiting_for_store`, only a nudge, which returns nothing.
    /// Marks the writer busy before returning; `None` once ingestion is off.
    pub(in crate::stream) fn wait_for_work(
        &self,
        writer: usize,
        limit: usize,
        waiting_for_store: bool,
    ) -> Option<WriterBatch> {
        let mut inbox = self.lock_inbox();
        let flags = &self.flags[writer];
        flags.busy.store(false, Ordering::Release);
        loop {
            if !self.enabled() {
                return None;
            }
            let queue = &mut inbox.queues[writer];
            if std::mem::take(&mut queue.nudged) && waiting_for_store {
                flags.busy.store(true, Ordering::Release);
                return Some(WriterBatch::default());
            }
            if !waiting_for_store && (!queue.items.is_empty() || !queue.slabs.is_empty()) {
                let take = queue.items.len().min(limit);
                let items: Vec<WriterInput> = queue.items.drain(..take).collect();
                let slabs: Vec<(Dimension, SlabWork)> =
                    std::mem::take(&mut queue.slabs).into_iter().collect();
                inbox.items -= items.len();
                inbox.slabs -= slabs.len();
                inbox.sampled -= items
                    .iter()
                    .filter(|input| matches!(input, WriterInput::Sampled(_)))
                    .count();
                flags.busy.store(true, Ordering::Release);
                return Some(WriterBatch { items, slabs });
            }
            inbox = self
                .ready
                .wait(inbox)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// True when no capture is in flight, unwritten or queued and no slab waits, so every
    /// capture to come is newer than every one the writers have seen.
    pub(in crate::stream) fn nothing_outstanding(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst) == 0
            && self.final_captures.load(Ordering::SeqCst) == 0
            && self.lock_inbox().is_empty()
    }

    /// True while a writer works, holds work back for the store's full queue or has slabs
    /// handed to it. The inbox is read first: a writer marks itself busy before it lets go
    /// of the inbox lock with the work it took.
    fn writers_busy(&self) -> bool {
        self.lock_inbox().slabs > 0
            || self.flags.iter().any(|flags| {
                flags.busy.load(Ordering::Acquire) || flags.retry_due.load(Ordering::Acquire)
            })
    }

    /// The counters with the frame's own figures; `dirty_columns` counts with the deferred
    /// columns the frame has not taken back yet.
    pub(in crate::stream) fn stats(&self, dirty_columns: usize) -> DistantTerrainStats {
        let load = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        let counters = &self.counters;
        let deferred = self
            .deferred
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
        DistantTerrainStats {
            dirty_columns: dirty_columns + deferred,
            in_flight: self.in_flight.load(Ordering::Relaxed),
            final_captures: self.final_captures.load(Ordering::Relaxed),
            queued: self.queued(),
            writer_busy: self.writers_busy(),
            sampled: load(&counters.sampled),
            skipped_unchanged: load(&counters.skipped_unchanged),
            superseded: load(&counters.superseded),
            deferred: load(&counters.deferred),
            dropped_final_captures: load(&counters.dropped_final_captures),
            merged_columns: load(&counters.merged_columns),
            sections_written: load(&counters.sections_written),
            store_errors: load(&counters.store_errors),
            store_queue_full: load(&counters.store_queue_full),
            stale_writes: load(&counters.stale_writes),
            handed_slabs: load(&counters.handed_slabs),
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
