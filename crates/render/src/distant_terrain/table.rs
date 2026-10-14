//! The CPU side of the renderer's buffers: which sections are drawn, where their quads,
//! biome grids, cluster records and section records live, and how a [`DrawUpdate`] reaches
//! them.
//!
//! An update is uploaded section by section over as many frames as the upload budget needs,
//! into space no drawn section uses and under section slots still dead, so nothing changes
//! on screen meanwhile. Once every new mesh is uploaded the update commits in one frame:
//! removed and replaced sections die (their section records and cluster records are
//! zeroed), new sections go live and relinked sections take their new neighbour levels.
//! A commit too large for one frame's commands rewrites the whole section record table in
//! one copy instead, so every update commits within a frame of its last upload.
//!
//! A section that finds the pool, the cluster slots or the section slots full is left out
//! of its update, which still commits, and is uploaded again in a later frame once space
//! was freed, unless a later update removes or replaces it first. Until then the section it
//! replaces stays drawn, or nothing is drawn there when the update removed that one.
//!
//! A dead section's cluster records are zeroed over as many frames as the commands need,
//! with its slot still held so no stale record names a live section. What is zeroed becomes
//! allocatable only from the next frame, after that frame's culling stopped listing it, so
//! a draw list never names reused space.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    ops::Range,
};

use lod::{
    draw::records::{ClusterRecord, SECTION_RECORD_WORDS, SectionRecord},
    geometry::{Face, SectionKey, SectionPos},
    mesh::{Bucket, MeshList, output::SectionMesh, translucent::TranslucentClass},
    select::shown::{DrawUpdate, DrawnSection, ShownSection},
};

use super::{
    layout::{BIOME_UNITS, Capacity, MAX_SECTIONS, POOL_UNIT_BYTES, Target},
    ranges::RangeAllocator,
    staging::Staging,
};

/// Words of one cluster record.
const CLUSTER_WORDS: u32 = lod::draw::records::CLUSTER_RECORD_WORDS as u32;

/// A drawn section's translucent clusters, which the far-to-near order lists.
#[derive(Debug, Clone)]
pub(crate) struct TranslucentClusters {
    pub(crate) pos: SectionPos,
    pub(crate) class: TranslucentClass,
    /// Cluster slots with their direction and list, in mesh order.
    pub(crate) clusters: Box<[(Face, MeshList, u32)]>,
    /// The centre of each direction's clusters in section-local voxel corners.
    pub(crate) centres: [[f32; 3]; 6],
}

/// Where one drawn section's data lives.
struct Resident {
    slot: u32,
    pool: offset_allocator::Allocation,
    units: u32,
    clusters: Range<u32>,
    record: SectionRecord,
    translucent: Option<Box<TranslucentClusters>>,
}

/// Space a commit freed.
struct Freed {
    slot: u32,
    pool: offset_allocator::Allocation,
    units: u32,
    clusters: Range<u32>,
}

/// An update being uploaded.
struct Pending {
    removed: Vec<SectionKey>,
    upserts: VecDeque<DrawnSection>,
    uploaded: Vec<(SectionKey, Resident)>,
    relinked: Vec<ShownSection>,
    /// Upserts that found a table full; retried after the commit.
    full: Vec<DrawnSection>,
    /// Handed over by the frame, which waits for its commit; false for a retry.
    handed: bool,
}

impl Pending {
    /// Uploads of sections left out of earlier commits.
    fn retry(sections: Vec<DrawnSection>) -> Self {
        Self {
            removed: Vec::new(),
            upserts: sections.into(),
            uploaded: Vec::new(),
            relinked: Vec::new(),
            full: Vec::new(),
            handed: false,
        }
    }
}

/// What one frame's step did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Step {
    /// Bytes of meshes uploaded.
    pub(crate) uploaded_bytes: u64,
    /// The update the frame handed over committed.
    pub(crate) committed: bool,
}

