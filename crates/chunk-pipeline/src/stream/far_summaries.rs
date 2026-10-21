//! Far-terrain summaries of complete and retiring columns, extracted on world workers.

#[cfg(not(target_arch = "wasm32"))]
use crossbeam_channel::Select;
use crossbeam_channel::unbounded;
use far_terrain::{ColumnSnapshot, EncodedSummary, SummaryBudget, SurfaceColours, SurfaceContext};

use super::*;

mod admission;
mod preparation;
mod producer;
#[cfg(feature = "publication-test-support")]
mod test_support;
#[cfg(feature = "publication-test-support")]
pub use test_support::FarSummaryFixture;

use preparation::{PendingContext, PreparedAssets};
use producer::{Event, Producer, Progress};

/// Sources captured and copied jobs admitted per owner poll or inbox drain.
const JOBS_PER_POLL: usize = 2;

/// The encoded surface summary of one column, keyed with its dimension; workers encode it
/// so the owner thread only compares hashes.
#[derive(Debug)]
pub struct FarColumnSummary {
    pub column: ChunkKey,
    pub summary: EncodedSummary,
    /// Issue order of the job that took it; later issues saw later blocks.
    pub generation: u64,
    /// The column held blocks when its summary was delivered; far terrain keeps such
    /// summaries whatever its memory budget, as nothing else may draw the column.
    pub resident: bool,
}

/// One summary job's result, tagged with the generation it was issued under.
#[cfg(test)]
struct Finished {
    column: ChunkKey,
    generation: u64,
    summary: EncodedSummary,
}

#[derive(Clone, Copy)]
struct Issued {
    latest: u64,
    in_flight: u32,
}

/// Receives finished summaries and drops any older than the newest issued for its column,
/// since workers may finish a column's jobs out of order.
pub struct FarSummaryInbox {
    tx: Sender<Event>,
    rx: Receiver<Event>,
    issued: HashMap<ChunkKey, Issued>,
    next_generation: u64,
    deferred: Option<Event>,
    producer: Producer,
    wake: Arc<admission::Wake>,
}

impl Default for FarSummaryInbox {
    fn default() -> Self {
        Self::new(Producer::default())
    }
}

impl FarSummaryInbox {
    /// Owns source progress and delivery independently of the originating WorldStream.
    fn new(producer: Producer) -> Self {
        let (tx, rx) = unbounded();
        let wake = producer.admission.subscribe(tx.clone());
        Self {
            tx,
            rx,
            issued: HashMap::new(),
            next_generation: 0,
            deferred: None,
            producer,
            wake,
        }
    }

    /// Captures issue order before admission, so a deferred newer source invalidates old output.
    fn enqueue(&mut self, column: ChunkKey, snapshot: ColumnSnapshot, context: PendingContext) {
        let generation = self.issue(column);
        if self
            .producer
            .push(column, generation, snapshot, context, &self.tx)
            .is_some()
        {
            self.acknowledge(column);
        }
    }

    /// Makes a bounded admission attempt without waiting for workers or capacity.
    fn pump(&mut self) {
        self.producer.pump(&self.tx, JOBS_PER_POLL);
    }

    /// Disconnects cancelled delivery without releasing work still owned by a worker.
    /// Dropping the old receiver discards queued/deferred results; later sends discard their
    /// results at the worker. Neither path depends on the disabled stream being polled.
    fn cancel_pending(&mut self) {
        let mut replacement = Self::new(self.producer.empty());
        replacement.next_generation = self.next_generation;
        *self = replacement;
    }

    /// Issues one generation even if its source cannot yet enter the worker queue.
    fn issue(&mut self, column: ChunkKey) -> u64 {
        self.next_generation += 1;
        let issued = self.issued.entry(column).or_insert(Issued {
            latest: 0,
            in_flight: 0,
        });
        issued.latest = self.next_generation;
        issued.in_flight += 1;
        self.next_generation
    }

    /// The generation of the latest job issued; any issued later saw later blocks.
    pub(super) const fn issued_generation(&self) -> u64 {
        self.next_generation
    }

    /// Appends completed summaries and advances any deferred source work without waiting.
    pub fn drain(&mut self, out: &mut Vec<FarColumnSummary>) {
        self.drain_budgeted(out, &mut SummaryBudget::new(usize::MAX, usize::MAX));
    }

