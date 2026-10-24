//! Immutable geometry pages with stable addresses and metadata-only relocation at capacity.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use foldhash::fast::FixedState;
use hashbrown::HashMap;

use super::{
    ActorRigGeometry, ActorRigGeometryError, ActorRigGeometrySpan, ActorRigVertex, EntityRigId,
};
use crate::MAX_ACTOR_CATALOG_VERTICES;

#[path = "catalog/pages.rs"]
mod pages;
use pages::VertexPages;

#[path = "catalog/free_ranges.rs"]
mod free_ranges;
use free_ranges::FreeRanges;

/// Makes geometries with equal vertex bytes share one allocation, so a later catalog layout
/// matches them by pointer instead of comparing their bytes.
pub fn share_identical_vertices<'a>(
    geometries: impl IntoIterator<Item = &'a mut ActorRigGeometry>,
) {
    let mut pages = VertexPages::default();
    for geometry in geometries {
        pages.intern(geometry);
    }
}

/// Distinct for independently constructed catalogs.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Immutable geometry pages and their storage-buffer addresses.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorRigVertexSegments {
    pub epoch: u64,
    pub segments: Arc<[Arc<[ActorRigVertex]>]>,
    pub offsets: Arc<[usize]>,
    len: usize,
}

impl Default for ActorRigVertexSegments {
    fn default() -> Self {
        Self {
            epoch: 0,
            segments: Arc::from([]),
            offsets: Arc::from([]),
            len: 0,
        }
    }
}

impl ActorRigVertexSegments {
    /// Starts an independent catalog containing one page.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn from_vertices(vertices: impl Into<Arc<[ActorRigVertex]>>) -> Self {
        let vertices = vertices.into();
        Self {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            len: vertices.len(),
            segments: Arc::from([vertices]),
            offsets: Arc::from([0]),
        }
    }

    /// Appends a page without changing any existing address.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn with_segment(&self, segment: Arc<[ActorRigVertex]>) -> Self {
        Self {
            epoch: self.epoch,
            len: self.len + segment.len(),
            segments: self.segments.iter().cloned().chain([segment]).collect(),
            offsets: self.offsets.iter().copied().chain([self.len]).collect(),
        }
    }

    /// Length of the occupied address range, including reusable gaps.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether there are no addressable vertices.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The vertices of a span within one immutable page.
    #[must_use]
    pub fn span(&self, span: ActorRigGeometrySpan) -> Option<&[ActorRigVertex]> {
        let start = span.first_vertex as usize;
        self.segments
            .iter()
            .zip(self.offsets.iter())
            .find_map(|(segment, &offset)| {
                start
                    .checked_sub(offset)
                    .and_then(|start| segment.get(start..start + span.vertex_count as usize))
            })
    }
}

/// Outcome of [`GeometryCatalog::plan_admission`].
#[derive(Debug, Default)]
pub(super) struct AdmissionPlan {
    /// Candidate indices that fit, in order.
    pub(super) admitted: Vec<usize>,
    /// Length of the `evictable` prefix to evict.
    pub(super) evicted: usize,
    /// Some candidate cannot fit even after every eviction.
    pub(super) denied: bool,
    /// Some candidate waits for a later frame's budget.
    pub(super) deferred: bool,
}

#[derive(Debug)]
pub(super) struct GeometryCatalog {
    pub(super) geometries: BTreeMap<EntityRigId, ActorRigGeometry>,
    /// Stable span index for each registered geometry; looked up for every drawn submission.
    pub(super) indices: HashMap<EntityRigId, u32, FixedState>,
    spans: Vec<ActorRigGeometrySpan>,
    pub(super) published_spans: Arc<[ActorRigGeometrySpan]>,
    pub(super) vertices: ActorRigVertexSegments,
    page_fingerprints: Vec<u64>,
    pub(super) revision: u64,
    pub(super) maximum_vertices: usize,
    /// Vertices in pages some geometry uses; vacated gaps are excluded.
    pub(super) live_vertices: usize,
}

impl GeometryCatalog {
    /// Places immutable geometry pages consecutively without copying their vertices.
    pub(super) fn layout(
        geometries: BTreeMap<EntityRigId, ActorRigGeometry>,
    ) -> Result<Self, ActorRigGeometryError> {
        Self::layout_with_limit(geometries, MAX_ACTOR_CATALOG_VERTICES)
    }

