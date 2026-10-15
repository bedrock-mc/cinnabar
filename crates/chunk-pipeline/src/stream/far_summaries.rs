//! Far-terrain summaries of complete and retiring columns, extracted on world workers.

use std::sync::OnceLock;

use crossbeam_channel::unbounded;
use far_terrain::{
    ColumnSnapshot, EncodedSummary, SummaryBudget, SurfaceColours, SurfaceContext, summarise_column,
};

use super::*;

/// Columns summarised by one decode-lane job.
const COLUMNS_PER_JOB: usize = 8;
/// Summary jobs started per poll while no terrain waits to decode.
const JOBS_PER_POLL: usize = 2;
/// Summary jobs started per poll while terrain decode is queued, so arriving terrain stays
/// ahead of summaries without starving them during long flights.
const JOBS_PER_BUSY_POLL: usize = 1;
/// Summary jobs on the workers at once.
const MAX_JOBS_IN_FLIGHT: usize = 4;

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

type SharedColours = Arc<OnceLock<Arc<SurfaceColours>>>;

/// One summary job's result, tagged with the generation it was issued under.
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
    tx: Sender<Finished>,
    rx: Receiver<Finished>,
    issued: HashMap<ChunkKey, Issued>,
    next_generation: u64,
    deferred: Option<Finished>,
}

impl Default for FarSummaryInbox {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            tx,
            rx,
            issued: HashMap::new(),
            next_generation: 0,
            deferred: None,
        }
    }
}

impl FarSummaryInbox {
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

    /// Appends the newest summary of each column finished since the last call.
    pub fn drain(&mut self, out: &mut Vec<FarColumnSummary>) {
        while let Some(finished) = self.deferred.take().or_else(|| self.rx.try_recv().ok()) {
            self.accept(finished, out);
        }
    }

    /// Appends finished summaries within both limits, counting stale results as work too.
    pub fn drain_budgeted(&mut self, out: &mut Vec<FarColumnSummary>, budget: &mut SummaryBudget) {
        while let Some(finished) = self.deferred.take().or_else(|| self.rx.try_recv().ok()) {
            if !budget.take(finished.summary.bytes.len()) {
                self.deferred = Some(finished);
                return;
            }
            self.accept(finished, out);
        }
    }

    /// Like [`Self::drain`], first blocking until `deadline` for one summary while work is
    /// issued and none has finished.
    pub fn drain_waiting(&mut self, out: &mut Vec<FarColumnSummary>, deadline: Instant) {
        if self.deferred.is_none()
            && self.rx.is_empty()
            && !self.is_idle()
            && let Ok(finished) = self.rx.recv_deadline(deadline)
        {
            self.accept(finished, out);
        }
        self.drain(out);
    }

    fn accept(&mut self, finished: Finished, out: &mut Vec<FarColumnSummary>) {
        let Some(issued) = self.issued.get_mut(&finished.column) else {
            return;
        };
        issued.in_flight -= 1;
        let current = issued.latest == finished.generation;
        if issued.in_flight == 0 {
            self.issued.remove(&finished.column);
        }
        if current {
            out.push(FarColumnSummary {
                column: finished.column,
                summary: finished.summary,
                generation: finished.generation,
                resident: false,
            });
        }
    }

    /// True when finished summaries wait to be drained.
    #[must_use]
    pub fn has_finished(&self) -> bool {
        self.deferred.is_some() || !self.rx.is_empty()
    }

    /// True once every issued summary has been drained.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.issued.is_empty()
    }
}

pub(super) struct FarTerrainFeed {
    enabled: bool,
    /// Material colours of one asset set, computed by the first job that needs them.
    colours: Option<(Arc<RuntimeAssets>, SharedColours)>,
    pub(super) pending: VecDeque<ChunkKey>,
    queued: HashSet<ChunkKey>,
    /// Columns that left residency, with the blocks they held, awaiting a summary job.
    pub(super) retiring: VecDeque<(ChunkKey, ColumnSnapshot)>,
    /// Columns that left residency since the last drain, whatever became of their summaries.
    unloaded: Vec<ChunkKey>,
    jobs_in_flight: Arc<AtomicUsize>,
    inbox: FarSummaryInbox,
}

impl Default for FarTerrainFeed {
    fn default() -> Self {
        Self {
            enabled: false,
            colours: None,
            pending: VecDeque::new(),
            queued: HashSet::new(),
            retiring: VecDeque::new(),
            unloaded: Vec::new(),
            jobs_in_flight: Arc::default(),
            inbox: FarSummaryInbox::default(),
        }
    }
}

