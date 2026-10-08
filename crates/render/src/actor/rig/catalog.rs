//! Immutable geometry pages with stable addresses and metadata-only relocation at capacity.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use super::{
    ActorRigGeometry, ActorRigGeometryError, ActorRigGeometrySpan, ActorRigVertex, EntityRigId,
};
use render_model::MAX_ACTOR_CATALOG_VERTICES;

#[path = "catalog/pages.rs"]
mod pages;
use pages::VertexPages;

/// Distinct for independently constructed catalogs.
static NEXT_EPOCH: AtomicU64 = AtomicU64::new(1);

/// Immutable geometry pages and their storage-buffer addresses.
#[derive(Clone, Debug, PartialEq)]
pub struct ActorRigVertexSegments {
    pub epoch: u64,
    pub segments: Arc<[Arc<[ActorRigVertex]>]>,
    pub(crate) offsets: Arc<[usize]>,
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
    #[cfg(test)]
    #[must_use]
    pub(crate) fn from_vertices(vertices: impl Into<Arc<[ActorRigVertex]>>) -> Self {
        let vertices = vertices.into();
        Self {
            epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
            len: vertices.len(),
            segments: Arc::from([vertices]),
            offsets: Arc::from([0]),
        }
    }

    /// Appends a page without changing any existing address.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_segment(&self, segment: Arc<[ActorRigVertex]>) -> Self {
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

#[derive(Debug)]
pub(super) struct GeometryCatalog {
    pub(super) geometries: BTreeMap<EntityRigId, ActorRigGeometry>,
    /// Stable span index for each registered geometry.
    pub(super) indices: BTreeMap<EntityRigId, u32>,
    spans: Vec<ActorRigGeometrySpan>,
    pub(super) published_spans: Arc<[ActorRigGeometrySpan]>,
    pub(super) vertices: ActorRigVertexSegments,
    page_fingerprints: Vec<u64>,
    pub(super) revision: u64,
    pub(super) maximum_vertices: usize,
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
        for geometry in geometries.values_mut() {
            geometry.revalidate()?;
        }
        let mut indices = BTreeMap::new();
        let mut pages = VertexPages::default();
        let mut offsets = Vec::with_capacity(geometries.len());
        let mut spans = Vec::with_capacity(geometries.len());
        let mut len = 0;
        for (id, geometry) in &mut geometries {
            let page = pages.intern(&geometry.vertices);
            if page == offsets.len() {
                if geometry.vertices.len() > maximum_vertices.saturating_sub(len) {
                    return Err(ActorRigGeometryError::CatalogCapacity);
                }
                offsets.push(len);
                len += geometry.vertices.len();
            }
            geometry.vertices = Arc::clone(&pages.pages[page]);
            indices.insert(*id, spans.len() as u32);
            spans.push(ActorRigGeometrySpan {
                first_vertex: offsets[page] as u32,
                vertex_count: geometry.vertices.len() as u32,
            });
        }
        let revision = content_revision(&pages.pages, &spans);
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
        })
    }

    pub(super) fn can_admit(&self, geometries: &BTreeMap<EntityRigId, ActorRigGeometry>) -> bool {
        let mut pages = VertexPages::retained(&self.vertices.segments, &self.page_fingerprints);
        let mut used = BTreeSet::new();
        let mut len = 0;
        for geometry in geometries.values() {
            let page = pages.intern(&geometry.vertices);
            if used.insert(page) {
                if geometry.vertices.len() > self.maximum_vertices.saturating_sub(len) {
                    return false;
                }
                len += geometry.vertices.len();
            }
        }
        true
    }

    /// Reuses vacant addresses; capacity fragmentation changes only page addresses.
    /// Admission is transactional and uses live vertices, as with the former full repack.
    pub(super) fn append(
        &mut self,
        mut added: Vec<ActorRigGeometry>,
        revision: u64,
    ) -> Result<(), ActorRigGeometryError> {
        for geometry in &mut added {
            geometry.revalidate()?;
        }
        let mut geometries = self.geometries.clone();
        geometries.extend(added.into_iter().map(|geometry| (geometry.id, geometry)));
        let mut pages = VertexPages::retained(&self.vertices.segments, &self.page_fingerprints);
        let retained_count = pages.pages.len();
        let mut spans = self.spans.clone();
        let mut indices = self.indices.clone();
        let mut route_pages = vec![0; spans.len()];
        let mut used = BTreeSet::new();
        let mut live_len = 0;
        for (id, geometry) in &mut geometries {
            let page = pages.intern(&geometry.vertices);
            geometry.vertices = Arc::clone(&pages.pages[page]);
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
                    route_pages.push(page);
                    index
                }
            };
            route_pages[index] = page;
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
        let mut relocate = false;
        for &page in order.iter().filter(|&&page| page >= retained_count) {
            let count = pages.pages[page].len();
            if let Some(offset) = vacant_range(&occupied, count, self.maximum_vertices) {
                occupied.insert(offset, count);
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
        for (span, &page) in spans.iter_mut().zip(&route_pages) {
            *span = ActorRigGeometrySpan {
                first_vertex: page_offsets[page] as u32,
                vertex_count: pages.pages[page].len() as u32,
            };
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
        Ok(())
    }
}

/// Finds the first contiguous unused address range within the existing vertex ceiling.
fn vacant_range(
    occupied: &BTreeMap<usize, usize>,
    count: usize,
    maximum_vertices: usize,
) -> Option<usize> {
    let mut start = 0;
    for (&offset, &len) in occupied {
        if offset >= start + count {
            return Some(start);
        }
        start = start.max(offset + len);
    }
    (start + count <= maximum_vertices).then_some(start)
}

/// Hashes initial page contents in the same order as the former contiguous layout.
fn content_revision(segments: &[Arc<[ActorRigVertex]>], spans: &[ActorRigGeometrySpan]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in segments
        .iter()
        .flat_map(|segment| bytemuck::cast_slice::<ActorRigVertex, u8>(segment))
        .chain(bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(spans))
    {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash.max(1)
}

#[cfg(test)]
#[path = "catalog/tests.rs"]
mod tests;
