//! The mesher thread: mesh jobs on the Lod lane, the snapshots they read, their results
//! and the colour tables.

use std::{
    cell::RefCell,
    io,
    sync::{Arc, atomic::Ordering},
};

use lod::{
    codec::SectionCodec,
    mesh::{ExteriorCulling, cache::MeshCache, cache::MeshJob, class::MeshClasses, mesher::Mesher},
};
use lod_store::{
    actor::{ViewChange, ViewEpoch},
    identity::Dimension,
};

use super::{
    blocks::{MaterialClassTable, SessionBlocks},
    colours::{ColourTableBuilder, biome_tints},
    meshes::{Interest, JobResult, LOD_MESH_CACHE_BYTES, MAX_LOD_MESH_JOBS, MeshInbox, MeshShared},
    snapshots::{HiddenColumns, MeshSnapshot, SectionSnapshots, SnapshotError},
};
use crate::stream::workers::{Lane, WORKERS};

thread_local! {
    /// Each Lod-lane worker's mesher and its scratch, reused between jobs.
    static MESHER: RefCell<Mesher> = RefCell::new(Mesher::new());
    /// Each Lod-lane worker's record decoder.
    static CODEC: RefCell<SectionCodec> = RefCell::new(SectionCodec::new());
}

/// Starts the mesher thread for `shared`; it stops once meshing is turned off.
pub(super) fn spawn(shared: Arc<MeshShared>) -> io::Result<()> {
    std::thread::Builder::new()
        .name("lod-mesher".into())
        .spawn(move || {
            crate::stream::workers::lower_beside_pool("lod-mesher");
            let mut state = MesherState::new();
            while let Some(inbox) = shared.wait() {
                state.step(&shared, inbox);
            }
        })
        .map(drop)
}

/// Decodes `snapshot` and meshes it with this worker's scratch, then posts the result.
fn run_job(
    shared: &MeshShared,
    classes: &MeshClasses,
    generation: u64,
    job: MeshJob,
    snapshot: MeshSnapshot,
) {
    let mut decoded = Vec::new();
    let sections = CODEC.with_borrow_mut(|codec| snapshot.decode(codec, &mut decoded));
    let mesh = sections.map(|sections| {
        let input = sections.input();
        MESHER.with_borrow_mut(|mesher| mesher.mesh(&input, classes, ExteriorCulling::SectionLocal))
    });
    shared.post(JobResult {
        generation,
        job,
        mesh,
        decoded,
    });
}

/// The mesher thread's state.
struct MesherState {
    cache: MeshCache,
    /// Moves on whenever the cache is replaced, so results of an earlier one are dropped.
    generation: u64,
    dimension: Option<Dimension>,
    sections: SectionSnapshots,
    /// What read-through showed when the thread last looked.
    view: Option<ViewEpoch>,
    blocks: Option<Arc<SessionBlocks>>,
    class_table: MaterialClassTable,
    /// Mesh classes and the registry length they cover.
    classes: Option<(Arc<MeshClasses>, usize)>,
    colours: ColourTableBuilder,
    tints: Option<Arc<assets::ResolvedBiomeTints>>,
}

impl MesherState {
    /// A thread with nothing wanted.
    fn new() -> Self {
        Self {
            cache: MeshCache::new(LOD_MESH_CACHE_BYTES),
            generation: 0,
            dimension: None,
            sections: SectionSnapshots::default(),
            view: None,
            blocks: None,
            class_table: MaterialClassTable::default(),
            classes: None,
            colours: ColourTableBuilder::default(),
            tints: None,
        }
    }

    /// Takes in `inbox`, starts what jobs it can and publishes results and tables.
    fn step(&mut self, shared: &Arc<MeshShared>, inbox: MeshInbox) {
        #[cfg(feature = "tracy")]
        let _zone = tracing::info_span!("stream.lod_mesher").entered();
        if inbox.dimension != self.dimension {
            self.dimension = inbox.dimension;
            self.cache = MeshCache::new(LOD_MESH_CACHE_BYTES);
            // Meshes of the old dimension not yet taken are neither wanted nor counted.
            shared.lock_output().fresh.clear();
            self.generation += 1;
            self.sections.clear();
        }
        for result in inbox.results {
            if result.generation == self.generation {
                for decoded in result.decoded {
                    self.sections.decoded(decoded);
                }
                self.cache.complete(result.job, result.mesh);
            }
        }
        for interest in inbox.interest {
            match interest {
                Interest::Want(pos) => self.cache.want(pos),
                Interest::Release(key) => self.cache.release(key),
            }
        }
        for (dimension, key) in inbox.written {
            if Some(dimension) == self.dimension {
                self.sections.remove(key);
                self.cache.changed(key.pos());
            }
        }
        self.follow_view(shared);
        if let Some(blocks) = inbox.blocks {
            self.refresh_tables(shared, &blocks);
        }
        if let Some(tints) = inbox.tints
            && !self
                .tints
                .as_ref()
                .is_some_and(|known| Arc::ptr_eq(known, &tints))
        {
            let table = Arc::new(biome_tints(&tints));
            self.tints = Some(tints);
            shared.lock_output().biomes = Some(table);
        }
        self.dispatch(shared);
        self.publish(shared);
    }

