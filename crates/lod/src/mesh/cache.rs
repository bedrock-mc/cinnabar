//! The meshes a renderer wants: which sections need meshing, in which version, and the
//! finished meshes kept on the CPU for upload, within a byte budget.
//!
//! Every wanted section has a version that moves on whenever the section or a section its
//! mesh reads ([`dependents`]) is written. A job meshes one version; a result for an older
//! version is dropped as stale and the section meshed again, and every job ends in exactly
//! one [`MeshCache::complete`]. Finished meshes wait as fresh until they are handed out
//! ([`MeshCache::take_fresh`]), and count against the budget until the receiver took them
//! ([`MeshCache::received`]), handed meshes the cache dropped or replaced meanwhile
//! included. While fresh and handed meshes fill the budget no new job starts, so the meshes
//! alive at once take at most the budget plus the results of jobs in flight. Taken meshes
//! stay cached for upload again, the least recently used dropped first once the budget is
//! exceeded.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::Arc,
};

use crate::{
    geometry::{SectionKey, SectionPos},
    mesh::{
        input::{dependents, reads},
        output::SectionMesh,
    },
};

/// One section to mesh in one version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshJob {
    pub pos: SectionPos,
    pub version: u64,
}

/// What [`MeshCache::complete`] did with a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Completion {
    /// Kept as the section's fresh mesh.
    Stored,
    /// The section changed after the job started; it is meshed again.
    Stale,
    /// The section is no longer wanted.
    Unwanted,
}

/// A finished mesh the renderer has not taken: `None` when no section is stored there.
pub type FreshMesh = (SectionKey, Option<Arc<SectionMesh>>);

/// Lifetime counters of a [`MeshCache`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MeshCacheStats {
    /// Sections wanted now.
    pub wanted: usize,
    /// Wanted sections waiting for a job.
    pub dirty: usize,
    /// Jobs started and not completed.
    pub in_flight: usize,
    /// Bytes of meshes held, fresh, handed out or taken.
    pub held_bytes: usize,
    /// Bytes of fresh meshes.
    pub fresh_bytes: usize,
    /// Bytes of meshes handed out and not yet received, held or not.
    pub handed_bytes: usize,
    pub jobs: u64,
    pub stored: u64,
    pub stale: u64,
    pub unwanted: u64,
    /// Taken meshes dropped to stay within the budget.
    pub evicted: u64,
}

/// Where a held mesh is on its way to the receiver.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Stage {
    /// Received, or nothing held: a held mesh may be dropped for the budget.
    #[default]
    Taken,
    /// Finished and not yet handed out.
    Fresh,
    /// Handed out and not yet received.
    Handed,
}

/// One wanted section.
#[derive(Debug, Default)]
struct Entry {
    /// Moves on with every change of the section or what its mesh reads.
    version: u64,
    /// The version of the held mesh or missing-section result, if any.
    meshed: Option<u64>,
    /// The version a job is meshing.
    in_flight: Option<u64>,
    /// Listed in [`MeshCache::dirty`].
    queued: bool,
    mesh: Option<Arc<SectionMesh>>,
    stage: Stage,
    /// Last use, for dropping taken meshes least recently used first.
    used: u64,
}

impl Entry {
    /// Heap bytes of the held mesh.
    fn bytes(&self) -> usize {
        self.mesh.as_ref().map_or(0, |mesh| mesh.heap_bytes())
    }

    /// True when the held mesh may be dropped for the budget: one is held and received.
    fn evictable(&self) -> bool {
        self.stage == Stage::Taken && self.mesh.is_some()
    }
}

/// See the module documentation.
#[derive(Debug)]
pub struct MeshCache {
    entries: HashMap<SectionKey, Entry>,
    /// Wanted sections to mesh, in the order they changed.
    dirty: VecDeque<SectionKey>,
    /// Finished meshes not yet handed out, in the order they finished.
    fresh: Vec<SectionKey>,
    /// Meshes handed out and not yet received.
    handed: Vec<SectionKey>,
    /// `(used, key)` of every evictable entry, least recently used first.
    lru: BTreeSet<(u64, SectionKey)>,
    budget: usize,
    held_bytes: usize,
    fresh_bytes: usize,
    /// Bytes of held meshes handed out and not yet received.
    handed_bytes: usize,
    /// Bytes of meshes handed out and not yet received that the cache no longer holds.
    orphaned_bytes: usize,
    in_flight: usize,
    /// The last version handed out; versions never repeat, so a job of a section released
    /// and wanted again never passes for a current one.
    version: u64,
    tick: u64,
    stats: MeshCacheStats,
}

