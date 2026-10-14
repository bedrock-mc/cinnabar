//! Distant terrain meshes kept on the CPU for upload: an opt-in Cinnabar extension, not
//! vanilla behaviour.
//!
//! The renderer names the sections it wants ([`WorldStream::want_distant_terrain_meshes`])
//! and releases them, in order. A mesher thread owns a [`lod::mesh::cache::MeshCache`],
//! reads the record of every section a mesh needs as the session sees it (read-through
//! included) into a byte-bounded snapshot cache ([`super::snapshots`]), the uncached ones in
//! one batched store read per mesh, and queues each mesh as a job on the background-only
//! Lod lane with its records, so mesh workers never load sections and decode them in
//! parallel; decoded sections go back to the cache. Workers mesh with a per-thread
//! [`lod::mesh::mesher::Mesher`] and every job posts exactly one result; a result for an
//! older version of its section is dropped and the section meshed again. Sections the
//! writers store mesh them and their readers again; a region hidden from read-through
//! ([`lod_store::actor::ViewChange::Hidden`]) does so for the sections overlapping it, and
//! a session or leading epoch change for every section. A failed or refused read defers
//! the job until the frame's next nudge. The mesher also keeps the colour tables
//! ([`super::colours`]) of the session's assets and store materials.

use std::sync::{
    Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak,
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
};

use assets::ResolvedBiomeTints;
use lod::{
    colour::{BiomeTints, MaterialColours},
    geometry::{SectionKey, SectionPos},
    mesh::{
        cache::{FreshMesh, MeshCacheStats, MeshJob},
        output::SectionMesh,
    },
};
use lod_store::{
    actor::{StoreClient, ViewEpoch},
    identity::Dimension,
    minute::Minute,
};

use super::{blocks::SessionBlocks, hooks::session_blocks, mesher, snapshots::DecodedSection};
use crate::stream::WorldStream;

/// Bytes of finished meshes the CPU keeps for upload: a share of distant terrain's CPU
/// memory cap.
pub const LOD_MESH_CACHE_BYTES: usize = 64 << 20;
/// Mesh jobs queued on the Lod lane at once.
pub const MAX_LOD_MESH_JOBS: usize = 16;
/// Bytes of store records and decoded sections the mesher keeps for mesh snapshots: a
/// share of distant terrain's CPU memory cap.
pub(super) const LOD_MESH_SNAPSHOT_BYTES: usize = 24 << 20;

/// Distant terrain mesh counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DistantTerrainMeshStats {
    pub cache: MeshCacheStats,
    /// Sections read from the store for snapshots.
    pub section_reads: u64,
    /// Section reads the store failed; the jobs reading them waited for the next poll and
    /// read again.
    pub read_failures: u64,
    /// Meshes waiting for the renderer to take them.
    pub ready: usize,
    /// True once meshing stopped because the store stopped; nothing more is meshed until
    /// meshing is enabled again with a working store.
    pub stopped: bool,
}

/// The colour tables of the session's assets for the store's materials and biomes.
#[derive(Debug, Clone)]
pub struct DistantTerrainColours {
    pub materials: Arc<MaterialColours>,
    /// `None` until the session's biome tints are known.
    pub biomes: Option<Arc<BiomeTints>>,
}

/// Told when finished meshes wait in the output and when writers store sections; the
/// selection thread listens, so it never polls.
pub(super) trait MeshObserver: Send + Sync {
    /// Finished meshes wait to be taken.
    fn meshes_ready(&self);
    /// A writer stored `keys` in `dimension`.
    fn written(&self, dimension: Dimension, keys: &[SectionKey]);
}

/// One change to which sections the renderer wants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Interest {
    Want(SectionPos),
    Release(SectionKey),
    /// Releases every section, as when a new selection takes over.
    ReleaseAll,
}

/// One finished job: the mesher generation it started in, the job, its mesh and the
/// sections it decoded.
pub(super) struct JobResult {
    pub(super) generation: u64,
    pub(super) job: MeshJob,
    pub(super) mesh: Option<SectionMesh>,
    pub(super) decoded: Vec<DecodedSection>,
}