/// See the module documentation.
pub(crate) struct SectionTable {
    capacity: Capacity,
    drawn: HashMap<SectionKey, Resident>,
    slots: RangeAllocator,
    pool: offset_allocator::Allocator,
    pool_used_units: u64,
    clusters: RangeAllocator,
    /// Dead sections whose cluster records are not zeroed yet, oldest first.
    zeroing: VecDeque<Freed>,
    /// Zeroed in this frame's staging; allocatable from the next frame.
    deferred: Vec<Freed>,
    pending: Option<Pending>,
    /// Upserts that found the pool, the cluster slots or the section slots full, counted
    /// once each.
    dropped: u64,
    /// Sections left out of a commit for want of space, uploaded again once space is freed.
    retry: Vec<DrawnSection>,
    /// Space was freed since the last retry.
    freed_since_retry: bool,
    /// The set of translucent sections changed since the order was last built.
    translucent_changed: bool,
}

impl SectionTable {
    /// An empty table for buffers of `capacity`.
    pub(crate) fn new(capacity: Capacity) -> Self {
        Self {
            capacity,
            drawn: HashMap::new(),
            slots: RangeAllocator::new(1, MAX_SECTIONS),
            pool: offset_allocator::Allocator::with_max_allocs(
                capacity.pool_units,
                2 * MAX_SECTIONS,
            ),
            pool_used_units: 0,
            clusters: RangeAllocator::new(0, capacity.clusters),
            zeroing: VecDeque::new(),
            deferred: Vec::new(),
            pending: None,
            dropped: 0,
            retry: Vec::new(),
            freed_since_retry: false,
            translucent_changed: false,
        }
    }

    /// Starts a frame: what the last frame zeroed becomes allocatable.
    pub(crate) fn begin_frame(&mut self) {
        self.freed_since_retry |= !self.deferred.is_empty();
        for freed in self.deferred.drain(..) {
            self.slots.free(freed.slot..freed.slot + 1);
            self.pool.free(freed.pool);
            self.pool_used_units -= u64::from(freed.units);
            self.clusters.free(freed.clusters);
        }
    }

    /// True while an update is being uploaded.
    #[cfg(test)]
    pub(crate) fn is_applying(&self) -> bool {
        self.pending.is_some()
    }

    /// Starts uploading `update`; the previous one must have committed. Sections it
    /// removes or replaces are no longer retried, and a retry in progress is folded into
    /// it.
    pub(crate) fn start(&mut self, update: DrawUpdate) {
        let mut pending = Pending {
            removed: update.removed,
            upserts: update.upserts.into(),
            uploaded: Vec::new(),
            relinked: update.relinked,
            full: Vec::new(),
            handed: true,
        };
        let superseded: HashSet<SectionKey> = pending
            .removed
            .iter()
            .copied()
            .chain(pending.upserts.iter().map(|section| section.pos.key()))
            .collect();
        if let Some(retrying) = self.pending.take() {
            debug_assert!(!retrying.handed, "one update at a time");
            for (key, resident) in retrying.uploaded {
                if superseded.contains(&key) {
                    // Never live: its section record was not written.
                    self.kill(resident, None);
                } else {
                    pending.uploaded.push((key, resident));
                }
            }
            // What it had not tried yet is tried at the next retry without waiting for space.
            self.freed_since_retry |= !retrying.upserts.is_empty();
            self.retry.extend(retrying.upserts);
            self.retry.extend(retrying.full);
        }
        self.retry
            .retain(|section| !superseded.contains(&section.pos.key()));
        self.pending = Some(pending);
    }

    /// Uploads the pending update's meshes into `staging`, nearest first, until their bytes
    /// pass `budget` (at least one a frame), commits it once all are uploaded and the
    /// commit's writes fit, then zeroes dead sections' cluster records with the room left.
    pub(crate) fn step(&mut self, staging: &mut Staging, budget: u64) -> Step {
        let step = self.step_pending(staging, budget);
        self.zero_dead_clusters(staging);
        step
    }

