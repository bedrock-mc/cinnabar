use std::{
    fmt,
    mem::size_of,
    sync::{Arc, Mutex},
};

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};
use bytemuck::{Pod, Zeroable};

pub const MAX_UI_VERTICES: usize = 262_144;
pub const MAX_UI_INDICES: usize = 393_216;
pub const MAX_UI_BATCHES: usize = 8_192;
pub const MAX_UI_DRAW_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_UI_TEXTURE_SIDE: u32 = 4_096;
pub const MAX_UI_TEXTURE_LAYERS: u32 = 256;
pub const MAX_UI_TEXTURE_BYTES: usize = 64 * 1024 * 1024;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct UiRenderVertex {
    pub position: [f32; 2],
    pub uv: [u16; 2],
    pub color: [u8; 4],
    pub style_flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Pod, Zeroable)]
pub struct UiScissor {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl UiScissor {
    #[must_use]
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Wire value for the classic alpha-over blend.
pub const UI_BLEND_ALPHA: u32 = 0;
/// Wire value for the crosshair invert blend (src*(1-dst) + dst*(1-src)).
pub const UI_BLEND_INVERT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, Pod, Zeroable)]
pub struct UiRenderBatch {
    pub texture_page: u32,
    pub scissor: UiScissor,
    pub first_index: u32,
    pub index_count: u32,
    /// One of [`UI_BLEND_ALPHA`] or [`UI_BLEND_INVERT`]; any other value is
    /// rejected at publication.
    pub blend_mode: u32,
    _padding: u32,
}