/// What the frame and the writers hand the mesher thread.
#[derive(Default)]
pub(super) struct MeshInbox {
    /// The dimension wanted sections are in; a change drops every mesh.
    pub(super) dimension: Option<Dimension>,
    /// Wants and releases in the order the renderer sent them.
    pub(super) interest: Vec<Interest>,
    pub(super) written: Vec<(Dimension, SectionKey)>,
    pub(super) results: Vec<JobResult>,
    pub(super) blocks: Option<Arc<SessionBlocks>>,
    pub(super) tints: Option<Arc<ResolvedBiomeTints>>,
    /// Inputs changed since the thread last looked, or the frame nudged it.
    pending: bool,
}

/// What the mesher thread hands back.
#[derive(Default)]
pub(super) struct MeshOutput {
    pub(super) fresh: Vec<FreshMesh>,
    /// The dimension `fresh` was meshed in.
    pub(super) fresh_dimension: Option<Dimension>,
    pub(super) colours: Option<Arc<MaterialColours>>,
    pub(super) biomes: Option<Arc<BiomeTints>>,
    pub(super) cache: MeshCacheStats,
}

/// State shared by the frame, the writers, the mesher thread and its jobs.
pub(in crate::stream) struct MeshShared {
    pub(super) client: StoreClient,
    now: AtomicU32,
    enabled: AtomicBool,
    inbox: Mutex<MeshInbox>,
    /// Signalled when the inbox gains work or meshing stops.
    ready: Condvar,
    output: Mutex<MeshOutput>,
    /// The thread waits for the frame's nudge to retry reads a full store queue refused.
    pub(super) retry_due: AtomicBool,
    pub(super) section_reads: AtomicU64,
    pub(super) read_failures: AtomicU64,
    /// Held weakly: the observer owns this state.
    observer: Mutex<Option<Weak<dyn MeshObserver>>>,
}

impl MeshShared {
    fn new(client: StoreClient, now: Minute) -> Self {
        Self {
            client,
            now: AtomicU32::new(now.get()),
            enabled: AtomicBool::new(true),
            inbox: Mutex::new(MeshInbox::default()),
            ready: Condvar::new(),
            output: Mutex::new(MeshOutput::default()),
            retry_due: AtomicBool::new(false),
            section_reads: AtomicU64::new(0),
            read_failures: AtomicU64::new(0),
            observer: Mutex::new(None),
        }
    }

    /// Makes `observer` the one told about meshes and writes, or none.
    pub(super) fn set_observer(&self, observer: Option<Weak<dyn MeshObserver>>) {
        *self.observer.lock().unwrap_or_else(PoisonError::into_inner) = observer;
    }

    /// The observer, while it lives.
    pub(super) fn observer(&self) -> Option<Arc<dyn MeshObserver>> {
        self.observer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .and_then(Weak::upgrade)
    }

    /// Wants `want` and releases `release`, in that order, in `dimension`, if `alive` holds
    /// when the inbox is locked; wanting another dimension than before drops every mesh
    /// and the interest not yet applied. A sender that stops before
    /// [`Self::release_all`] sees `alive` fail, so its interest never follows that release.
    pub(super) fn send_interest(
        &self,
        alive: impl FnOnce() -> bool,
        dimension: Dimension,
        want: impl IntoIterator<Item = SectionPos>,
        release: impl IntoIterator<Item = SectionKey>,
    ) {
        self.send(|inbox| {
            if !alive() {
                return;
            }
            if inbox.dimension != Some(dimension) {
                inbox.dimension = Some(dimension);
                inbox.interest.clear();
            }
            inbox.interest.extend(want.into_iter().map(Interest::Want));
            inbox
                .interest
                .extend(release.into_iter().map(Interest::Release));
        });
    }

    /// Releases every wanted section and drops the meshes not yet taken.
    pub(super) fn release_all(&self) {
        self.send(|inbox| inbox.interest.push(Interest::ReleaseAll));
    }

    /// Moves the finished meshes of `dimension` into `out` and lets the thread hand out
    /// more, if `alive` holds while the output is locked; meshes of another dimension,
    /// finished before the mesher saw the change, are dropped. A taker that stops before
    /// [`Self::release_all`] sees `alive` fail, so it never takes the meshes finished for
    /// whoever wants sections after that release.
    pub(in crate::stream) fn take_fresh(
        &self,
        alive: impl FnOnce() -> bool,
        dimension: Dimension,
        out: &mut Vec<FreshMesh>,
    ) {
        let mut output = self.lock_output();
        if !alive() {
            return;
        }
        let taken = std::mem::take(&mut output.fresh);
        let current = output.fresh_dimension == Some(dimension);
        drop(output);
        if !taken.is_empty() {
            if current {
                out.extend(taken);
            }
            self.send(|_| {});
        }
    }