    /// Appends finished summaries within both limits, counting stale and continuation work too.
    pub fn drain_budgeted(&mut self, out: &mut Vec<FarColumnSummary>, budget: &mut SummaryBudget) {
        self.pump();
        while let Some(event) = self.deferred.take().or_else(|| self.rx.try_recv().ok()) {
            let bytes = match &event {
                Event::Prepared => {
                    self.wake.acknowledge();
                    continue;
                }
                Event::Retry { .. } => 0,
                Event::Step(step) => match &step.progress {
                    Progress::Complete(summary) => summary.bytes.len(),
                    Progress::Continue { .. } => 0,
                },
                #[cfg(test)]
                Event::Finished(finished) => finished.summary.bytes.len(),
            };
            if !budget.take(bytes) {
                self.deferred = Some(event);
                break;
            }
            self.accept(event, out);
        }
        self.pump();
    }

    /// Advances the producer, waiting for dependency, capacity or result publication until
    /// `deadline`. The application retains its existing bounded shutdown wait.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn drain_waiting(&mut self, out: &mut Vec<FarColumnSummary>, deadline: Instant) {
        self.drain(out);
        while !self.is_idle() && Instant::now() < deadline {
            let Ok(event) = self.rx.recv_deadline(deadline) else {
                break;
            };
            self.accept(event, out);
            self.drain(out);
        }
    }

    /// Waits for any non-idle inbox after the caller has given every inbox a drain turn.
    /// Deferred results are already ready. Waiting does not consume output or transfer its
    /// session ownership, and the caller supplies one shared deadline for the whole group.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn wait_for_any<'a>(
        inboxes: impl IntoIterator<Item = &'a Self>,
        deadline: Instant,
    ) -> bool {
        let mut ready = Select::new();
        let mut waiting = false;
        for inbox in inboxes {
            if inbox.is_idle() {
                continue;
            }
            if inbox.deferred.is_some() {
                return true;
            }
            ready.recv(&inbox.rx);
            waiting = true;
        }
        waiting && ready.ready_deadline(deadline).is_ok()
    }

    /// Releases one issue only when it is delivered, discarded as stale, or cancelled.
    fn acknowledge(&mut self, column: ChunkKey) {
        if let Some(issued) = self.issued.get_mut(&column) {
            issued.in_flight -= 1;
            if issued.in_flight == 0 {
                self.issued.remove(&column);
            }
        }
    }

    /// Transfers output to the delivery owner before releasing its admission reservation.
    fn accept(&mut self, event: Event, out: &mut Vec<FarColumnSummary>) {
        let (column, generation, summary, _permit) = match event {
            Event::Prepared => {
                self.wake.acknowledge();
                return;
            }
            Event::Retry { column, generation } => {
                if self.producer.sources.contains_key(&generation)
                    && self
                        .issued
                        .get(&column)
                        .is_some_and(|issued| issued.latest == generation)
                {
                    self.producer.retry(generation);
                } else {
                    self.producer.complete(generation);
                    self.acknowledge(column);
                }
                return;
            }
            Event::Step(step) => {
                let producer = &mut self.producer;
                match step.progress {
                    Progress::Continue { before, scan } => {
                        let current = self
                            .issued
                            .get(&step.column)
                            .is_some_and(|issued| issued.latest == step.generation);
                        if current && producer.sources.contains_key(&step.generation) {
                            producer.resume(step.generation, before, scan, step._permit);
                        } else {
                            producer.complete(step.generation);
                            self.acknowledge(step.column);
                        }
                        return;
                    }
                    Progress::Complete(summary) => {
                        producer.complete(step.generation);
                        (step.column, step.generation, summary, Some(step._permit))
                    }
                }
            }
            #[cfg(test)]
            Event::Finished(finished) => {
                (finished.column, finished.generation, finished.summary, None)
            }
        };
        let current = self
            .issued
            .get(&column)
            .is_some_and(|issued| issued.latest == generation);
        self.acknowledge(column);
        if current {
            out.push(FarColumnSummary {
                column,
                summary,
                generation,
                resident: false,
            });
        }
    }

    /// True when finished summaries wait to be drained.
    #[must_use]
    pub fn has_finished(&self) -> bool {
        !self.issued.is_empty() && (self.deferred.is_some() || !self.rx.is_empty())
    }

    /// True once every issued summary has been drained.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.issued.is_empty()
    }
}

pub(super) struct FarTerrainFeed {
    enabled: bool,
    prepared: Option<Arc<PreparedAssets>>,
    pub(super) pending: VecDeque<ChunkKey>,
    queued: HashSet<ChunkKey>,
    /// Columns that left residency, with the blocks they held, awaiting a summary job.
    pub(super) retiring: VecDeque<(ChunkKey, ColumnSnapshot)>,
    /// Columns that left residency since the last drain, whatever became of their summaries.
    unloaded: Vec<ChunkKey>,
    inbox: FarSummaryInbox,
}