    pub(super) fn layout_with_limit(
        mut geometries: BTreeMap<EntityRigId, ActorRigGeometry>,
        maximum_vertices: usize,
    ) -> Result<Self, ActorRigGeometryError> {
        let _span =
            tracing::info_span!("actor.catalog_layout", geometries = geometries.len()).entered();
        {
            let _validation = tracing::info_span!("actor.catalog_validate").entered();
            for geometry in geometries.values_mut() {
                geometry.revalidate()?;
            }
        }
        let mut indices =
            HashMap::with_capacity_and_hasher(geometries.len(), FixedState::default());
        let mut pages = VertexPages::default();
        let mut offsets = Vec::with_capacity(geometries.len());
        let mut spans = Vec::with_capacity(geometries.len());
        let mut len = 0;
        for (id, geometry) in &mut geometries {
            let page = pages.intern(geometry);
            if page == offsets.len() {
                if geometry.vertices.len() > maximum_vertices.saturating_sub(len) {
                    return Err(ActorRigGeometryError::CatalogCapacity);
                }
                offsets.push(len);
                len += geometry.vertices.len();
            }
            indices.insert(*id, spans.len() as u32);
            spans.push(ActorRigGeometrySpan {
                first_vertex: offsets[page] as u32,
                vertex_count: geometry.vertices.len() as u32,
            });
        }
        let revision = content_revision(&pages.fingerprints, &spans);
        Ok(Self {
            geometries,
            indices,
            published_spans: Arc::from(spans.as_slice()),
            spans,
            vertices: ActorRigVertexSegments {
                epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
                segments: pages.pages.into(),
                offsets: offsets.into(),
                len,
            },
            page_fingerprints: pages.fingerprints,
            revision,
            maximum_vertices,
            live_vertices: len,
        })
    }

    /// Plans admitting `candidates` in order, evicting a prefix of `evictable` (least valuable
    /// first) as needed. Charges only storage a rig adds: a page already resident or claimed by
    /// an earlier candidate costs nothing, and evicting frees a page once no rig uses it. Past
    /// the first admission, a rig adding more than `budget` remaining vertices is deferred.
    pub(super) fn plan_admission(
        &self,
        candidates: &[ActorRigGeometry],
        evictable: &[EntityRigId],
        budget: usize,
    ) -> AdmissionPlan {
        let mut pages = VertexPages::retained(&self.vertices.segments, &self.page_fingerprints);
        let mut users = vec![0_usize; pages.pages.len()];
        let mut rig_pages = BTreeMap::new();
        for (id, geometry) in &self.geometries {
            let page = pages.intern(&mut geometry.clone());
            users[page] += 1;
            rig_pages.insert(*id, page);
        }
        let mut free = self.maximum_vertices.saturating_sub(self.live_vertices);
        let mut remaining = budget;
        let mut plan = AdmissionPlan::default();
        for (index, geometry) in candidates.iter().enumerate() {
            let page = pages.intern(&mut geometry.clone());
            if page >= users.len() {
                users.resize(page + 1, 0);
            }
            let cost = if users[page] == 0 {
                pages.pages[page].len()
            } else {
                0
            };
            if !plan.admitted.is_empty() && cost > remaining {
                plan.deferred = true;
                continue;
            }
            users[page] += 1;
            let (mut available, mut through, mut released) = (free, plan.evicted, Vec::new());
            while available < cost && through < evictable.len() {
                if let Some(&evicted) = rig_pages.get(&evictable[through]) {
                    users[evicted] -= 1;
                    released.push(evicted);
                    if users[evicted] == 0 {
                        available += pages.pages[evicted].len();
                    }
                }
                through += 1;
            }
            if available < cost {
                for evicted in released {
                    users[evicted] += 1;
                }
                users[page] -= 1;
                plan.denied = true;
                continue;
            }
            free = available - cost;
            plan.evicted = through;
            remaining = remaining.saturating_sub(cost);
            plan.admitted.push(index);
        }
        plan
    }

    /// Reuses vacant addresses; capacity fragmentation changes only page addresses.
    /// Admission is transactional and uses live vertices, as with the former full repack.
    pub(super) fn append(
        &mut self,
        added: Vec<ActorRigGeometry>,
        revision: u64,
    ) -> Result<(), ActorRigGeometryError> {
        self.append_evicting(added, |_| false, revision)
    }