    fn lock_inbox(&self) -> MutexGuard<'_, MeshInbox> {
        self.inbox.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn lock_output(&self) -> MutexGuard<'_, MeshOutput> {
        self.output.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Changes `inbox` and wakes the thread; does nothing once meshing stopped, as no
    /// thread drains the inbox then.
    fn send(&self, change: impl FnOnce(&mut MeshInbox)) {
        let mut inbox = self.lock_inbox();
        if !self.enabled() {
            return;
        }
        change(&mut inbox);
        inbox.pending = true;
        drop(inbox);
        self.ready.notify_all();
    }

    /// Stops the thread; results of jobs in flight are dropped.
    pub(super) fn disable(&self) {
        self.enabled.store(false, Ordering::Release);
        self.ready.notify_all();
    }

    /// False once meshing stopped.
    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }

    /// The minute stamped on store reads.
    pub(super) fn now(&self) -> Minute {
        Minute::new(self.now.load(Ordering::Relaxed))
    }

    /// Records that a writer stored `keys` in `dimension`, so their meshes and those
    /// reading them are made again.
    pub(in crate::stream) fn written(&self, dimension: Dimension, keys: &[SectionKey]) {
        if keys.is_empty() {
            return;
        }
        self.send(|inbox| {
            inbox
                .written
                .extend(keys.iter().map(|&key| (dimension, key)));
        });
        if let Some(observer) = self.observer() {
            observer.written(dimension, keys);
        }
    }

    /// Posts a job's result; every job started posts exactly one.
    pub(super) fn post(&self, result: JobResult) {
        self.send(|inbox| inbox.results.push(result));
    }

    /// Blocks the mesher thread until its inputs changed; `None` once meshing stopped.
    pub(super) fn wait(&self) -> Option<MeshInbox> {
        let mut inbox = self.lock_inbox();
        loop {
            if !self.enabled() {
                return None;
            }
            if inbox.pending {
                let taken = MeshInbox {
                    dimension: inbox.dimension,
                    interest: std::mem::take(&mut inbox.interest),
                    written: std::mem::take(&mut inbox.written),
                    results: std::mem::take(&mut inbox.results),
                    blocks: inbox.blocks.clone(),
                    tints: inbox.tints.clone(),
                    pending: false,
                };
                inbox.pending = false;
                return Some(taken);
            }
            inbox = self
                .ready
                .wait(inbox)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Wants and releases waiting for the thread.
    #[cfg(test)]
    pub(in crate::stream) fn queued_interest(&self) -> usize {
        self.lock_inbox().interest.len()
    }

    /// The view epoch the store's reads answer in now.
    pub(super) fn view_epoch(&self) -> ViewEpoch {
        self.client.view_epoch()
    }
}

/// The frame's handle of the mesher thread; dropping it stops the thread.
pub(in crate::stream) struct LodMeshes {
    shared: Arc<MeshShared>,
    /// The session identities last handed to the thread.
    blocks: Option<Arc<SessionBlocks>>,
    tints: Option<Arc<ResolvedBiomeTints>>,
    /// The store's view epoch last seen, to wake the thread when it moved on.
    view: Option<ViewEpoch>,
}

impl Drop for LodMeshes {
    fn drop(&mut self) {
        self.shared.disable();
    }
}

impl WorldStream {
    /// Starts meshing distant terrain read through `client`, stamping reads with `now`.
    /// Meshes follow the sections the writers of this stream store; a read-only store
    /// meshes what it holds. Fails when the mesher thread cannot start.
    pub fn enable_distant_terrain_meshes(
        &mut self,
        client: StoreClient,
        now: Minute,
    ) -> std::io::Result<()> {
        self.disable_distant_terrain_meshes();
        let shared = Arc::new(MeshShared::new(client, now));
        mesher::spawn(Arc::clone(&shared))?;
        if let Some(lod) = &self.lod {
            lod.shared.set_meshes(Some(Arc::clone(&shared)));
        }
        self.lod_meshes = Some(Box::new(LodMeshes {
            shared,
            blocks: None,
            tints: None,
            view: None,
        }));
        self.sync_distant_terrain_meshes();
        Ok(())
    }

    /// Stops meshing and drops every mesh.
    pub fn disable_distant_terrain_meshes(&mut self) {
        self.disable_distant_terrain_selection();
        if let Some(lod) = &self.lod {
            lod.shared.set_meshes(None);
        }
        self.lod_meshes = None;
    }

    /// The mesher the writers of a new ingestion notify, if meshing is on.
    pub(in crate::stream) fn lod_mesh_shared(&self) -> Option<Arc<MeshShared>> {
        self.lod_meshes
            .as_ref()
            .map(|meshes| Arc::clone(&meshes.shared))
    }

    /// Wants `sections` of the current dimension meshed and kept meshed as they and their
    /// neighbours change; wanting another dimension's sections drops every mesh. While
    /// selection runs ([`Self::enable_distant_terrain_selection`]) it decides what is
    /// wanted, and this must not be called.
    pub fn want_distant_terrain_meshes(&mut self, sections: impl IntoIterator<Item = SectionPos>) {
        let dimension = Dimension(self.current_dimension());
        if let Some(meshes) = &self.lod_meshes {
            meshes
                .shared
                .send_interest(|| true, dimension, sections, []);
        }
    }

    /// Stops wanting `keys`, dropping their meshes.
    pub fn release_distant_terrain_meshes(&mut self, keys: impl IntoIterator<Item = SectionKey>) {
        if let Some(meshes) = &self.lod_meshes {
            meshes.shared.send(|inbox| {
                inbox
                    .interest
                    .extend(keys.into_iter().map(Interest::Release))
            });
        }
    }

    /// Moves the meshes finished since the last call into `out`: `None` for a section no
    /// store record holds. The mesher holds new jobs back while meshes wait here. While
    /// selection runs it takes them instead, and this finds none.
    pub fn take_distant_terrain_meshes(&mut self, out: &mut Vec<FreshMesh>) {
        let dimension = Dimension(self.current_dimension());
        if let Some(meshes) = &self.lod_meshes {
            meshes.shared.take_fresh(|| true, dimension, out);
        }
    }

    /// The colour tables, once the mesher built them.
    #[must_use]
    pub fn distant_terrain_colours(&self) -> Option<DistantTerrainColours> {
        let meshes = self.lod_meshes.as_ref()?;
        let output = meshes.shared.lock_output();
        Some(DistantTerrainColours {
            materials: Arc::clone(output.colours.as_ref()?),
            biomes: output.biomes.clone(),
        })
    }

    /// Mesh counters; `None` while meshing is off.
    #[must_use]
    pub fn distant_terrain_mesh_stats(&self) -> Option<DistantTerrainMeshStats> {
        let shared = &self.lod_meshes.as_ref()?.shared;
        let output = shared.lock_output();
        Some(DistantTerrainMeshStats {
            cache: output.cache,
            section_reads: shared.section_reads.load(Ordering::Relaxed),
            read_failures: shared.read_failures.load(Ordering::Relaxed),
            ready: output.fresh.len(),
            stopped: !shared.enabled(),
        })
    }

    /// Hands the mesher the session's current identities and biome tints when they
    /// changed, and wakes it when what the store shows changed or to retry reads a full
    /// store queue refused; the poll calls it.
    pub(in crate::stream) fn sync_distant_terrain_meshes(&mut self) {
        let Some(meshes) = self.lod_meshes.as_deref_mut() else {
            return;
        };
        let mut cached = meshes.blocks.clone();
        let blocks = session_blocks(&mut cached, &self.authority);
        let tints = self.authority.resolved_biome_tints();
        let blocks_changed = !meshes
            .blocks
            .as_ref()
            .is_some_and(|known| Arc::ptr_eq(known, &blocks));
        let tints_changed = !meshes
            .tints
            .as_ref()
            .is_some_and(|known| Arc::ptr_eq(known, tints));
        let retry = meshes.shared.retry_due.swap(false, Ordering::AcqRel);
        let view = meshes.shared.view_epoch();
        let view_changed = meshes.view.replace(view) != Some(view);
        if blocks_changed || tints_changed || retry || view_changed {
            meshes.blocks = Some(Arc::clone(&blocks));
            meshes.tints = Some(Arc::clone(tints));
            let tints = Arc::clone(tints);
            meshes.shared.send(|inbox| {
                inbox.blocks = Some(blocks);
                inbox.tints = Some(tints);
            });
        }
    }

    /// Sets the minute stamped on later mesh reads.
    pub(in crate::stream) fn set_distant_terrain_mesh_minute(&mut self, now: Minute) {
        if let Some(meshes) = &self.lod_meshes {
            meshes.shared.now.store(now.get(), Ordering::Relaxed);
        }
    }
}