impl Default for FarTerrainFeed {
    fn default() -> Self {
        Self {
            enabled: false,
            prepared: None,
            pending: VecDeque::new(),
            queued: HashSet::new(),
            retiring: VecDeque::new(),
            unloaded: Vec::new(),
            inbox: FarSummaryInbox::default(),
        }
    }
}

impl WorldStream {
    /// Ends delivery from the previous dimension, including eviction snapshots and late workers.
    pub(super) fn reset_far_terrain_dimension(&mut self) {
        let feed = &mut self.far_terrain;
        feed.pending.clear();
        feed.queued.clear();
        feed.retiring.clear();
        feed.unloaded.clear();
        feed.inbox.cancel_pending();
    }

    /// Discards old resource or namespace generations and resummarizes complete resident columns.
    pub fn reload_far_terrain_resources(&mut self) {
        let feed = &mut self.far_terrain;
        feed.prepared = None;
        feed.inbox.cancel_pending();
        feed.queued.clear();
        feed.pending.clear();
        if feed.enabled {
            feed.queued.extend(self.loaded_columns.iter().copied());
            feed.pending.extend(self.loaded_columns.iter().copied());
        }
    }

    /// Starts or stops summarising columns for far terrain; off drops queued work.
    pub fn set_far_terrain_enabled(&mut self, enabled: bool) {
        let was_enabled = self.far_terrain.enabled;
        let feed = &mut self.far_terrain;
        feed.enabled = enabled;
        if !enabled {
            feed.pending.clear();
            feed.queued.clear();
            feed.retiring.clear();
            feed.unloaded.clear();
            feed.inbox.cancel_pending();
            feed.prepared = None;
            self.clear_near_drawn();
        } else if !was_enabled {
            // Columns already resident were never summarised or tracked as drawn while off.
            let columns: Vec<ChunkKey> = self.loaded_columns.iter().copied().collect();
            for column in columns {
                self.note_far_column_complete(column);
            }
        }
    }

    #[must_use]
    pub const fn far_terrain_enabled(&self) -> bool {
        self.far_terrain.enabled
    }

    /// The radius the server last said it sends, in chunks.
    #[must_use]
    pub const fn server_view_radius(&self) -> Option<i32> {
        self.chunk_radius
    }

    /// The radius the client's chunk grid retains, once the server has confirmed one.
    #[must_use]
    pub fn retained_view_radius(&self) -> Option<i32> {
        self.retained_radius()
    }

    /// The local player's column, which centres retention and far terrain.
    #[must_use]
    pub fn far_terrain_centre(&self) -> ChunkKey {
        self.player_chunk()
    }

    /// True when finished summaries are waiting, so callers can borrow mutably only then.
    #[must_use]
    pub fn has_far_summaries(&self) -> bool {
        self.far_terrain.inbox.has_finished()
    }

    /// The generation of the latest summary job issued.
    pub(super) const fn far_terrain_issued_generation(&self) -> u64 {
        self.far_terrain.inbox.issued_generation()
    }

    /// Appends columns that left residency since the last call and are still gone, so far
    /// terrain stops keeping their summaries whatever their summaries' fate.
    pub fn drain_far_unloaded(&mut self, out: &mut Vec<ChunkKey>) {
        let unloaded = std::mem::take(&mut self.far_terrain.unloaded);
        out.extend(
            unloaded
                .into_iter()
                .filter(|column| !self.far_column_resident(*column)),
        );
    }

    /// True once no far summary waits, runs or awaits draining, so a settled view has paid
    /// for every summary it caused.
    #[must_use]
    pub fn far_summaries_idle(&self) -> bool {
        let feed = &self.far_terrain;
        feed.pending.is_empty() && feed.retiring.is_empty() && feed.inbox.is_idle()
    }

    /// True when columns left residency since the last drain.
    #[must_use]
    pub fn has_far_unloaded(&self) -> bool {
        !self.far_terrain.unloaded.is_empty()
    }

    /// Records columns leaving residency for [`WorldStream::drain_far_unloaded`].
    pub(super) fn note_far_unloaded(&mut self, columns: &BTreeSet<ChunkKey>) {
        if self.far_terrain.enabled {
            self.far_terrain.unloaded.extend(columns.iter().copied());
        }
    }

    /// True while the column holds blocks, including during a resend.
    fn far_column_resident(&self, column: ChunkKey) -> bool {
        self.loaded_columns.contains(&column) || self.resident.column(column).next().is_some()
    }

