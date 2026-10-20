//! Where each resident section's expanded faces live: vertex pages within the memory budget,
//! the expansion queue, and the origin that positions are relative to.

use std::collections::{HashMap, HashSet, VecDeque};
use std::ops::Range;
use std::time::{Duration, Instant};

use bevy::log::debug;
use bevy::math::{DVec3, IVec3};
use bevy::prelude::Entity;

use super::allocator::FaceAllocator;
use super::jobs::{FACING_GROUPS, SectionPlan};

/// Default memory budget of the expanded vertex pages, the most any device gets.
pub const DEFAULT_GEOMETRY_BUDGET_BYTES: u64 = 1 << 30;
/// Smallest device-sized budget.
const MIN_GEOMETRY_BUDGET_BYTES: u64 = 256 << 20;
/// Budget when the device does not report dedicated memory (shared-memory adapters).
const SHARED_MEMORY_GEOMETRY_BUDGET_BYTES: u64 = 512 << 20;

/// The vertex budget for an adapter with `dedicated` bytes of graphics memory (0 when it
/// reports none): an eighth of it, within 256 MiB and [`DEFAULT_GEOMETRY_BUDGET_BYTES`].
pub fn device_geometry_budget(dedicated: u64) -> u64 {
    if dedicated == 0 {
        return SHARED_MEMORY_GEOMETRY_BUDGET_BYTES;
    }
    (dedicated / 8).clamp(MIN_GEOMETRY_BUDGET_BYTES, DEFAULT_GEOMETRY_BUDGET_BYTES)
}

/// Bytes of one expanded face of `stride`-byte vertices: four vertices.
pub(crate) const fn face_bytes(stride: u64) -> u64 {
    4 * stride
}
/// Size of a vertex page. Pages are created at this size (or what the budget has left) and
/// never grow, so filling the budget allocates whole pages once and copies nothing; the
/// device allocates a page memory of its own, and 64 MiB is small enough that the last,
/// partly filled page wastes little and that running out of memory costs at most one page
/// of terrain.
pub(crate) const PAGE_BYTES: u64 = 64 << 20;
/// Grid the geometry origin snaps to, in blocks.
pub(crate) const ORIGIN_GRID: i32 = 1024;
/// Camera distance from the origin, in blocks, past which the origin moves and every section
/// re-expands. f32 positions within this range keep sub-millimetre precision.
pub(crate) const ORIGIN_REBASE_DISTANCE: f64 = 4096.0;
/// Distance from a section's centre to its farthest corner, in blocks.
pub(crate) const SECTION_RADIUS: f64 = 8.0 * 1.732_050_807_568_877_2;
/// Frames in a row without a section arriving, changing or leaving before a page may start
/// draining ([`Residency::observe_frame`]).
pub(crate) const DRAIN_SETTLED_FRAMES: u32 = 120;

/// Squared distance from `eye` to the centre of the section at block `origin`.
pub(crate) fn section_distance(origin: [i32; 3], eye: DVec3) -> f64 {
    let centre = IVec3::from_array(origin).as_dvec3() + DVec3::splat(8.0);
    centre.distance_squared(eye)
}

/// A resident section's faces: their page, the range they occupy and each category's range.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ResidentFaces {
    pub(crate) translucent: Vec<crate::chunk::transparent::mixed::plan::MixedFace>,
    pub(crate) page: usize,
    pub(crate) faces: Range<u32>,
    pub(crate) categories: [Range<u32>; 3],
    /// Each category's faces by facing group (`jobs::FACING_GROUPS`).
    pub(crate) facing: [[Range<u32>; FACING_GROUPS]; 3],
    /// Section origin in blocks.
    pub(crate) origin: [i32; 3],
    /// Origin epoch the positions were written against; stale faces are not drawn.
    pub(crate) epoch: u64,
    /// Expansion input generation used to bake atlas coordinates and material attributes.
    pub(crate) generation: u64,
    /// Unique to this placement: a draw list built from it stays valid while the section
    /// keeps this stamp.
    pub(crate) stamp: u64,
    /// The section's entry in the per-draw origin table, which its draws select as their
    /// first instance.
    pub(crate) slot: u32,
}