impl WorldStream {
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
        feed.pending.is_empty()
            && feed.retiring.is_empty()
            && feed.jobs_in_flight.load(Ordering::Acquire) == 0
            && feed.inbox.is_idle()
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

    /// Starts a bounded number of summary jobs, retiring columns first; terrain still waiting
    /// to decode keeps priority on the shared lane.
    pub(super) fn dispatch_far_summaries(&mut self) {
        let decoding = !self.pending_decode.is_empty() || self.in_flight_decode_jobs > 0;
        let jobs = if decoding {
            JOBS_PER_BUSY_POLL
        } else {
            JOBS_PER_POLL
        };
        for _ in 0..jobs {
            if self.far_terrain.jobs_in_flight.load(Ordering::Acquire) >= MAX_JOBS_IN_FLIGHT {
                return;
            }
            let mut batch = Vec::with_capacity(COLUMNS_PER_JOB);
            while batch.len() < COLUMNS_PER_JOB {
                let Some(retiring) = self.far_terrain.retiring.pop_front() else {
                    break;
                };
                batch.push(retiring);
            }
            while batch.len() < COLUMNS_PER_JOB {
                let Some(column) = self.far_terrain.pending.pop_front() else {
                    break;
                };
                self.far_terrain.queued.remove(&column);
                // A column evicted since it completed was summarised as it retired.
                if self.loaded_columns.contains(&column) {
                    batch.push((column, self.far_snapshot(column)));
                }
            }
            if batch.is_empty() {
                return;
            }
            self.spawn_far_summaries(batch);
        }
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

    pub(super) fn spawn_far_summaries(&mut self, columns: Vec<(ChunkKey, ColumnSnapshot)>) {
        let mut columns = columns.into_iter().peekable();
        while columns.peek().is_some() {
            let batch: Vec<_> = columns.by_ref().take(COLUMNS_PER_JOB).collect();
            let contexts = self.far_contexts(&batch);
            let generations: Vec<u64> = batch
                .iter()
                .map(|(column, _)| self.far_terrain.inbox.issue(*column))
                .collect();
            let tx = self.far_terrain.inbox.tx.clone();
            let in_flight = Arc::clone(&self.far_terrain.jobs_in_flight);
            in_flight.fetch_add(1, Ordering::AcqRel);
            workers::WORKERS.spawn(workers::Lane::Decode, move || {
                #[cfg(feature = "tracy")]
                let _zone = tracing::info_span!("far_terrain.summarise").entered();
                for (((column, snapshot), context), generation) in
                    batch.into_iter().zip(contexts).zip(generations)
                {
                    let summary = summarise_column(&context.resolve(), &snapshot).encode();
                    let finished = Finished {
                        column,
                        generation,
                        summary,
                    };
                    workers::send_result(&tx, finished);
                }
                in_flight.fetch_sub(1, Ordering::AcqRel);
            });
        }
    }

    fn far_contexts(&mut self, batch: &[(ChunkKey, ColumnSnapshot)]) -> Vec<PendingContext> {
        let assets = Arc::clone(self.authority.runtime_assets());
        let colours = match &self.far_terrain.colours {
            Some((cached, colours)) if Arc::ptr_eq(cached, &assets) => Arc::clone(colours),
            _ => {
                let colours = SharedColours::default();
                self.far_terrain.colours = Some((Arc::clone(&assets), Arc::clone(&colours)));
                colours
            }
        };
        batch
            .iter()
            .map(|(column, _)| {
                let ids = self.decode_ids(column.dimension);
                PendingContext {
                    colours: Arc::clone(&colours),
                    air: ids.air,
                    assets: ids.assets,
                    mode: ids.mode,
                    tints: ids.biome_tints,
                    default_biome: ids.default_biome,
                }
            })
            .collect()
    }
}

/// Registries captured on the owner thread; colours resolve on the worker.
struct PendingContext {
    colours: SharedColours,
    air: u32,
    assets: Arc<RuntimeAssets>,
    mode: NetworkIdMode,
    tints: Arc<ResolvedBiomeTints>,
    default_biome: u32,
}

impl PendingContext {
    fn resolve(self) -> SurfaceContext {
        let colours = Arc::clone(
            self.colours
                .get_or_init(|| Arc::new(SurfaceColours::new(&self.assets))),
        );
        SurfaceContext {
            air: self.air,
            assets: self.assets,
            mode: self.mode,
            colours,
            tints: self.tints,
            default_biome: self.default_biome,
        }
    }
}

#[cfg(test)]
mod delivery_tests;
