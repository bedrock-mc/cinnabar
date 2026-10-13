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

pub(super) struct FarTerrainFeed {
    enabled: bool,
    /// Material colours of one asset set, computed by the first job that needs them.
    colours: Option<(Arc<RuntimeAssets>, SharedColours)>,
    pub(super) pending: VecDeque<ChunkKey>,
    queued: HashSet<ChunkKey>,
    tx: Sender<FarColumnSummary>,
    rx: Receiver<FarColumnSummary>,
}

impl Default for FarTerrainFeed {
    fn default() -> Self {
        let (tx, rx) = unbounded();
        Self {
            enabled: false,
            colours: None,
            pending: VecDeque::new(),
            queued: HashSet::new(),
            tx,
            rx,
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

    #[must_use]
    pub const fn far_terrain_enabled(&self) -> bool {
        self.far_terrain.enabled
    }

    /// The local player's column, which centres retention and far terrain.
    #[must_use]
    pub fn player_column(&self) -> ChunkKey {
        self.player_chunk()
    }

    /// Appends summaries finished since the last call.
    pub fn drain_far_summaries(&self, out: &mut Vec<FarColumnSummary>) {
        out.extend(self.far_terrain.rx.try_iter());
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
                if let Some(chunk) = self.authority.terrain().chunk(column) {
                    batch.push((column, ColumnSnapshot::of(chunk)));
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
            .filter_map(|&column| {
                Some((
                    column,
                    ColumnSnapshot::of(self.authority.terrain().chunk(column)?),
                ))
            })
            .collect()
    }

    pub(super) fn spawn_far_summaries(&mut self, columns: Vec<(ChunkKey, ColumnSnapshot)>) {
        let mut columns = columns.into_iter().peekable();
        while columns.peek().is_some() {
            let batch: Vec<_> = columns.by_ref().take(COLUMNS_PER_JOB).collect();
            let contexts = self.far_contexts(&batch);
            let tx = self.far_terrain.tx.clone();
            workers::WORKERS.spawn(workers::Lane::Decode, move || {
                #[cfg(feature = "tracy")]
                let _zone = tracing::info_span!("far_terrain.summarise").entered();
                for ((column, snapshot), context) in batch.into_iter().zip(contexts) {
                    let summary = summarise_column(&context.resolve(), &snapshot).encode();
                    if tx.send(FarColumnSummary { column, summary }).is_err() {
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