/// Face space of one vertex page. Pages never exceed one storage-buffer binding, since the
/// kernels bind a page whole.
#[derive(Debug, Default)]
pub(crate) struct PageSpace {
    pub(crate) faces: FaceAllocator,
}

/// Where a planned section was placed, and whether that placement opened a new page.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Placement {
    pub(crate) page: usize,
    pub(crate) faces: Range<u32>,
    /// The placement opened page `page`, which the GPU side must create.
    pub(crate) new_page: bool,
}

/// Resident sections by distance from the camera, nearest first, built on the first eviction
/// of a frame and consumed from the far end.
#[derive(Debug, Default)]
pub(crate) struct Eviction {
    residents: Option<Vec<(f64, Entity)>>,
}

/// CPU bookkeeping of the expanded arena.
#[derive(Debug)]
pub(crate) struct Residency {
    pub(crate) pages: Vec<PageSpace>,
    pub(crate) sections: HashMap<Entity, ResidentFaces>,
    queue: VecDeque<Entity>,
    queued: HashSet<Entity>,
    /// Sections that did not fit the budget and that nothing farther could make room for;
    /// they are tried again once faces are freed or the camera moves.
    waiting: HashSet<Entity>,
    /// Requests that cannot fit any page even after evicting all its faces.
    unplaceable: HashSet<Entity>,
    /// Faces one page may hold.
    page_faces: u32,
    /// Faces all pages together may hold.
    budget_faces: u64,
    /// Bytes one face takes in a page.
    face_bytes: u64,
    pub(crate) origin: Option<IVec3>,
    pub(crate) epoch: u64,
    pub(crate) generation: u64,
    /// Stamp the next committed placement takes.
    next_stamp: u64,
    /// Draw-origin slots given back by sections that left, reused first.
    free_slots: Vec<u32>,
    /// Slots handed out so far: the origin table must hold this many entries.
    slot_count: u32,
    /// The page being emptied so its memory can go: placements land elsewhere while
    /// anything else fits, and its sections are queued to move.
    draining: Option<usize>,
    /// Frames in a row in which no section arrived, changed or left.
    settled_frames: u32,
    /// Counts changes to `sections` or the epoch: draw lists built at one revision stay
    /// valid until it moves.
    revision: u64,
    /// A drain returned to its source page; only a changed free layout may retry it.
    failed_drain: Option<usize>,
}

impl Residency {
    /// `binding_bytes` is the largest buffer a kernel may bind; `budget_bytes` the memory budget.
    /// Pages hold [`PAGE_BYTES`] (less when the binding limit is smaller) of expanded faces of
    /// `stride`-byte vertices.
    pub(crate) fn new(binding_bytes: u64, budget_bytes: u64, stride: u64) -> Self {
        Self::with_faces(
            binding_bytes.min(PAGE_BYTES),
            budget_bytes,
            face_bytes(stride),
        )
    }

    /// Pages of `page_bytes` holding faces of `face_bytes` each.
    fn with_faces(page_bytes: u64, budget_bytes: u64, face_bytes: u64) -> Self {
        let page_faces = (page_bytes / face_bytes).min(u64::from(u32::MAX / 4)) as u32;
        Self {
            pages: Vec::new(),
            sections: HashMap::new(),
            queue: VecDeque::new(),
            queued: HashSet::new(),
            waiting: HashSet::new(),
            unplaceable: HashSet::new(),
            page_faces,
            budget_faces: budget_bytes / face_bytes,
            face_bytes,
            origin: None,
            epoch: 0,
            generation: 0,
            next_stamp: 0,
            free_slots: Vec::new(),
            slot_count: 0,
            draining: None,
            settled_frames: 0,
            revision: 0,
            failed_drain: None,
        }
    }

    /// Draw-origin slots handed out so far, free or not.
    pub(crate) fn slot_count(&self) -> u32 {
        self.slot_count
    }