    /// The upload and commit part of [`Self::step`].
    fn step_pending(&mut self, staging: &mut Staging, budget: u64) -> Step {
        let mut step = Step::default();
        if self.pending.is_none() && !self.retry.is_empty() && self.freed_since_retry {
            self.freed_since_retry = false;
            self.pending = Some(Pending::retry(std::mem::take(&mut self.retry)));
        }
        let Some(mut pending) = self.pending.take() else {
            return step;
        };
        while let Some(section) = pending.upserts.front() {
            let bytes = upload_bytes(&section.mesh);
            if step.uploaded_bytes != 0 && step.uploaded_bytes + bytes > budget {
                break;
            }
            match self.upload(section, staging) {
                Upload::Done(resident) => {
                    step.uploaded_bytes += bytes;
                    let key = section.pos.key();
                    pending.uploaded.push((key, resident));
                }
                Upload::Dropped => self.dropped += 1,
                Upload::Full => {
                    self.dropped += u64::from(pending.handed);
                    pending.full.push(section.clone());
                }
                Upload::NoRoom => break,
            }
            pending.upserts.pop_front();
        }
        if !pending.upserts.is_empty() {
            self.pending = Some(pending);
            return step;
        }
        let handed = pending.handed;
        let deaths = pending.removed.len() + pending.uploaded.len();
        let records = pending.uploaded.len() + pending.relinked.len();
        if staging.has_room(deaths + records, records * SECTION_RECORD_WORDS) {
            self.commit(pending, Some(staging));
        } else if staging.has_room(
            Staging::commands_for(self.table_image_words()),
            self.table_image_words(),
        ) {
            self.commit(pending, None);
            let image = self.table_image();
            let staged = staging.copy(Target::Sections, 0, &image);
            debug_assert!(staged, "the room was checked above");
        } else {
            // Uploads used the room; the next frame has it.
            self.pending = Some(pending);
            return step;
        }
        step.committed = handed;
        step
    }

    /// Words of the section record table up to the highest slot in use.
    fn table_image_words(&self) -> usize {
        self.slots.high_water() as usize * SECTION_RECORD_WORDS
    }

    /// The section record table up to the highest slot in use: live records, zeros
    /// elsewhere.
    fn table_image(&self) -> Vec<u32> {
        let mut image = vec![0; self.table_image_words()];
        for resident in self.drawn.values() {
            let at = resident.slot as usize * SECTION_RECORD_WORDS;
            image[at..at + SECTION_RECORD_WORDS].copy_from_slice(resident.record.words());
        }
        image
    }

    /// Kills removed and replaced sections, makes the uploaded ones live and relinks the
    /// rest, staging each record write in `staging` when given; see the module
    /// documentation.
    fn commit(&mut self, pending: Pending, mut staging: Option<&mut Staging>) {
        self.retry.extend(pending.full);
        if !self.retry.is_empty() && !pending.relinked.is_empty() {
            let relinked: HashMap<SectionKey, _> = pending
                .relinked
                .iter()
                .map(|relink| (relink.key, relink.neighbours))
                .collect();
            for section in &mut self.retry {
                if let Some(&neighbours) = relinked.get(&section.pos.key()) {
                    section.neighbours = neighbours;
                }
            }
        }
        for key in &pending.removed {
            if let Some(resident) = self.drawn.remove(key) {
                self.kill(resident, staging.as_deref_mut());
            }
        }
        for (key, resident) in pending.uploaded {
            if let Some(old) = self.drawn.remove(&key) {
                self.kill(old, staging.as_deref_mut());
            }
            write_record(&resident, staging.as_deref_mut());
            self.translucent_changed |= resident.translucent.is_some();
            self.drawn.insert(key, resident);
        }
        for relink in pending.relinked {
            let Some(mut resident) = self.drawn.remove(&relink.key) else {
                continue;
            };
            resident.record = resident.record.with_neighbours(relink.neighbours);
            write_record(&resident, staging.as_deref_mut());
            self.drawn.insert(relink.key, resident);
        }
    }

