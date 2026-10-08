//! Checked, texture-addressed geometry for one ordered JSON-UI model control.

use std::{fmt, mem::size_of, ops::Range, sync::Arc};

use crate::UiLimits;

use super::UiBlendMode;

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
    /// Producer-authored, interpolated linear lighting multiplier; ordinary UI uses one.
    pub model_light: f32,
    /// Native entity overlay RGB and its mix amount, before model lighting.
    pub overlay_color: [f32; 4],
    pub style_flags: u8,
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

#[derive(Clone, Debug, PartialEq)]
pub struct UiMesh {
    vertices: Arc<[UiMeshVertex]>,
    indices: Arc<[u32]>,
    batches: Arc<[UiMeshBatch]>,
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
                vertex.color[3] = (f32::from(vertex.color[3]) * opacity).round() as u8;
            }
        }
        self
    }
    pub fn new(
        vertices: Arc<[UiMeshVertex]>,
        indices: Arc<[u32]>,
        batches: Arc<[UiMeshBatch]>,
    ) -> Result<Self, UiMeshError> {
        if vertices.len() > UiLimits::MAX_UI_VERTICES {
            return Err(UiMeshError::VertexLimit);
        }
        if indices.len() > UiLimits::MAX_UI_INDICES {
            return Err(UiMeshError::IndexLimit);
        }
        // Draw emission gives each triangle corner its batch material value.
        if indices.len() > UiLimits::MAX_UI_VERTICES {
            return Err(UiMeshError::VertexLimit);
        }
        if batches.len() > UiLimits::MAX_DRAW_BATCHES {
            return Err(UiMeshError::BatchLimit);
        }
        let bytes = vertices
            .len()
            .checked_mul(size_of::<UiMeshVertex>())
            .and_then(|bytes| {
                indices
                    .len()
                    .checked_mul(size_of::<u32>())
                    .and_then(|indices| bytes.checked_add(indices))
            })
            .and_then(|bytes| {
                batches
                    .len()
                    .checked_mul(size_of::<UiMeshBatch>())
                    .and_then(|batches| bytes.checked_add(batches))
            })
            .ok_or(UiMeshError::ByteLimit)?;
        if bytes > UiLimits::MAX_DRAW_LIST_BYTES {
            return Err(UiMeshError::ByteLimit);
        }
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
        Ok(Self {
            vertices,
            indices,
            batches,
        })
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