    /// Moves whenever `sections` or `epoch` change.
    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    fn release(&mut self, resident: &ResidentFaces) {
        self.failed_drain = None;
        self.pages[resident.page]
            .faces
            .release(resident.faces.clone());
        self.free_slots.push(resident.slot);
    }

    /// Bytes one face takes in a page.
    pub(crate) fn face_bytes(&self) -> u64 {
        self.face_bytes
    }

    /// Changes the memory budget and retries waiting requests; pages already open stay.
    pub(crate) fn set_budget(&mut self, budget_bytes: u64) {
        let budget_faces = budget_bytes / self.face_bytes;
        if self.budget_faces != budget_faces {
            self.budget_faces = budget_faces;
            self.retry_waiting();
        }
    }

    /// Whether any live section needs the native scene fallback until its request changes.
    pub(crate) fn has_unplaceable(&self) -> bool {
        !self.unplaceable.is_empty()
    }

    /// Undoes an uncommitted `placement` of `entity` whose new page could not be allocated:
    /// the page is dropped and the section waits without faces.
    pub(crate) fn revoke(&mut self, entity: Entity, placement: &Placement) {
        debug_assert!(
            placement.new_page && placement.page + 1 == self.pages.len(),
            "only a new page, the last one, can fail to allocate"
        );
        self.pages.pop();
        self.wait(entity);
    }

    pub(crate) fn capacity_faces(&self) -> u64 {
        self.pages
            .iter()
            .map(|page| u64::from(page.faces.capacity()))
            .sum()
    }

    /// Sections waiting for room in the budget.
    pub(crate) fn waiting_len(&self) -> usize {
        self.waiting.len()
    }

    /// Queues `entity` for (re-)expansion once.
    pub(crate) fn enqueue(&mut self, entity: Entity) {
        self.waiting.remove(&entity);
        if self.queued.insert(entity) {
            self.queue.push_back(entity);
        }
    }

    /// Withdraws faces baked against previous inputs and queues every live section to rebuild.
    pub(crate) fn invalidate_contents(&mut self, entities: impl IntoIterator<Item = Entity>) {
        self.generation += 1;
        self.enqueue_all(entities);
    }

    /// Queues every resident section, as after an origin, table or atlas change.
    pub(crate) fn enqueue_all(&mut self, entities: impl IntoIterator<Item = Entity>) {
        for entity in entities {
            self.enqueue(entity);
        }
    }

    /// Sections queued or waiting for room.
    pub(crate) fn pending_len(&self) -> usize {
        self.queue.len() + self.waiting.len()
    }

