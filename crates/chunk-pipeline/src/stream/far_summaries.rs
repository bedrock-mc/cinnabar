//! Far-terrain summaries of complete and retiring columns, extracted on world workers.

use std::sync::OnceLock;

use crossbeam_channel::unbounded;
use far_terrain::{
    ColumnSnapshot, EncodedSummary, SurfaceColours, SurfaceContext, summarise_column,
};

use super::*;

/// Columns summarised by one decode-lane job.
const COLUMNS_PER_JOB: usize = 8;
/// Summary jobs started per poll, so summaries never crowd out terrain decode.
const JOBS_PER_POLL: usize = 2;

/// The encoded surface summary of one column, keyed with its dimension; workers encode it
/// so the owner thread only compares hashes.
#[derive(Debug)]
pub struct FarColumnSummary {
    pub column: ChunkKey,
    pub summary: EncodedSummary,
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
}

impl Default for FarSummaryInbox {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            tx,
            rx,
            issued: HashMap::new(),
            next_generation: 0,
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

    /// Appends the newest summary of each column finished since the last call.
    pub fn drain(&mut self, out: &mut Vec<FarColumnSummary>) {
        for finished in self.rx.try_iter() {
            let Some(issued) = self.issued.get_mut(&finished.column) else {
                continue;
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
                });
            }
        }
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
    inbox: FarSummaryInbox,
}

impl Default for FarTerrainFeed {
    fn default() -> Self {
        Self {
            enabled: false,
            colours: None,
            pending: VecDeque::new(),
            queued: HashSet::new(),
            inbox: FarSummaryInbox::default(),
        }
    }
}

impl WorldStream {
    /// Starts or stops summarising columns for far terrain; off drops queued work.
    pub fn set_far_terrain_enabled(&mut self, enabled: bool) {
        let feed = &mut self.far_terrain;
        feed.enabled = enabled;
        if !enabled {
            feed.pending.clear();
            feed.queued.clear();
        }
    }

    /// Appends the newest summary of each column finished since the last call.
    pub fn drain_far_summaries(&mut self, out: &mut Vec<FarColumnSummary>) {
        self.far_terrain.inbox.drain(out);
    }

    /// Summarises every complete column before the stream is dropped, and hands over the
    /// inbox their summaries arrive in, so edits made this session are not lost.
    pub fn retire_far_terrain(&mut self) -> FarSummaryInbox {
        if self.far_terrain.enabled {
            let columns = self.loaded_columns.clone();
            let snapshots = self.far_retiring_snapshots(&columns);
            self.spawn_far_summaries(snapshots);
        }
        self.far_terrain.enabled = false;
        self.far_terrain.pending.clear();
        self.far_terrain.queued.clear();
        std::mem::take(&mut self.far_terrain.inbox)
    }

    /// Queues a column whose every section has arrived, including a resend.
    pub(super) fn note_far_column_complete(&mut self, column: ChunkKey) {
        let feed = &mut self.far_terrain;
        if feed.enabled && feed.queued.insert(column) {
            feed.pending.push_back(column);
        }
    }

    /// Starts a bounded number of summary jobs for queued complete columns.
    pub(super) fn dispatch_far_summaries(&mut self) {
        for _ in 0..JOBS_PER_POLL {
            let mut batch = Vec::with_capacity(COLUMNS_PER_JOB);
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
                    if tx.send(finished).is_err() {
                        return;
                    }
                }
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
            assets: self.assets,
            mode: self.mode,
            colours,
            tints: self.tints,
            default_biome: self.default_biome,
        }
    }
}
