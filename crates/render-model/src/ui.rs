use std::{
    fmt,
    mem::size_of,
    sync::{Arc, Mutex},
};

use bytemuck::{Pod, Zeroable};

pub const MAX_UI_VERTICES: usize = 262_144;
pub const MAX_UI_INDICES: usize = 393_216;
pub const MAX_UI_BATCHES: usize = 8_192;
pub const MAX_UI_DRAW_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_UI_TEXTURE_SIDE: u32 = 4_096;
pub const MAX_UI_TEXTURE_LAYERS: u32 = 256;
/// Fixed artwork and blank native slots retain the ordinary UI and Unicode budgets.
pub const MAX_UI_FIXED_TEXTURE_BYTES: usize = 128 * 1024 * 1024
    + assets::FONT_FALLBACK_ATLAS_SIDE as usize
        * assets::FONT_FALLBACK_ATLAS_SIDE as usize
        * assets::MAX_FONT_FALLBACK_PAGES;
/// Native slot growth has dedicated capacity without allocating unused reserves.
pub const MAX_UI_TEXTURE_BYTES: usize =
    MAX_UI_FIXED_TEXTURE_BYTES + crate::ui_textures::MAX_UI_NATIVE_TEXTURE_GROWTH_BYTES;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct UiRenderVertex {
    /// Physical screen XY multiplied by clip W; no perspective divide is done on the CPU.
    pub position: [f32; 2],
    pub clip_z: f32,
    pub clip_w: f32,
    pub uv: [f32; 2],
    pub color: [u8; 4],
    pub style_flags: u32,
    /// Negative disables explicit model-material sampled alpha testing.
    pub alpha_cutoff: f32,
    /// Native linear model lighting, interpolated without byte-color quantization.
    pub model_light: f32,
    /// Native entity overlay RGB and its mix amount, without byte quantization.
    pub overlay_color: [f32; 4],
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
/// Elliptical gradient: UV is the centered radius coordinate, tint is the inner
/// straight RGBA stop, and overlay_color is the outer straight RGBA stop.
pub const UI_STYLE_RADIAL_GRADIENT: u32 = 1;
/// Animated item glint; the only vertex style that changes pixels without a new revision.
pub const UI_STYLE_GLINT: u32 = 1 << 1;
/// Reject sampled texture alpha below one half before multiplying vertex alpha.
pub const UI_STYLE_ALPHA_TEST: u32 = 1 << 4;
/// Texture alpha weights dye color; every surviving sampled texel is opaque.
pub const UI_STYLE_COLOR_MASK: u32 = 1 << 7;

#[cfg(test)]
mod style_tests {
    use super::*;

    #[test]
    fn font_coverage_never_enables_opaque_model_color_masks() {
        for rendering in [
            assets::FontRendering::Coverage,
            assets::FontRendering::NativeCoverage,
            assets::FontRendering::NativeSdf,
        ] {
            assert_eq!(
                u32::from(rendering.style_flags()) & UI_STYLE_COLOR_MASK,
                0,
                "text coverage must remain transparent outside glyphs"
            );
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UiRenderBatch {
    pub texture_page: u32,
    pub scissor: UiScissor,
    pub first_index: u32,
    pub index_count: u32,
    /// One of [`UI_BLEND_ALPHA`] or [`UI_BLEND_INVERT`]; any other value is
    /// rejected at publication.
    pub blend_mode: u32,
    /// 0 for Always/no comparison, 1 for reverse-Z world-depth testing.
    pub depth_test: u32,
    pub depth_write: u32,
    /// 1 for world-projected UI, rendered before near-camera hands and ordinary HUD.
    pub world_projection: u32,
    /// An ordered screen-space model's depth lifetime, separate from world depth.
    pub isolated_depth_scope: Option<u32>,
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
            depth_test: 0,
            depth_write: 0,
            world_projection: 0,
            isolated_depth_scope: None,
        }
    }

    #[must_use]
    pub const fn with_depth_test(mut self, depth_test: bool) -> Self {
        self.depth_test = depth_test as u32;
        if depth_test {
            self.world_projection = 1;
        }
        self
    }

    #[must_use]
    pub const fn with_world_projection(mut self, projected: bool) -> Self {
        self.world_projection = projected as u32;
        self
    }

    #[must_use]
    pub const fn with_depth_write(mut self, depth_write: bool) -> Self {
        self.depth_write = depth_write as u32;
        if depth_write {
            self.world_projection = 1;
        }
        self
    }

    #[must_use]
    pub const fn with_isolated_depth_scope(mut self, scope: Option<u32>) -> Self {
        self.isolated_depth_scope = scope;
        if scope.is_some() {
            self.world_projection = 0;
        }
        self
    }
}

pub type UiRenderTextureArray = crate::ui_textures::UiTextureCatalog;

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
        if self.vertices.iter().any(|vertex| {
            !vertex.position.iter().all(|value| value.is_finite())
                || !vertex.clip_z.is_finite()
                || !vertex.clip_w.is_finite()
                || !vertex.uv.iter().all(|value| value.is_finite())
                || !vertex.alpha_cutoff.is_finite()
                || vertex.alpha_cutoff > 1.0
                || !vertex.model_light.is_finite()
                || vertex.model_light < 0.0
                || !vertex.overlay_color.iter().all(|value| value.is_finite())
        }) {
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
        // Texture catalogs have private fields and checked constructors; no
        // raster hashing or immutable byte traversal occurs during publication.
        validate_batches(self)?;
        Ok(())
    }

    /// Same revision, viewport and safe area over the very same buffers.
    fn shares_buffers(&self, other: &Self) -> bool {
        self.revision == other.revision
            && self.viewport_size == other.viewport_size
            && self.safe_area == other.safe_area
            && Arc::ptr_eq(&self.vertices, &other.vertices)
            && Arc::ptr_eq(&self.indices, &other.indices)
            && Arc::ptr_eq(&self.batches, &other.batches)
            && Arc::ptr_eq(&self.textures, &other.textures)
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
    UnsupportedDepthTest { batch: usize },
    UnsupportedDepthWrite { batch: usize },
    UnsupportedWorldProjection { batch: usize },
    InvalidIsolatedDepthScope { batch: usize },
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

#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiRenderScene {
    pub revision: u64,
    pub input: Option<Arc<UiRenderInput>>,
    rejected_since_publish: bool,
    admitted_static_identity: Option<[u8; 32]>,
}

impl UiRenderScene {
    pub fn publish(
        &mut self,
        input: UiRenderInput,
        stats: &UiRenderStats,
    ) -> Result<(), UiRenderReject> {
        // The UI republishes its last frame unchanged most of the time; the admitted copy
        // was already validated, so identical buffers need no per-vertex work.
        if self
            .input
            .as_deref()
            .is_some_and(|current| current.shares_buffers(&input))
        {
            return Ok(());
        }
        let revision = input.revision;
        let result = input.validate().and_then(|()| {
            if revision < self.revision
                || (self.rejected_since_publish && revision == self.revision)
            {
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
            } else if self
                .admitted_static_identity
                .is_some_and(|identity| identity != input.textures.static_identity())
            {
                Err(UiRenderRejectReason::TextureIdentityConflict {
                    identity: input.textures.identity(),
                })
            } else {
                Ok(())
            }
        });
        if let Err(reason) = result {
            self.input = None;
            self.rejected_since_publish = true;
            stats.update(|snapshot| {
                snapshot.accepted_revision = None;
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
        self.admitted_static_identity = Some(input.textures.static_identity());
        self.rejected_since_publish = false;
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

/// Shared handle: clones observe and update the same snapshot.
#[derive(Clone, Debug, Default)]
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

    pub fn update(&self, update: impl FnOnce(&mut UiRenderStatsSnapshot)) {
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

fn validate_batches(input: &UiRenderInput) -> Result<(), UiRenderRejectReason> {
    let mut expected_first = 0usize;
    let mut previous_scope = None;
    let mut completed_scopes = std::collections::BTreeSet::new();
    for (batch_index, batch) in input.batches.iter().enumerate() {
        if batch.isolated_depth_scope != previous_scope {
            if let Some(scope) = previous_scope {
                completed_scopes.insert(scope);
            }
            if batch
                .isolated_depth_scope
                .is_some_and(|scope| completed_scopes.contains(&scope))
            {
                return Err(UiRenderRejectReason::InvalidIsolatedDepthScope { batch: batch_index });
            }
            previous_scope = batch.isolated_depth_scope;
        }
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
        if batch.texture_page as usize >= input.textures.pages().len() {
            return Err(UiRenderRejectReason::TexturePageOutOfBounds { batch: batch_index });
        }
        if batch.blend_mode > UI_BLEND_INVERT {
            return Err(UiRenderRejectReason::UnsupportedBlendMode { batch: batch_index });
        }
        if batch.depth_test > 1 {
            return Err(UiRenderRejectReason::UnsupportedDepthTest { batch: batch_index });
        }
        if batch.depth_write > 1 {
            return Err(UiRenderRejectReason::UnsupportedDepthWrite { batch: batch_index });
        }
        if batch.world_projection > 1
            || (batch.isolated_depth_scope.is_some() && batch.world_projection != 0)
            || ((batch.depth_test == 1 || batch.depth_write == 1)
                && batch.world_projection == 0
                && batch.isolated_depth_scope.is_none())
        {
            return Err(UiRenderRejectReason::UnsupportedWorldProjection { batch: batch_index });
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