    /// Every section queued or waiting for room.
    pub(crate) fn pending(&self) -> impl Iterator<Item = Entity> + '_ {
        self.queue.iter().chain(&self.waiting).copied()
    }

    /// Empties the queue, in arrival order.
    pub(crate) fn take_queue(&mut self) -> Vec<Entity> {
        self.queued.clear();
        self.queue.drain(..).collect()
    }

    /// Parks `entity` until faces are freed or [`Self::retry_waiting`].
    pub(crate) fn wait(&mut self, entity: Entity) {
        if !self.queued.contains(&entity) {
            self.waiting.insert(entity);
        }
    }

    /// Queues every waiting section again.
    pub(crate) fn retry_waiting(&mut self) {
        let waiting: Vec<_> = self.waiting.drain().collect();
        for entity in waiting {
            self.enqueue(entity);
        }
    }

    /// Frees a removed section's faces.
    pub(crate) fn remove(&mut self, entity: Entity) {
        if let Some(resident) = self.sections.remove(&entity) {
            self.release(&resident);
            self.revision += 1;
        }
        self.waiting.remove(&entity);
        self.unplaceable.remove(&entity);
        if self.queued.remove(&entity) {
            self.queue.retain(|queued| *queued != entity);
        }
    }

    /// Frees a resident section's faces to make room for a nearer one; it waits for room.
    pub(crate) fn evict(&mut self, entity: Entity) {
        if let Some(resident) = self.sections.remove(&entity) {
            self.release(&resident);
            self.wait(entity);
            self.revision += 1;
        }
    }

    /// Moves the origin when `camera` has left its range; returns whether it moved, in which
    /// case every resident section needs expanding again, unless its positions are packed
    /// ([`Self::keep_faces_across_rebase`]).
    pub(crate) fn follow_camera(&mut self, camera: DVec3) -> bool {
        let snapped = (camera / f64::from(ORIGIN_GRID)).round() * f64::from(ORIGIN_GRID);
        let snapped = IVec3::new(snapped.x as i32, snapped.y as i32, snapped.z as i32);
        match self.origin {
            Some(origin) if camera.distance(origin.as_dvec3()) <= ORIGIN_REBASE_DISTANCE => false,
            _ => {
                self.origin = Some(snapped);
                self.epoch += 1;
                self.revision += 1;
                true
            }
        }
    }

    /// Marks every resident section's faces as written against the current origin: packed
    /// positions are section-relative, so a rebase only moves their draw origins.
    pub(crate) fn keep_faces_across_rebase(&mut self) {
        let epoch = self.epoch;
        for faces in self.sections.values_mut() {
            faces.epoch = epoch;
        }
        self.revision += 1;
    }

    /// Places `plan` for `entity`, replacing its previous faces, growing a page or adding one
    /// within the budget. `None` when it does not fit; the section then keeps its old faces.
    pub(crate) fn place(&mut self, entity: Entity, plan: &SectionPlan) -> Option<Placement> {
        if plan.faces > self.page_faces {
            return None;
        }
        let previous = self.sections.remove(&entity);
        let draining_move = previous.as_ref().is_some_and(|resident| {
            (Some(resident.page) == self.draining || Some(resident.page) == self.failed_drain)
                && resident.faces.end - resident.faces.start == plan.faces
        });
        if let Some(previous) = &previous {
            self.pages[previous.page]
                .faces
                .release(previous.faces.clone());
            self.revision += 1;
        }
        let placement = self.allocate(plan.faces);
        let changed_space = placement.as_ref().is_some_and(|placed| {
            previous.as_ref().is_none_or(|resident| {
                resident.page != placed.page || resident.faces != placed.faces
            })
        });
        let shrank = previous
            .as_ref()
            .is_some_and(|old| placement.is_some() && old.faces.end - old.faces.start > plan.faces);
        match previous {
            Some(previous) if placement.is_none() => {
                // Keep drawing the old faces until a later frame finds room.
                let reclaimed = self.pages[previous.page]
                    .faces
                    .allocate_at(previous.faces.clone());
                debug_assert!(reclaimed, "released faces are free");
                self.sections.insert(entity, previous);
            }
            Some(previous) => self.free_slots.push(previous.slot),
            None => {}
        }
        if changed_space && !draining_move {
            self.failed_drain = None;
        }
        if shrank {
            self.retry_waiting();
        }
        placement
    }

    /// [`Self::place`], making room over budget by evicting resident sections farther from
    /// the camera than this one, farthest first. `near` is the camera and this section's
    /// squared distance from it; without it nothing is evicted. Evicted sections wait for
    /// room. `eviction` carries the resident order across one frame's placements.
    pub(crate) fn place_nearest(
        &mut self,
        entity: Entity,
        plan: &SectionPlan,
        near: Option<(DVec3, f64)>,
        eviction: &mut Eviction,
    ) -> Option<Placement> {
        if !self.can_fit(plan.faces) {
            self.unplaceable.insert(entity);
            return None;
        }
        self.unplaceable.remove(&entity);
        loop {
            if let Some(placement) = self.place(entity, plan) {
                return Some(placement);
            }
            let (eye, distance) = near?;
            let residents = eviction.residents.get_or_insert_with(|| {
                let mut residents: Vec<_> = self
                    .sections
                    .iter()
                    .map(|(entity, faces)| (section_distance(faces.origin, eye), *entity))
                    .collect();
                residents.sort_by(|a, b| a.0.total_cmp(&b.0));
                residents
            });
            let victim = loop {
                let &(farthest, candidate) = residents.last()?;
                if farthest <= distance {
                    return None;
                }
                residents.pop();
                if candidate != entity && self.sections.contains_key(&candidate) {
                    break candidate;
                }
            };
            self.evict(victim);
        }
    }

    /// Whether a request can fit any page without changing the binding limit or budget.
    pub(crate) fn can_fit(&self, faces: u32) -> bool {
        faces != 0
            && faces <= self.page_faces
            && (self.pages.iter().any(|page| page.faces.capacity() >= faces)
                || u64::from(faces) <= self.budget_faces.saturating_sub(self.capacity_faces()))
    }

    fn allocate(&mut self, faces: u32) -> Option<Placement> {
        let draining = self.draining;
        for (page, space) in self.pages.iter_mut().enumerate() {
            if Some(page) == draining {
                continue;
            }
            if let Some(range) = space.faces.allocate(faces) {
                return Some(Placement {
                    page,
                    faces: range,
                    new_page: false,
                });
            }
        }
        if let Some(page) = draining
            && let Some(range) = self.pages[page].faces.allocate(faces)
        {
            // Nothing else has room: the page stays until its free layout changes.
            self.draining = None;
            self.failed_drain = Some(page);
            return Some(Placement {
                page,
                faces: range,
                new_page: false,
            });
        }
        let budget_left = self.budget_faces.saturating_sub(self.capacity_faces());
        let capacity = u64::from(self.page_faces).min(budget_left) as u32;
        if capacity < faces || faces == 0 {
            return None;
        }
        let mut space = PageSpace {
            faces: FaceAllocator::new(capacity),
        };
        let range = space.faces.allocate(faces)?;
        self.pages.push(space);
        Some(Placement {
            page: self.pages.len() - 1,
            faces: range,
            new_page: true,
        })
    }

    /// Whether `entity` is resident on the draining page, queued to move off it.
    pub(crate) fn moving(&self, entity: Entity) -> bool {
        self.draining.is_some_and(|page| {
            self.sections
                .get(&entity)
                .is_some_and(|resident| resident.page == page)
        })
    }

    /// Sections queued or waiting for room, leaving out those moving off the draining page,
    /// which still draw.
    pub(crate) fn undrawn_len(&self) -> usize {
        self.pending()
            .filter(|entity| !self.moving(*entity))
            .count()
    }

    /// Counts a frame in which sections arrived, changed or left (`streamed`), or none did.
    /// Draining waits for [`DRAIN_SETTLED_FRAMES`] settled frames: while the camera streams
    /// terrain in and out, removals keep fragmenting the pages and new sections fill the
    /// holes, so moving a page's faces would re-expand them only to be undone by the next
    /// removals.
    pub(crate) fn observe_frame(&mut self, streamed: bool) {
        self.settled_frames = if streamed {
            0
        } else {
            self.settled_frames.saturating_add(1)
        };
    }

    /// Starts emptying the page with the fewest faces once streaming has settled, nothing
    /// waits and the other pages' free faces hold its faces with a quarter to spare, queueing
    /// its sections to move. Returns whether it started.
    pub(crate) fn start_draining(&mut self) -> bool {
        if self.draining.is_some()
            || self.failed_drain.is_some()
            || self.settled_frames < DRAIN_SETTLED_FRAMES
            || self.pending_len() > 0
            || self.pages.len() < 2
        {
            return false;
        }
        let free: u64 = self
            .pages
            .iter()
            .map(|page| u64::from(page.faces.capacity() - page.faces.used()))
            .sum();
        let Some((page, space)) = self
            .pages
            .iter()
            .enumerate()
            .min_by_key(|(_, page)| page.faces.used())
        else {
            return false;
        };
        let used = u64::from(space.faces.used());
        let free_elsewhere = free - u64::from(space.faces.capacity() - space.faces.used());
        if free_elsewhere < used + used / 4 {
            return false;
        }
        self.draining = Some(page);
        let moving: Vec<_> = self
            .sections
            .iter()
            .filter(|(_, resident)| resident.page == page)
            .map(|(entity, _)| *entity)
            .collect();
        self.enqueue_all(moving);
        true
    }

    /// Drops the draining page once its last face has moved, renumbering the pages after
    /// it. Returns the dropped page, whose buffer the caller drops too.
    pub(crate) fn finish_draining(&mut self) -> Option<usize> {
        let page = self.draining?;
        if self.pages[page].faces.used() > 0 {
            return None;
        }
        self.draining = None;
        self.pages.remove(page);
        for resident in self.sections.values_mut() {
            if resident.page > page {
                resident.page -= 1;
            }
        }
        self.revision += 1;
        Some(page)
    }

    /// Records `entity`'s placed plan, returning its draw-origin slot.
    pub(crate) fn commit(
        &mut self,
        entity: Entity,
        placement: &Placement,
        plan: &SectionPlan,
        origin: [i32; 3],
    ) -> u32 {
        let slot = self.free_slots.pop().unwrap_or_else(|| {
            self.slot_count += 1;
            self.slot_count - 1
        });
        self.sections.insert(
            entity,
            ResidentFaces {
                translucent: Vec::new(),
                page: placement.page,
                faces: placement.faces.clone(),
                categories: plan.categories.clone(),
                facing: plan.facing.clone(),
                origin,
                epoch: self.epoch,
                generation: self.generation,
                stamp: self.next_stamp,
                slot,
            },
        );
        self.next_stamp += 1;
        self.revision += 1;
        slot
    }

    /// Drops every page and section, as when the mode turns off.
    pub(crate) fn clear(&mut self) {
        self.pages.clear();
        self.sections.clear();
        self.queue.clear();
        self.queued.clear();
        self.waiting.clear();
        self.unplaceable.clear();
        self.origin = None;
        self.free_slots.clear();
        self.slot_count = 0;
        self.draining = None;
        self.revision += 1;
        self.failed_drain = None;
    }
}