impl UiRenderBatch {
    #[must_use]
    pub const fn new(
        texture_page: u32,
        scissor: UiScissor,
        first_index: u32,
        index_count: u32,
        blend_mode: u32,
    ) -> Self {
        Self {
            texture_page,
            scissor,
            first_index,
            index_count,
            blend_mode,
            _padding: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiRenderTextureArray {
    pub identity: [u8; 32],
    pub width: u32,
    pub height: u32,
    pub layers: u32,
    pub rgba8: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiRenderInput {
    pub revision: u64,
    pub viewport_size: [u32; 2],
    pub safe_area: [u32; 4],
    pub vertices: Arc<[UiRenderVertex]>,
    pub indices: Arc<[u32]>,
    pub batches: Arc<[UiRenderBatch]>,
    pub textures: Arc<UiRenderTextureArray>,
}

impl UiRenderInput {
    pub fn validate(&self) -> Result<(), UiRenderRejectReason> {
        validate_limit(self.vertices.len(), MAX_UI_VERTICES, |actual, limit| {
            UiRenderRejectReason::VertexLimitExceeded { actual, limit }
        })?;
        validate_limit(self.indices.len(), MAX_UI_INDICES, |actual, limit| {
            UiRenderRejectReason::IndexLimitExceeded { actual, limit }
        })?;
        validate_limit(self.batches.len(), MAX_UI_BATCHES, |actual, limit| {
            UiRenderRejectReason::BatchLimitExceeded { actual, limit }
        })?;
        if self.viewport_size.contains(&0) {
            return Err(UiRenderRejectReason::InvalidViewport);
        }
        let [left, top, right, bottom] = self.safe_area;
        if left.saturating_add(right) > self.viewport_size[0]
            || top.saturating_add(bottom) > self.viewport_size[1]
        {
            return Err(UiRenderRejectReason::InvalidSafeArea);
        }
        if self
            .vertices
            .iter()
            .any(|vertex| !vertex.position.iter().all(|value| value.is_finite()))
        {
            return Err(UiRenderRejectReason::NonFiniteVertex);
        }
        if self
            .indices
            .iter()
            .any(|index| *index as usize >= self.vertices.len())
        {
            return Err(UiRenderRejectReason::VertexIndexOutOfBounds);
        }
        validate_draw_bytes(self)?;
        validate_textures(&self.textures)?;
        validate_batches(self)?;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UiRenderRejectReason {
    VertexLimitExceeded { actual: usize, limit: usize },
    IndexLimitExceeded { actual: usize, limit: usize },
    BatchLimitExceeded { actual: usize, limit: usize },
    DrawByteLimitExceeded { actual: usize, limit: usize },
    InvalidViewport,
    InvalidSafeArea,
    NonFiniteVertex,
    VertexIndexOutOfBounds,
    EmptyBatch { batch: usize },
    BatchIndexRangeInvalid { batch: usize },
    BatchOrderInvalid { batch: usize },
    InvalidScissor { batch: usize },
    TexturePageOutOfBounds { batch: usize },
    UnsupportedBlendMode { batch: usize },
    InvalidTextureExtent,
    TextureByteLengthInvalid { actual: usize, expected: usize },
    TextureByteLimitExceeded { actual: usize, limit: usize },
    NoPublishedScene,
    StaleRevision { current: u64, rejected: u64 },
    RevisionConflict { revision: u64 },
    TextureIdentityConflict { identity: [u8; 32] },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiRenderReject {
    pub revision: u64,
    pub reason: UiRenderRejectReason,
}

impl fmt::Display for UiRenderReject {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "UI render revision {} rejected: {:?}",
            self.revision, self.reason
        )
    }
}

impl std::error::Error for UiRenderReject {}

#[derive(Resource, ExtractResource, Clone, Debug, Default, PartialEq)]
pub struct UiRenderScene {
    pub revision: u64,
    pub input: Option<Arc<UiRenderInput>>,
}

impl UiRenderScene {
    pub fn publish(
        &mut self,
        input: UiRenderInput,
        stats: &UiRenderStats,
    ) -> Result<(), UiRenderReject> {
        let revision = input.revision;
        let result = input.validate().and_then(|()| {
            if self.input.is_some() && revision < self.revision {
                Err(UiRenderRejectReason::StaleRevision {
                    current: self.revision,
                    rejected: revision,
                })
            } else if revision == self.revision
                && self
                    .input
                    .as_deref()
                    .is_some_and(|current| current != &input)
            {
                Err(UiRenderRejectReason::RevisionConflict { revision })
            } else if self.input.as_deref().is_some_and(|current| {
                current.textures.identity == input.textures.identity
                    && current.textures.as_ref() != input.textures.as_ref()
            }) {
                Err(UiRenderRejectReason::TextureIdentityConflict {
                    identity: input.textures.identity,
                })
            } else {
                Ok(())
            }
        });
        if let Err(reason) = result {
            stats.update(|snapshot| {
                snapshot.rejected_revision = Some(revision);
                snapshot.rejected_reason = Some(reason);
                snapshot.rejection_count = snapshot.rejection_count.saturating_add(1);
            });
            return Err(UiRenderReject { revision, reason });
        }
        if self
            .input
            .as_deref()
            .is_some_and(|current| current == &input)
        {
            return Ok(());
        }
        self.revision = revision;
        self.input = Some(Arc::new(input));
        stats.update(|snapshot| {
            snapshot.rejected_revision = None;
            snapshot.rejected_reason = None;
        });
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct UiRenderStatsSnapshot {
    pub accepted_revision: Option<u64>,
    pub uploaded_vertices: u32,
    pub uploaded_indices: u32,
    pub draw_calls: u32,
    pub retained_gpu_bytes: u64,
    pub vertex_arena_capacity: u32,
    pub index_arena_capacity: u32,
    pub per_node_gpu_allocations: u32,
    pub rejected_revision: Option<u64>,
    pub rejected_reason: Option<UiRenderRejectReason>,
    pub rejection_count: u64,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct UiRenderStats {
    inner: Arc<Mutex<UiRenderStatsSnapshot>>,
}

impl UiRenderStats {
    #[must_use]
    pub fn snapshot(&self) -> UiRenderStatsSnapshot {
        *self
            .inner
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(crate) fn update(&self, update: impl FnOnce(&mut UiRenderStatsSnapshot)) {
        update(
            &mut self
                .inner
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()),
        );
    }
}

fn validate_limit(
    actual: usize,
    limit: usize,
    reason: fn(usize, usize) -> UiRenderRejectReason,
) -> Result<(), UiRenderRejectReason> {
    if actual > limit {
        return Err(reason(actual, limit));
    }
    Ok(())
}

fn validate_draw_bytes(input: &UiRenderInput) -> Result<(), UiRenderRejectReason> {
    let bytes = input
        .vertices
        .len()
        .checked_mul(size_of::<UiRenderVertex>())
        .and_then(|vertices| {
            input
                .indices
                .len()
                .checked_mul(size_of::<u32>())
                .and_then(|indices| vertices.checked_add(indices))
        })
        .and_then(|used| {
            input
                .batches
                .len()
                .checked_mul(size_of::<UiRenderBatch>())
                .and_then(|batches| used.checked_add(batches))
        })
        .unwrap_or(usize::MAX);
    if bytes > MAX_UI_DRAW_BYTES {
        return Err(UiRenderRejectReason::DrawByteLimitExceeded {
            actual: bytes,
            limit: MAX_UI_DRAW_BYTES,
        });
    }
    Ok(())
}

fn validate_textures(textures: &UiRenderTextureArray) -> Result<(), UiRenderRejectReason> {
    if textures.width == 0
        || textures.height == 0
        || textures.layers == 0
        || textures.width > MAX_UI_TEXTURE_SIDE
        || textures.height > MAX_UI_TEXTURE_SIDE
        || textures.layers > MAX_UI_TEXTURE_LAYERS
    {
        return Err(UiRenderRejectReason::InvalidTextureExtent);
    }
    let expected = usize::try_from(textures.width)
        .ok()
        .and_then(|width| width.checked_mul(textures.height as usize))
        .and_then(|pixels| pixels.checked_mul(textures.layers as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .unwrap_or(usize::MAX);
    if expected > MAX_UI_TEXTURE_BYTES {
        return Err(UiRenderRejectReason::TextureByteLimitExceeded {
            actual: expected,
            limit: MAX_UI_TEXTURE_BYTES,
        });
    }
    if textures.rgba8.len() != expected {
        return Err(UiRenderRejectReason::TextureByteLengthInvalid {
            actual: textures.rgba8.len(),
            expected,
        });
    }
    Ok(())
}

fn validate_batches(input: &UiRenderInput) -> Result<(), UiRenderRejectReason> {
    let mut expected_first = 0usize;
    for (batch_index, batch) in input.batches.iter().enumerate() {
        if batch.index_count == 0 {
            return Err(UiRenderRejectReason::EmptyBatch { batch: batch_index });
        }
        if batch.first_index as usize != expected_first {
            return Err(UiRenderRejectReason::BatchOrderInvalid { batch: batch_index });
        }
        let Some(end) = expected_first.checked_add(batch.index_count as usize) else {
            return Err(UiRenderRejectReason::BatchIndexRangeInvalid { batch: batch_index });
        };
        if end > input.indices.len() {
            return Err(UiRenderRejectReason::BatchIndexRangeInvalid { batch: batch_index });
        }
        if batch.texture_page >= input.textures.layers {
            return Err(UiRenderRejectReason::TexturePageOutOfBounds { batch: batch_index });
        }
        if batch.blend_mode > UI_BLEND_INVERT {
            return Err(UiRenderRejectReason::UnsupportedBlendMode { batch: batch_index });
        }
        let scissor = batch.scissor;
        let within_viewport = scissor.width > 0
            && scissor.height > 0
            && scissor
                .x
                .checked_add(scissor.width)
                .is_some_and(|right| right <= input.viewport_size[0])
            && scissor
                .y
                .checked_add(scissor.height)
                .is_some_and(|bottom| bottom <= input.viewport_size[1]);
        if !within_viewport {
            return Err(UiRenderRejectReason::InvalidScissor { batch: batch_index });
        }
        expected_first = end;
    }
    if expected_first != input.indices.len() {
        return Err(UiRenderRejectReason::BatchIndexRangeInvalid {
            batch: input.batches.len(),
        });
    }
    Ok(())
}