    /// [`Self::append`] after dropping the `evict` geometries, whose pages become vacant and whose
    /// span indices stay reserved with empty spans.
    pub(super) fn append_evicting(
        &mut self,
        mut added: Vec<ActorRigGeometry>,
        evict: impl Fn(&EntityRigId) -> bool,
        revision: u64,
    ) -> Result<(), ActorRigGeometryError> {
        let _span = tracing::info_span!("actor.catalog_append", added = added.len()).entered();
        {
            let _validation = tracing::info_span!("actor.catalog_validate").entered();
            for geometry in &mut added {
                geometry.revalidate()?;
            }
        }
        let mut geometries = self.geometries.clone();
        geometries.retain(|id, _| !evict(id));
        geometries.extend(added.into_iter().map(|geometry| (geometry.id, geometry)));
        let mut pages = VertexPages::retained(&self.vertices.segments, &self.page_fingerprints);
        let retained_count = pages.pages.len();
        let mut spans = self.spans.clone();
        let mut indices = self.indices.clone();
        let mut route_pages = vec![None; spans.len()];
        let mut used = BTreeSet::new();
        let mut live_len = 0;
        for (id, geometry) in &mut geometries {
            let page = pages.intern(geometry);
            if used.insert(page) {
                if geometry.vertices.len() > self.maximum_vertices.saturating_sub(live_len) {
                    return Err(ActorRigGeometryError::CatalogCapacity);
                }
                live_len += geometry.vertices.len();
            }
            let index = match indices.get(id) {
                Some(&index) => index as usize,
                None => {
                    let index = spans.len();
                    indices.insert(*id, index as u32);
                    spans.push(ActorRigGeometrySpan::default());
                    route_pages.push(None);
                    index
                }
            };
            route_pages[index] = Some(page);
        }
        let mut fresh: VecDeque<_> = used.range(retained_count..).copied().collect();
        let mut order = Vec::with_capacity(used.len());
        for page in 0..retained_count {
            if used.contains(&page) {
                order.push(page);
            } else if let Some(page) = fresh.pop_front() {
                order.push(page);
            }
        }
        order.extend(fresh);
        let mut page_offsets = vec![0; pages.pages.len()];
        let mut occupied = BTreeMap::new();
        for &page in used.range(..retained_count) {
            let offset = self.vertices.offsets[page];
            page_offsets[page] = offset;
            occupied.insert(offset, pages.pages[page].len());
        }
        let mut free = FreeRanges::new(&occupied, self.maximum_vertices);
        let mut relocate = false;
        for &page in order.iter().filter(|&&page| page >= retained_count) {
            let count = pages.pages[page].len();
            if let Some(offset) = free.allocate(count) {
                page_offsets[page] = offset;
            } else {
                relocate = true;
            }
        }
        if relocate {
            let mut offset = 0;
            for &page in &order {
                page_offsets[page] = offset;
                offset += pages.pages[page].len();
            }
        }
        for (span, page) in spans.iter_mut().zip(&route_pages) {
            *span = page.map_or_else(ActorRigGeometrySpan::default, |page| ActorRigGeometrySpan {
                first_vertex: page_offsets[page] as u32,
                vertex_count: pages.pages[page].len() as u32,
            });
        }
        let offsets = order.iter().map(|&page| page_offsets[page]).collect();
        let segments = order
            .iter()
            .map(|&page| Arc::clone(&pages.pages[page]))
            .collect();
        let len = order
            .iter()
            .map(|&page| page_offsets[page] + pages.pages[page].len())
            .max()
            .unwrap_or(0);
        self.geometries = geometries;
        self.indices = indices;
        self.page_fingerprints = order.iter().map(|&page| pages.fingerprints[page]).collect();
        self.vertices = ActorRigVertexSegments {
            epoch: self.vertices.epoch,
            segments,
            offsets,
            len,
        };
        self.published_spans = Arc::from(spans.as_slice());
        self.spans = spans;
        self.revision = revision;
        self.live_vertices = live_len;
        Ok(())
    }
}

/// Combines immutable page fingerprints and routes without rereading prepared vertex bytes.
fn content_revision(fingerprints: &[u64], spans: &[ActorRigGeometrySpan]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in fingerprints
        .iter()
        .flat_map(|fingerprint| fingerprint.to_le_bytes())
        .chain(
            bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(spans)
                .iter()
                .copied(),
        )
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash.max(1)
}

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;