    /// Appends the newest summary of each column finished since the last call.
    pub fn drain_far_summaries(&mut self, out: &mut Vec<FarColumnSummary>) {
        let start = out.len();
        self.far_terrain.inbox.drain(out);
        for summary in &mut out[start..] {
            summary.resident = self.far_column_resident(summary.column);
        }
        self.note_far_summaries_delivered(&out[start..]);
    }

    /// Delivers a bounded share of summaries, acknowledging only those delivered now.
    pub fn drain_far_summaries_budgeted(
        &mut self,
        out: &mut Vec<FarColumnSummary>,
        budget: &mut SummaryBudget,
    ) {
        let start = out.len();
        self.far_terrain.inbox.drain_budgeted(out, budget);
        for summary in &mut out[start..] {
            summary.resident = self.far_column_resident(summary.column);
        }
        self.note_far_summaries_delivered(&out[start..]);
    }

    /// Summarises every complete column before the stream is dropped, and hands over the
    /// inbox their summaries arrive in, so edits made this session are not lost.
    pub fn retire_far_terrain(&mut self) -> FarSummaryInbox {
        if self.far_terrain.enabled {
            let columns = self.loaded_columns.clone();
            let mut snapshots: Vec<_> = self.far_terrain.retiring.drain(..).collect();
            snapshots.extend(self.far_retiring_snapshots(&columns));
            self.spawn_far_summaries(snapshots);
            self.far_terrain.inbox.pump();
        }
        self.far_terrain.enabled = false;
        self.far_terrain.pending.clear();
        self.far_terrain.queued.clear();
        self.far_terrain.retiring.clear();
        std::mem::take(&mut self.far_terrain.inbox)
    }

    /// Queues a column whose every section has arrived, including a resend.
    pub(super) fn note_far_column_complete(&mut self, column: ChunkKey) {
        self.note_near_drawn(column);
        let feed = &mut self.far_terrain;
        if feed.enabled && feed.queued.insert(column) {
            feed.pending.push_back(column);
        }
    }

    /// Captures a bounded share of sources and attempts admission below the near-work lanes.
    pub(super) fn dispatch_far_summaries(&mut self) {
        if !self.far_terrain.inbox.producer.sources.is_empty() {
            self.far_terrain.inbox.pump();
            return;
        }
        for _ in 0..JOBS_PER_POLL {
            let next = self.far_terrain.retiring.pop_front().or_else(|| {
                while let Some(column) = self.far_terrain.pending.pop_front() {
                    self.far_terrain.queued.remove(&column);
                    if self.loaded_columns.contains(&column) {
                        return Some((column, self.far_snapshot(column)));
                    }
                }
                None
            });
            let Some((column, snapshot)) = next else {
                break;
            };
            self.spawn_far_summaries(vec![(column, snapshot)]);
        }
        self.far_terrain.inbox.pump();
    }

    /// Snapshots complete columns about to retire so their final state is summarised.
    pub(super) fn far_retiring_snapshots(
        &self,
        columns: &BTreeSet<ChunkKey>,
    ) -> Vec<(ChunkKey, ColumnSnapshot)> {
        if !self.far_terrain.enabled {
            return Vec::new();
        }
        columns
            .iter()
            .filter(|column| self.loaded_columns.contains(column))
            .map(|&column| (column, self.far_snapshot(column)))
            .collect()
    }

    /// The column's sections, or none for a complete column the server sent as all air.
    fn far_snapshot(&self, column: ChunkKey) -> ColumnSnapshot {
        self.authority.terrain().chunk(column).map_or_else(
            || ColumnSnapshot::new(Arc::default(), Vec::new()),
            ColumnSnapshot::of,
        )
    }

    /// Retains final immutable sources; global admission decides when their copies run.
    pub(super) fn spawn_far_summaries(&mut self, columns: Vec<(ChunkKey, ColumnSnapshot)>) {
        for (column, snapshot) in columns {
            let ids = self.decode_ids(column.dimension);
            let prepared = match &self.far_terrain.prepared {
                Some(prepared) if prepared.matches(&ids.assets) => Arc::clone(prepared),
                _ => {
                    let prepared = PreparedAssets::shared(Arc::clone(&ids.assets));
                    self.far_terrain.prepared = Some(Arc::clone(&prepared));
                    prepared
                }
            };
            let context = PendingContext {
                prepared,
                air: ids.air,
                assets: ids.assets,
                mode: ids.mode,
                tints: ids.biome_tints,
                default_biome: ids.default_biome,
            };
            self.far_terrain.inbox.enqueue(column, snapshot, context);
        }
    }
}

#[cfg(test)]
mod delivery_tests;

#[cfg(test)]
mod producer_tests;
