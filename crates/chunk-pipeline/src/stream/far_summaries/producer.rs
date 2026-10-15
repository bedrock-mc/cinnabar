//! Inbox-owned, resumable sources survive the stream that captured them.

use far_terrain::SurfaceScan;

use super::{
    admission::{Admission, GLOBAL, JOB_OVERHEAD, Permit},
    preparation::PendingContext,
    *,
};

pub(super) type Job = Box<dyn FnOnce() + Send>;
pub(super) type Executor = Arc<dyn Fn(Job) + Send + Sync>;

/// Raw snapshots and registry allocations are retained source backlog, never worker closures.
/// Only a bounded copy of the first two block layers and matching biome data enters a job.
pub(super) struct Source {
    column: ChunkKey,
    generation: u64,
    snapshot: ColumnSnapshot,
    context: PendingContext,
    before: Option<i32>,
    scan: Option<Box<SurfaceScan>>,
    permit: Option<Permit>,
}

pub(super) enum Progress {
    Complete(EncodedSummary),
    Continue { before: i32, scan: Box<SurfaceScan> },
}

/// The permit stays attached through channel delivery and a budget-deferred result.
pub(super) struct Step {
    pub(super) column: ChunkKey,
    pub(super) generation: u64,
    pub(super) progress: Progress,
    pub(super) _permit: Permit,
}

pub(super) enum Event {
    Prepared,
    Retry {
        column: ChunkKey,
        generation: u64,
    },
    Step(Step),
    #[cfg(test)]
    Finished(Finished),
}

pub(super) struct Producer {
    pub(super) sources: HashMap<u64, Source>,
    pending: VecDeque<ChunkKey>,
    queued: HashMap<ChunkKey, u64>,
    subscriptions: HashSet<usize>,
    // Retain identities while subscribed sources exist, preventing address reuse.
    contexts: Vec<Arc<preparation::PreparedAssets>>,
    pub(super) admission: Arc<Admission>,
    executor: Executor,
}

impl Default for Producer {
    fn default() -> Self {
        Self::new(
            Arc::clone(&GLOBAL),
            Arc::new(|job| workers::WORKERS.spawn(workers::Lane::Summary, job)),
        )
    }
}

impl Producer {
    /// Installs a manual executor and isolated admission owner in tests.
    pub(super) fn new(admission: Arc<Admission>, executor: Executor) -> Self {
        Self {
            sources: HashMap::new(),
            pending: VecDeque::new(),
            queued: HashMap::new(),
            subscriptions: HashSet::new(),
            contexts: Vec::new(),
            admission,
            executor,
        }
    }

    /// Retains an immutable source before attempting any admission; no failed attempt loses it.
    pub(super) fn push(
        &mut self,
        column: ChunkKey,
        generation: u64,
        snapshot: ColumnSnapshot,
        context: PendingContext,
        tx: &Sender<Event>,
    ) -> Option<u64> {
        let identity = Arc::as_ptr(&context.prepared) as usize;
        if self.subscriptions.insert(identity) {
            context.prepared.subscribe(tx.clone());
            self.contexts.push(Arc::clone(&context.prepared));
        }
        let replaced = self.queued.insert(column, generation);
        if let Some(old) = replaced {
            self.sources.remove(&old);
        } else {
            self.pending.push_back(column);
        }
        self.sources.insert(
            generation,
            Source {
                column,
                generation,
                snapshot,
                context,
                before: None,
                scan: None,
                permit: None,
            },
        );
        if replaced.is_some() {
            self.prune_contexts();
        }
        replaced
    }

