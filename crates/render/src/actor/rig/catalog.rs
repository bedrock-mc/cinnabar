//! Immutable geometry pages with stable addresses and metadata-only relocation at capacity.

use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use super::{
    ActorRigGeometry, ActorRigGeometryError, ActorRigGeometrySpan, ActorRigVertex, EntityRigId,
    MAX_ACTOR_RIG_VERTICES,
};

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
    pub(super) revision: u64,
}

impl GeometryCatalog {
    /// Places immutable geometry pages consecutively without copying their vertices.
    pub(super) fn layout(
        mut geometries: BTreeMap<EntityRigId, ActorRigGeometry>,
    ) -> Result<Self, ActorRigGeometryError> {
        for geometry in geometries.values_mut() {
            geometry.revalidate()?;
        }
        let mut indices = BTreeMap::new();
        let mut segments = Vec::with_capacity(geometries.len());
        let mut offsets = Vec::with_capacity(geometries.len());
        let mut spans = Vec::with_capacity(geometries.len());
        let mut len = 0;
        for (id, geometry) in &geometries {
            if len + geometry.vertices.len() > MAX_ACTOR_RIG_VERTICES {
                return Err(ActorRigGeometryError::CatalogCapacity);
            }
            indices.insert(*id, spans.len() as u32);
            offsets.push(len);
            segments.push(Arc::clone(&geometry.vertices));
            spans.push(ActorRigGeometrySpan {
                first_vertex: len as u32,
                vertex_count: geometry.vertices.len() as u32,
            });
            len += geometry.vertices.len();
        }
        let revision = content_revision(&segments, &spans);
        Ok(Self {
            geometries,
            indices,
            published_spans: Arc::from(spans.as_slice()),
            spans,
            vertices: ActorRigVertexSegments {
                epoch: NEXT_EPOCH.fetch_add(1, Ordering::Relaxed),
                segments: segments.into(),
                offsets: offsets.into(),
                len,
            },
            revision,
        })
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
        if geometries
            .values()
            .map(|geometry| geometry.vertices.len())
            .sum::<usize>()
            > MAX_ACTOR_RIG_VERTICES
        {
            return Err(ActorRigGeometryError::CatalogCapacity);
        }
        let mut segments = self.vertices.segments.to_vec();
        let mut spans = self.spans.clone();
        let mut occupied = BTreeMap::new();
        for (id, &index) in &self.indices {
            let span = spans[index as usize];
            if Arc::ptr_eq(&self.geometries[id].vertices, &geometries[id].vertices) {
                occupied.insert(span.first_vertex as usize, span.vertex_count as usize);
            }
        }
        let mut relocate = false;
        for (id, geometry) in &geometries {
            let index = match self.indices.get(id) {
                Some(&index) => index as usize,
                None => {
                    let index = spans.len();
                    self.indices.insert(*id, index as u32);
                    spans.push(ActorRigGeometrySpan::default());
                    segments.push(Arc::clone(&geometry.vertices));
                    index
                }
            };
            if self
                .geometries
                .get(id)
                .is_some_and(|old| Arc::ptr_eq(&old.vertices, &geometry.vertices))
            {
                continue;
            }
            let count = geometry.vertices.len();
            let offset = vacant_range(&occupied, count);
            relocate |= offset.is_none();
            let offset = offset.unwrap_or(0);
            occupied.insert(offset, count);
            segments[index] = Arc::clone(&geometry.vertices);
            spans[index] = ActorRigGeometrySpan {
                first_vertex: offset as u32,
                vertex_count: count as u32,
            };
        }
        if relocate {
            let mut offset = 0;
            for span in &mut spans {
                span.first_vertex = offset;
                offset += span.vertex_count;
            }
        }
        let offsets: Arc<[usize]> = spans
            .iter()
            .map(|span| span.first_vertex as usize)
            .collect();
        let len = spans
            .iter()
            .map(|span| span.first_vertex as usize + span.vertex_count as usize)
            .max()
            .unwrap_or(0);
        self.geometries = geometries;
        self.vertices = ActorRigVertexSegments {
            epoch: self.vertices.epoch,
            segments: segments.into(),
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
fn vacant_range(occupied: &BTreeMap<usize, usize>, count: usize) -> Option<usize> {
    let mut start = 0;
    for (&offset, &len) in occupied {
        if offset >= start + count {
            return Some(start);
        }
        start = start.max(offset + len);
    }
    (start + count <= MAX_ACTOR_RIG_VERTICES).then_some(start)
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