    /// Zeroes `resident`'s section record in `staging` when given and queues its cluster
    /// records for zeroing.
    fn kill(&mut self, resident: Resident, staging: Option<&mut Staging>) {
        if let Some(staging) = staging {
            let staged = staging.fill(
                Target::Sections,
                resident.slot * SECTION_RECORD_WORDS as u32,
                SECTION_RECORD_WORDS as u32,
                0,
            );
            debug_assert!(staged, "the commit checked the room");
        }
        self.translucent_changed |= resident.translucent.is_some();
        self.zeroing.push_back(Freed {
            slot: resident.slot,
            pool: resident.pool,
            units: resident.units,
            clusters: resident.clusters,
        });
    }

    /// Zeroes the cluster records of dead sections, oldest first, while `staging` has room;
    /// their space is freed from the next frame.
    fn zero_dead_clusters(&mut self, staging: &mut Staging) {
        while let Some(freed) = self.zeroing.front() {
            if !staging.fill(
                Target::Clusters,
                freed.clusters.start * CLUSTER_WORDS,
                freed.clusters.len() as u32 * CLUSTER_WORDS,
                0,
            ) {
                break;
            }
            self.deferred.extend(self.zeroing.pop_front());
        }
    }

    /// Allocates and stages one section's quads, biome grid and cluster records under a
    /// dead section slot.
    fn upload(&mut self, section: &DrawnSection, staging: &mut Staging) -> Upload {
        let mesh = &section.mesh;
        let (Some(biomes), false) = (mesh.biomes(), mesh.is_empty()) else {
            return Upload::Dropped;
        };
        let quads = mesh.quads().len() as u32;
        let clusters = mesh.clusters().len() as u32;
        let pool_words = (BIOME_UNITS + quads) as usize * 2;
        let cluster_words = (clusters * CLUSTER_WORDS) as usize;
        let commands = Staging::commands_for(pool_words)
            + Staging::commands_for(cluster_words)
            + Staging::commands_for(clusters as usize);
        if !staging.has_room(commands, pool_words + cluster_words) {
            return Upload::NoRoom;
        }
        let Some(units) = BIOME_UNITS.checked_add(quads) else {
            return Upload::Dropped;
        };
        let Some(slot) = self.slots.allocate(1) else {
            return Upload::Full;
        };
        let Some(cluster_range) = self.clusters.allocate(clusters) else {
            self.slots.free(slot);
            return Upload::Full;
        };
        let Some(pool) = self.pool.allocate(units) else {
            self.slots.free(slot);
            self.clusters.free(cluster_range);
            return Upload::Full;
        };
        let slot = slot.start;
        let biome_unit = pool.offset;
        let quad_base = biome_unit + BIOME_UNITS;
        let mut records = Vec::with_capacity(cluster_words);
        for cluster in mesh.clusters() {
            let record = ClusterRecord::new(cluster, slot, quad_base)
                .expect("slots and pool offsets fit the record");
            records.extend_from_slice(record.words());
        }
        let staged = staging.copy_with(Target::Pool, biome_unit * 2, pool_words, |payload| {
            payload.extend_from_slice(biomes);
            payload.extend(
                mesh.quads()
                    .iter()
                    .flat_map(|quad| [quad.geometry(), quad.payload()]),
            );
        }) && staging.copy(
            Target::Clusters,
            cluster_range.start * CLUSTER_WORDS,
            &records,
        ) && staging.fill(
            Target::Flags,
            self.capacity.history_word() + cluster_range.start,
            clusters,
            0,
        );
        debug_assert!(staged, "the room was checked above");
        self.pool_used_units += u64::from(units);
        Upload::Done(Resident {
            slot,
            pool,
            units,
            translucent: translucent_clusters(section.pos, mesh, cluster_range.start),
            clusters: cluster_range,
            record: SectionRecord::new(section.pos, section.neighbours, biome_unit),
        })
    }

    /// Drawn sections.
    pub(crate) fn sections(&self) -> u32 {
        self.drawn.len() as u32
    }

    /// One past the highest section slot in use.
    pub(crate) fn section_slots(&self) -> u32 {
        self.slots.high_water()
    }

    /// One past the highest cluster slot in use.
    pub(crate) fn cluster_slots(&self) -> u32 {
        self.clusters.high_water()
    }

