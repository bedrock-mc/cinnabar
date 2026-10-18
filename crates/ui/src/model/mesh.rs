//! Checked, texture-addressed geometry for one ordered JSON-UI model control.

use std::{fmt, mem::size_of, ops::Range, sync::Arc};

use crate::UiLimits;

use super::UiBlendMode;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UiMeshVertex {
    /// Normalized node XY multiplied by W; the node's bounds supply the viewport mapping.
    pub position: [f32; 2],
    /// Homogeneous reverse-Z depth and W. Orthographic producers use W=1.
    pub clip_z: f32,
    pub clip_w: f32,
    /// Texture-page texel coordinates; model side faces retain fractional pixel centers.
    pub uv: [f32; 2],
    /// Producer-authored sRGB tint, independent of the model's native lighting.
    pub color: [u8; 4],
    /// Interpolated linear lighting; coverage paint uses this field for the border area.
    pub model_light: f32,
    /// Entity overlay RGB and mix amount, or the coverage paint's packed constant colors.
    pub overlay_color: [f32; 4],
    pub style_flags: u16,
    pub alpha_test: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiMeshBatch {
    pub texture_page: u16,
    pub index_range: Range<u32>,
    pub blend: UiBlendMode,
    pub depth_test: bool,
    pub depth_write: bool,
    /// Sampled-texture cutoff, independent of faded vertex alpha and glyph alpha testing.
    pub alpha_cutoff: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct UiMesh {
    vertices: Arc<[UiMeshVertex]>,
    indices: Arc<[u32]>,
    batches: Arc<[UiMeshBatch]>,
    draw_vertex_count: usize,
}

/// Meshes compare by contents. [`UiMesh::new`] admits only finite floats, so buffers shared
/// between two meshes are equal without reading them, as retained frames compare every drawn
/// mesh with the last frame's.
impl PartialEq for UiMesh {
    fn eq(&self, other: &Self) -> bool {
        fn same<T: PartialEq>(left: &Arc<[T]>, right: &Arc<[T]>) -> bool {
            Arc::ptr_eq(left, right) || **left == **right
        }
        same(&self.vertices, &other.vertices)
            && same(&self.indices, &other.indices)
            && same(&self.batches, &other.batches)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiMeshError {
    VertexLimit,
    IndexLimit,
    BatchLimit,
    ByteLimit,
    NonFiniteVertex,
    InvalidHomogeneousW,
    IndexOutOfBounds,
    InvalidTriangleCount,
    InvalidBatchRange,
    InvalidAlphaCutoff,
    InvalidModelLight,
}

impl fmt::Display for UiMeshError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "UI model geometry rejected: {self:?}")
    }
}

impl std::error::Error for UiMeshError {}

impl UiMesh {
    /// Fades vertex tint while retaining topology and texture bindings.
    pub fn with_opacity(mut self, opacity: f32) -> Self {
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        if opacity < 1.0 {
            for vertex in Arc::make_mut(&mut self.vertices) {
                if vertex.style_flags & render_api::UI_STYLE_COVERAGE_PAINT != 0 {
                    (vertex.color, vertex.overlay_color) =
                        render_api::UiCoveragePaint::from_vertex_colors(
                            vertex.color,
                            vertex.overlay_color,
                        )
                        .with_opacity(opacity)
                        .vertex_colors();
                } else {
                    vertex.color[3] = (f32::from(vertex.color[3]) * opacity).round() as u8;
                }
            }
        }
        self
    }
    /// Checks retained and emitted geometry budgets before allocating or appending a mesh.
    pub fn check_size(vertices: usize, indices: usize, batches: usize) -> Result<(), UiMeshError> {
        if vertices > UiLimits::MAX_UI_VERTICES {
            return Err(UiMeshError::VertexLimit);
        }
        if indices > UiLimits::MAX_UI_INDICES {
            return Err(UiMeshError::IndexLimit);
        }
        // Keep source triangle references within the existing per-mesh admission cap.
        if indices > UiLimits::MAX_UI_VERTICES {
            return Err(UiMeshError::VertexLimit);
        }
        if batches > UiLimits::MAX_DRAW_BATCHES {
            return Err(UiMeshError::BatchLimit);
        }
        let bytes = vertices
            .checked_mul(size_of::<UiMeshVertex>())
            .and_then(|bytes| {
                indices
                    .checked_mul(size_of::<u32>())
                    .and_then(|indices| bytes.checked_add(indices))
            })
            .and_then(|bytes| {
                batches
                    .checked_mul(size_of::<UiMeshBatch>())
                    .and_then(|batches| bytes.checked_add(batches))
            })
            .ok_or(UiMeshError::ByteLimit)?;
        if bytes > UiLimits::MAX_DRAW_LIST_BYTES {
            return Err(UiMeshError::ByteLimit);
        }
        Ok(())
    }

    pub fn new(
        vertices: Arc<[UiMeshVertex]>,
        indices: Arc<[u32]>,
        batches: Arc<[UiMeshBatch]>,
    ) -> Result<Self, UiMeshError> {
        Self::check_size(vertices.len(), indices.len(), batches.len())?;
        if vertices.iter().any(|vertex| {
            !vertex.position.iter().all(|v| v.is_finite())
                || !vertex.clip_z.is_finite()
                || !vertex.clip_w.is_finite()
                || !vertex.uv.iter().all(|value| value.is_finite())
                || !vertex.overlay_color.iter().all(|value| value.is_finite())
        }) {
            return Err(UiMeshError::NonFiniteVertex);
        }
        if vertices.iter().any(|vertex| vertex.clip_w <= 0.0) {
            return Err(UiMeshError::InvalidHomogeneousW);
        }
        if vertices
            .iter()
            .any(|vertex| !vertex.model_light.is_finite() || vertex.model_light < 0.0)
        {
            return Err(UiMeshError::InvalidModelLight);
        }
        if indices
            .iter()
            .any(|index| *index as usize >= vertices.len())
        {
            return Err(UiMeshError::IndexOutOfBounds);
        }
        if !indices.len().is_multiple_of(3) {
            return Err(UiMeshError::InvalidTriangleCount);
        }
        let mut expected = 0;
        for batch in &*batches {
            if batch.index_range.start != expected
                || batch.index_range.end <= expected
                || batch.index_range.end as usize > indices.len()
                || !(batch.index_range.end - expected).is_multiple_of(3)
            {
                return Err(UiMeshError::InvalidBatchRange);
            }
            if batch
                .alpha_cutoff
                .is_some_and(|cutoff| !cutoff.is_finite() || !(0.0..=1.0).contains(&cutoff))
            {
                return Err(UiMeshError::InvalidAlphaCutoff);
            }
            expected = batch.index_range.end;
        }
        if expected as usize != indices.len() {
            return Err(UiMeshError::InvalidBatchRange);
        }
        let mut seen = vec![0; vertices.len()];
        let mut draw_vertex_count = 0;
        for (index, batch) in batches.iter().enumerate() {
            let generation = index + 1;
            for &source in
                &indices[batch.index_range.start as usize..batch.index_range.end as usize]
            {
                if seen[source as usize] != generation {
                    seen[source as usize] = generation;
                    draw_vertex_count += 1;
                }
            }
        }
        Ok(Self {
            vertices,
            indices,
            batches,
            draw_vertex_count,
        })
    }

    /// Reserves each referenced vertex once per material, keeping material cutoffs independent.
    pub(super) fn draw_vertex_count(&self) -> usize {
        self.draw_vertex_count
    }

    pub fn vertices(&self) -> &[UiMeshVertex] {
        &self.vertices
    }
    pub fn indices(&self) -> &[u32] {
        &self.indices
    }
    pub fn batches(&self) -> &[UiMeshBatch] {
        &self.batches
    }
}