impl MeshCache {
    /// An empty cache keeping at most about `budget` bytes of meshes.
    #[must_use]
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            dirty: VecDeque::new(),
            fresh: Vec::new(),
            handed: Vec::new(),
            lru: BTreeSet::new(),
            budget,
            held_bytes: 0,
            fresh_bytes: 0,
            handed_bytes: 0,
            orphaned_bytes: 0,
            in_flight: 0,
            version: 0,
            tick: 0,
            stats: MeshCacheStats::default(),
        }
    }

    /// Wants `pos` meshed; a section without a current mesh or result (new, or its mesh
    /// was dropped for the budget) is queued.
    pub fn want(&mut self, pos: SectionPos) {
        let key = pos.key();
        let next_version = self.version + 1;
        let entry = self.entries.entry(key).or_insert_with(|| Entry {
            version: next_version,
            ..Entry::default()
        });
        self.version = self.version.max(entry.version);
        self.touch(key);
        let entry = self.entries.get_mut(&key).expect("inserted above");
        if entry.meshed.is_none() && entry.in_flight.is_none() && !entry.queued {
            entry.queued = true;
            self.dirty.push_back(key);
        }
    }

    /// Stops wanting `key` and drops its mesh; a job in flight for it completes as
    /// [`Completion::Unwanted`].
    pub fn release(&mut self, key: SectionKey) {
        if let Some(entry) = self.entries.remove(&key) {
            self.drop_mesh(key, &entry);
        }
    }

    /// Records that the section at `pos` was written or arrived: it and every wanted
    /// section whose mesh reads it are meshed again.
    pub fn changed(&mut self, pos: SectionPos) {
        for affected in std::iter::once(pos).chain(dependents(pos)) {
            self.outdate(affected.key());
        }
    }

    /// Meshes again every wanted section that is, or whose mesh reads, a position
    /// `touched` names, as when what some regions read as changed without a write.
    pub fn changed_where(&mut self, mut touched: impl FnMut(SectionPos) -> bool) {
        let keys: Vec<SectionKey> = self
            .entries
            .keys()
            .filter(|key| {
                let pos = key.pos();
                touched(pos) || reads(pos).any(&mut touched)
            })
            .copied()
            .collect();
        for key in keys {
            self.outdate(key);
        }
    }

    /// Meshes every wanted section again, as when what the sections read as changed
    /// wholesale.
    pub fn changed_all(&mut self) {
        let keys: Vec<SectionKey> = self.entries.keys().copied().collect();
        for key in keys {
            self.outdate(key);
        }
    }

    /// Moves `key`'s version on and queues it, when it is wanted.
    fn outdate(&mut self, key: SectionKey) {
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        self.version += 1;
        entry.version = self.version;
        if !entry.queued && entry.in_flight.is_none() {
            entry.queued = true;
            self.dirty.push_back(key);
        }
    }

    /// The next section to mesh, marked in flight; `None` when nothing is out of date or
    /// fresh and handed meshes fill the budget.
    pub fn next_job(&mut self) -> Option<MeshJob> {
        if self.fresh_bytes + self.handed_bytes + self.orphaned_bytes >= self.budget {
            return None;
        }
        while let Some(key) = self.dirty.pop_front() {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            entry.queued = false;
            if entry.meshed == Some(entry.version) || entry.in_flight.is_some() {
                continue;
            }
            entry.in_flight = Some(entry.version);
            self.in_flight += 1;
            self.stats.jobs += 1;
            return Some(MeshJob {
                pos: key.pos(),
                version: entry.version,
            });
        }
        None
    }

    /// Ends `job` with its result: the mesh, or `None` when no section is stored at its
    /// position. Every job started must complete exactly once.
    pub fn complete(&mut self, job: MeshJob, mesh: Option<SectionMesh>) -> Completion {
        let key = job.pos.key();
        self.in_flight -= 1;
        let Some(entry) = self
            .entries
            .get_mut(&key)
            .filter(|entry| entry.in_flight == Some(job.version))
        else {
            // Released since, perhaps wanted again with a job of its own.
            self.stats.unwanted += 1;
            return Completion::Unwanted;
        };
        entry.in_flight = None;
        if job.version != entry.version {
            self.stats.stale += 1;
            if !entry.queued {
                entry.queued = true;
                self.dirty.push_back(key);
            }
            return Completion::Stale;
        }
        let old_bytes = entry.bytes();
        let old_stage = entry.stage;
        if entry.evictable() {
            self.lru.remove(&(entry.used, key));
        }
        entry.mesh = mesh.map(Arc::new);
        entry.meshed = Some(job.version);
        entry.stage = Stage::Fresh;
        let new_bytes = entry.bytes();
        self.held_bytes = self.held_bytes - old_bytes + new_bytes;
        match old_stage {
            Stage::Fresh => self.fresh_bytes -= old_bytes,
            Stage::Handed => {
                // The receiver still holds the replaced mesh.
                self.handed_bytes -= old_bytes;
                self.orphaned_bytes += old_bytes;
                self.fresh.push(key);
            }
            Stage::Taken => self.fresh.push(key),
        }
        self.fresh_bytes += new_bytes;
        self.stats.stored += 1;
        self.evict_taken();
        Completion::Stored
    }

    /// Ends `job` without a result because its inputs could not be read yet: the section
    /// goes back to the front of the queue, keeping its version.
    pub fn defer(&mut self, job: MeshJob) {
        self.in_flight -= 1;
        let key = job.pos.key();
        let Some(entry) = self
            .entries
            .get_mut(&key)
            .filter(|entry| entry.in_flight == Some(job.version))
        else {
            return;
        };
        entry.in_flight = None;
        if !entry.queued {
            entry.queued = true;
            self.dirty.push_front(key);
        }
    }

    /// Hands every fresh mesh out into `out`, in the order they finished. They stay
    /// cached, and count against the budget until [`Self::received`] says the receiver
    /// took them.
    pub fn take_fresh(&mut self, out: &mut Vec<FreshMesh>) {
        for key in std::mem::take(&mut self.fresh) {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if entry.stage != Stage::Fresh {
                continue;
            }
            entry.stage = Stage::Handed;
            let bytes = entry.bytes();
            self.fresh_bytes -= bytes;
            self.handed_bytes += bytes;
            self.handed.push(key);
            out.push((key, entry.mesh.clone()));
        }
    }

    /// Records that the receiver took every mesh handed out so far: they no longer count
    /// against the budget beyond the cache's own copies, which may now be dropped.
    pub fn received(&mut self) {
        for key in std::mem::take(&mut self.handed) {
            let Some(entry) = self.entries.get_mut(&key) else {
                continue;
            };
            if entry.stage != Stage::Handed {
                continue;
            }
            entry.stage = Stage::Taken;
            self.handed_bytes -= entry.bytes();
            if entry.evictable() {
                self.lru.insert((entry.used, key));
            }
        }
        self.orphaned_bytes = 0;
        self.evict_taken();
    }

    /// The held mesh of `key`, fresh, handed out or taken, marking it used.
    pub fn mesh(&mut self, key: SectionKey) -> Option<Arc<SectionMesh>> {
        self.touch(key);
        self.entries.get(&key)?.mesh.clone()
    }

    /// True when nothing waits for a job and no job is in flight.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.dirty.is_empty() && self.in_flight == 0
    }

    /// The counters and current sizes.
    #[must_use]
    pub fn stats(&self) -> MeshCacheStats {
        MeshCacheStats {
            wanted: self.entries.len(),
            dirty: self.dirty.len(),
            in_flight: self.in_flight,
            held_bytes: self.held_bytes,
            fresh_bytes: self.fresh_bytes,
            handed_bytes: self.handed_bytes + self.orphaned_bytes,
            ..self.stats
        }
    }

    /// Marks `key`'s entry, if wanted, used now.
    fn touch(&mut self, key: SectionKey) {
        self.tick += 1;
        let Some(entry) = self.entries.get_mut(&key) else {
            return;
        };
        if entry.evictable() {
            self.lru.remove(&(entry.used, key));
            self.lru.insert((self.tick, key));
        }
        entry.used = self.tick;
    }

    /// Forgets the mesh of `key`'s removed `entry`; a handed-out one still counts until
    /// received.
    fn drop_mesh(&mut self, key: SectionKey, entry: &Entry) {
        let bytes = entry.bytes();
        self.held_bytes -= bytes;
        match entry.stage {
            Stage::Fresh => self.fresh_bytes -= bytes,
            Stage::Handed => {
                self.handed_bytes -= bytes;
                self.orphaned_bytes += bytes;
            }
            Stage::Taken => {
                self.lru.remove(&(entry.used, key));
            }
        }
    }

    /// Drops received meshes, least recently used first, until the held meshes and the
    /// handed-out ones the cache no longer holds fit the budget. Their sections stay
    /// wanted, and are meshed again when wanted again or changed.
    fn evict_taken(&mut self) {
        while self.held_bytes + self.orphaned_bytes > self.budget {
            let Some((_, key)) = self.lru.pop_first() else {
                break;
            };
            let entry = self
                .entries
                .get_mut(&key)
                .expect("listed entries are wanted");
            let bytes = entry.bytes();
            entry.mesh = None;
            entry.meshed = None;
            self.held_bytes -= bytes;
            self.stats.evicted += 1;
        }
    }
}