    /// Meshes again what read-through now shows differently: the sections overlapping
    /// newly hidden regions of the current dimension and their readers, or every section
    /// after a session change.
    fn follow_view(&mut self, shared: &MeshShared) {
        let Some(seen) = self.view else {
            self.view = Some(shared.view_epoch());
            return;
        };
        let (view, change) = shared.client.view_changes(seen);
        self.view = Some(view);
        match change {
            ViewChange::Unchanged => {}
            ViewChange::Hidden(regions) => {
                let columns = HiddenColumns::of(
                    regions
                        .into_iter()
                        .filter(|(dimension, _)| Some(*dimension) == self.dimension)
                        .map(|(_, region)| region),
                );
                self.sections.forget_columns(&columns);
                self.cache.changed_where(|pos| columns.covers(pos));
            }
            ViewChange::All => {
                self.sections.clear();
                self.cache.changed_all();
            }
        }
    }

    /// Rebuilds the mesh classes and colours when the session's identities changed, and
    /// extends them as the registry grows. New identities mesh every section again.
    fn refresh_tables(&mut self, shared: &MeshShared, blocks: &Arc<SessionBlocks>) {
        let registry = shared.client.registry();
        let identities_changed = !self
            .blocks
            .as_ref()
            .is_some_and(|known| Arc::ptr_eq(known, blocks));
        let materials = registry
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let stale_classes = self
            .classes
            .as_ref()
            .is_none_or(|(_, covered)| *covered != materials);
        if identities_changed || stale_classes {
            self.class_table.refresh(blocks, &registry);
            let count = u32::try_from(materials + 1).unwrap_or(u32::MAX);
            self.classes = Some((
                Arc::new(MeshClasses::new(&self.class_table, count)),
                materials,
            ));
        }
        if identities_changed {
            if self.blocks.is_some() {
                self.cache.changed_all();
            }
            self.blocks = Some(Arc::clone(blocks));
        }
        if self.colours.refresh(blocks, &registry) {
            shared.lock_output().colours = Some(Arc::clone(self.colours.table()));
        }
    }

    /// Starts jobs while fewer than [`MAX_LOD_MESH_JOBS`] run. A section no record holds
    /// completes at once without a mesh; jobs whose reads failed or found the store's queue
    /// full wait for the frame's next nudge.
    fn dispatch(&mut self, shared: &Arc<MeshShared>) {
        let (Some(dimension), Some((classes, _))) = (self.dimension, &self.classes) else {
            return;
        };
        let classes = Arc::clone(classes);
        let mut failed = Vec::new();
        while self.cache.stats().in_flight < MAX_LOD_MESH_JOBS {
            let Some(job) = self.cache.next_job() else {
                break;
            };
            match self.sections.snapshot(shared, dimension, job.pos) {
                Ok(Some(snapshot)) => {
                    let (shared, classes, generation) =
                        (Arc::clone(shared), Arc::clone(&classes), self.generation);
                    WORKERS.spawn(Lane::Lod, move || {
                        run_job(&shared, &classes, generation, job, snapshot);
                    });
                }
                Ok(None) => {
                    self.cache.complete(job, None);
                }
                Err(SnapshotError::Failed) => failed.push(job),
                Err(SnapshotError::Full) => {
                    failed.push(job);
                    break;
                }
                Err(SnapshotError::Stopped) => {
                    self.cache.defer(job);
                    shared.disable();
                    break;
                }
            }
        }
        if !failed.is_empty() {
            for job in failed.into_iter().rev() {
                self.cache.defer(job);
            }
            shared.retry_due.store(true, Ordering::Release);
        }
    }

    /// Hands finished meshes to the frame once it took the previous ones, which count
    /// against the cache's budget until then, and the counters.
    fn publish(&mut self, shared: &MeshShared) {
        let mut output = shared.lock_output();
        if output.fresh.is_empty() {
            self.cache.received();
            self.cache.take_fresh(&mut output.fresh);
        }
        output.cache = self.cache.stats();
    }
}