/// Once-a-second residency report, and the moment the expansion queue drains, as evidence of
/// how long terrain takes to fill in.
#[derive(Default)]
pub(super) struct ResidencyLog {
    window_start: Option<Instant>,
    expanded_faces: u64,
    frames: u32,
    filling_since: Option<Instant>,
}

impl ResidencyLog {
    pub(super) fn observe(
        &mut self,
        residency: &Residency,
        expanded: u32,
        complete_radius: Option<f32>,
    ) {
        let now = Instant::now();
        self.expanded_faces += u64::from(expanded);
        self.frames += 1;
        let pending = residency.undrawn_len();
        match (pending, self.filling_since) {
            (0, Some(since)) => {
                self.filling_since = None;
                debug!(
                    target: "cinnabar::shaderpack",
                    seconds = now.duration_since(since).as_secs_f32(),
                    sections = residency.sections.len(),
                    "shader-pack terrain complete"
                );
            }
            (1.., None) => self.filling_since = Some(now),
            _ => {}
        }
        let start = *self.window_start.get_or_insert(now);
        if now.duration_since(start) < Duration::from_secs(1) {
            return;
        }
        let resident_faces: u64 = residency
            .sections
            .values()
            .map(|section| u64::from(section.faces.end - section.faces.start))
            .sum();
        debug!(
            target: "cinnabar::shaderpack",
            sections = residency.sections.len(),
            resident_mib = (resident_faces * residency.face_bytes()) >> 20,
            allocated_mib = (residency.capacity_faces() * residency.face_bytes()) >> 20,
            pending,
            waiting = residency.waiting_len(),
            complete_radius,
            expanded_faces = self.expanded_faces,
            frames = self.frames,
            "shader-pack terrain residency"
        );
        *self = Self {
            filling_since: self.filling_since,
            ..Self::default()
        };
    }
}

#[cfg(test)]
#[path = "residency_tests.rs"]
mod tests;