    /// Cluster slots in use.
    pub(crate) fn clusters_used(&self) -> u32 {
        self.clusters.used()
    }

    /// Pool bytes in use, uploading sections included.
    pub(crate) fn pool_used_bytes(&self) -> u64 {
        self.pool_used_units * POOL_UNIT_BYTES
    }

    /// Sections that could not be drawn or did not fit when handed over.
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Sections waiting for space to be uploaded again.
    #[cfg(test)]
    pub(crate) fn retrying(&self) -> usize {
        self.retry.len()
    }

    /// The drawn sections' translucent clusters, in no particular order.
    pub(crate) fn translucent(&self) -> impl Iterator<Item = &TranslucentClusters> {
        self.drawn
            .values()
            .filter_map(|resident| resident.translucent.as_deref())
    }

    /// True once since the last call when the set of translucent sections changed.
    pub(crate) fn take_translucent_changed(&mut self) -> bool {
        std::mem::take(&mut self.translucent_changed)
    }

    /// The section record of `key`, while it is drawn; for tests.
    #[cfg(test)]
    pub(crate) fn record(&self, key: SectionKey) -> Option<(u32, SectionRecord, Range<u32>)> {
        self.drawn
            .get(&key)
            .map(|resident| (resident.slot, resident.record, resident.clusters.clone()))
    }
}

/// Stages `resident`'s section record in `staging` when given; the commit checked the room.
fn write_record(resident: &Resident, staging: Option<&mut Staging>) {
    if let Some(staging) = staging {
        let staged = staging.copy(
            Target::Sections,
            resident.slot * SECTION_RECORD_WORDS as u32,
            resident.record.words(),
        );
        debug_assert!(staged, "the commit checked the room");
    }
}

/// The outcome of one section's upload.
enum Upload {
    Done(Resident),
    /// It cannot be drawn: it has no quads or biome grid, or is too large to address.
    Dropped,
    /// A table was full; retry once space is freed.
    Full,
    /// The frame's staging is full; retry next frame.
    NoRoom,
}

/// Bytes a section's upload stages, which the upload budget counts.
pub(crate) fn upload_bytes(mesh: &SectionMesh) -> u64 {
    if mesh.is_empty() {
        return 0;
    }
    (u64::from(BIOME_UNITS) + mesh.quads().len() as u64) * POOL_UNIT_BYTES
        + mesh.clusters().len() as u64 * u64::from(CLUSTER_WORDS) * 4
}

/// The translucent clusters of `mesh`, whose clusters start at slot `first`.
fn translucent_clusters(
    pos: SectionPos,
    mesh: &SectionMesh,
    first: u32,
) -> Option<Box<TranslucentClusters>> {
    if mesh.translucent() == TranslucentClass::None {
        return None;
    }
    let mut bounds: [Option<([u8; 3], [u8; 3])>; 6] = [None; 6];
    let clusters: Box<[(Face, MeshList, u32)]> = mesh
        .clusters()
        .iter()
        .enumerate()
        .filter(|(_, cluster)| cluster.bucket == Bucket::Translucent)
        .map(|(index, cluster)| {
            let entry = &mut bounds[cluster.face.index()];
            let (min, max) = entry.get_or_insert((cluster.bounds.min, cluster.bounds.max));
            for axis in 0..3 {
                min[axis] = min[axis].min(cluster.bounds.min[axis]);
                max[axis] = max[axis].max(cluster.bounds.max[axis]);
            }
            (cluster.face, cluster.list, first + index as u32)
        })
        .collect();
    if clusters.is_empty() {
        return None;
    }
    let centres = bounds.map(|bounds| {
        bounds.map_or([0.0; 3], |(min, max)| {
            std::array::from_fn(|axis| (f32::from(min[axis]) + f32::from(max[axis])) * 0.5)
        })
    });
    Some(Box::new(TranslucentClusters {
        pos,
        class: mesh.translucent(),
        clusters,
        centres,
    }))
}

#[cfg(test)]
#[path = "table_tests.rs"]
mod tests;