    /// Starts a finite share of copied work, retaining denied sources for a later drain/poll.
    pub(super) fn pump(&mut self, tx: &Sender<Event>, limit: usize) {
        for _ in 0..limit {
            let Some(&column) = self.pending.front() else {
                break;
            };
            let generation = self.queued[&column];
            let source = self.sources.get_mut(&generation).expect("pending source");
            let Some(context) = source.context.ready() else {
                source
                    .context
                    .prepared
                    .prepare(&self.admission, &self.executor);
                // Another identity can already be ready; do not let this dependency block it.
                self.pending.rotate_left(1);
                continue;
            };
            let slice_limit = self.admission.slice_bytes();
            let Some(mut permit) = source
                .permit
                .take()
                .or_else(|| self.admission.acquire(slice_limit + JOB_OVERHEAD))
            else {
                break;
            };
            let slice = source.snapshot.surface_slice(source.before, slice_limit);
            if slice.next().is_none() {
                permit.shrink(slice.retained_bytes() + JOB_OVERHEAD);
            }
            let mut scan = source.scan.take().unwrap_or_default();
            let column = source.column;
            let generation = source.generation;
            let mut completion = Completion {
                tx: tx.clone(),
                column,
                generation,
                permit: Some(permit),
            };
            self.pending.pop_front();
            self.queued.remove(&column);
            (self.executor)(Box::new(move || {
                #[cfg(feature = "tracy")]
                let _zone = tracing::info_span!("far_terrain.summarise").entered();
                scan.apply(&context, &slice);
                let progress = match slice.next().filter(|_| !scan.is_complete()) {
                    Some(before) => Progress::Continue { before, scan },
                    None => Progress::Complete((*scan).finish().encode()),
                };
                drop(slice);
                drop(context);
                workers::send_result(
                    &completion.tx,
                    Event::Step(Step {
                        column,
                        generation,
                        progress,
                        _permit: completion.permit.take().expect("live job permit"),
                    }),
                );
            }));
        }
    }

    /// Reuses a continuation's reservation before trying new sources. Putting it first
    /// prevents a full budget from hiding its own permit holders behind denied sources.
    pub(super) fn resume(
        &mut self,
        generation: u64,
        before: i32,
        scan: Box<SurfaceScan>,
        permit: Permit,
    ) {
        if let Some(source) = self.sources.get_mut(&generation) {
            source.before = Some(before);
            source.scan = Some(scan);
            source.permit = Some(permit);
            self.pending.push_front(source.column);
            self.queued.insert(source.column, generation);
        }
    }

    /// Restarts an abandoned copied step from its immutable source, preserving final edits.
    pub(super) fn retry(&mut self, generation: u64) {
        if let Some(source) = self.sources.get_mut(&generation) {
            source.before = None;
            source.scan = None;
            self.pending.push_back(source.column);
            self.queued.insert(source.column, generation);
        }
    }

    /// Removes source ownership only after final delivery or explicit cancellation.
    pub(super) fn complete(&mut self, generation: u64) {
        self.sources.remove(&generation);
        self.prune_contexts();
    }

    /// Releases obsolete identity subscriptions after replacement or final delivery.
    fn prune_contexts(&mut self) {
        self.contexts.retain(|context| {
            let used = self
                .sources
                .values()
                .any(|source| Arc::ptr_eq(&source.context.prepared, context));
            if !used {
                self.subscriptions.remove(&(Arc::as_ptr(context) as usize));
            }
            used
        });
    }

    /// Starts a new delivery lifetime with the same global admission owner and executor.
    pub(super) fn empty(&self) -> Self {
        Self::new(Arc::clone(&self.admission), Arc::clone(&self.executor))
    }

    /// Measures logical source payload independently of admitted copied work.
    #[cfg(test)]
    pub(super) fn source_bytes(&self) -> usize {
        self.sources
            .values()
            .map(|source| source.snapshot.retained_payload_bytes())
            .sum()
    }
}

/// Returns capacity on executor rejection/drop and leaves an inbox notice to retry its source.
struct Completion {
    tx: Sender<Event>,
    column: ChunkKey,
    generation: u64,
    permit: Option<Permit>,
}

impl Drop for Completion {
    fn drop(&mut self) {
        if self.permit.is_some() {
            workers::send_result(
                &self.tx,
                Event::Retry {
                    column: self.column,
                    generation: self.generation,
                },
            );
        }
    }
}
