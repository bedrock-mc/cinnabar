//! Ordered gamma-layer raster replay, optionally restricted to a conservative damage rectangle.
use super::*;
use super::{damage::UiDamage, overlay::retained_batch_ranges};
use bevy::{
    camera::Viewport,
    render::{
        render_resource::{
            LoadOp, Operations, RenderPassDepthStencilAttachment, RenderPassDescriptor, StoreOp,
        },
        renderer::RenderContext,
    },
};
use render_model::UiScissor;
use std::ops::Range;

pub(super) struct UiLayerDraw<'a> {
    pub(super) world: &'a World,
    pub(super) gpu: &'a UiGpu,
    pub(super) pipeline_cache: &'a PipelineCache,
    pub(super) alpha: &'a RenderPipeline,
    pub(super) vertices: &'a Buffer,
    pub(super) indices: &'a Buffer,
    pub(super) owner: Entity,
    pub(super) layer: &'a super::composite::UiLayerTexture,
    pub(super) model_depth: Option<&'a super::model_depth::UiModelDepth>,
    pub(super) viewport: Option<&'a Viewport>,
    pub(super) skip: Option<&'a Range<u32>>,
    pub(super) clear: Option<&'a RenderPipeline>,
}

impl<'a> UiLayerDraw<'a> {
    /// Uses the same readiness check before partial replay and when encoding each group.
    pub(super) fn pipeline(&self, batch: &UiRenderBatch) -> Option<&'a RenderPipeline> {
        if batch.depth_test == 0 && batch.depth_write == 0 {
            return Some(self.alpha);
        }
        self.model_depth
            .and_then(|_| {
                self.gpu.model_view_pipelines.get(&(
                    self.owner,
                    batch.depth_test != 0,
                    batch.depth_write != 0,
                ))
            })
            .and_then(|pair| self.pipeline_cache.get_render_pipeline(pair.0))
    }
}

/// Clears each isolated model depth lifetime once and preserves its later materials.
pub(super) fn model_depth_attachment<'a>(
    depth: &'a super::model_depth::UiModelDepth,
    lifetime: &super::model_depth::ModelDepthLifetime,
) -> RenderPassDepthStencilAttachment<'a> {
    RenderPassDepthStencilAttachment {
        view: &depth.view,
        depth_ops: Some(Operations {
            load: if lifetime.cleared() {
                LoadOp::Load
            } else {
                LoadOp::Clear(0.0)
            },
            store: StoreOp::Store,
        }),
        stencil_ops: None,
    }
}

/// Material passes load the same gamma layer in authored order; each control owns
/// a fresh model-depth clear, shared by its later translucent/read-only materials.
pub(super) fn draw_ui_layer(
    context: &mut RenderContext,
    draw: &UiLayerDraw<'_>,
    batches: &[(usize, &UiRenderBatch, render_model::UiTextureLocation)],
    lifetime: &mut super::model_depth::ModelDepthLifetime,
    damage: Option<UiScissor>,
) -> LayerDrawn {
    let mut encoded = false;
    let mut complete = true;
    let mut start = 0;
    while let Some((_, first, _)) = batches.get(start) {
        let mode = (
            first.isolated_depth_scope,
            first.depth_test,
            first.depth_write,
        );
        let length = batches[start..]
            .iter()
            .take_while(|(_, batch, _)| {
                (
                    batch.isolated_depth_scope,
                    batch.depth_test,
                    batch.depth_write,
                ) == mode
            })
            .count();
        let group = &batches[start..start + length];
        start += length;
        lifetime.enter(mode.0);
        let needs_depth = mode.1 != 0 || mode.2 != 0;
        let Some(pipeline) = draw.pipeline(first) else {
            complete = false;
            continue;
        };
        let attachments = [Some(
            bevy::render::render_resource::RenderPassColorAttachment {
                view: &draw.layer.view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: if encoded || damage.is_some() {
                        LoadOp::Load
                    } else {
                        LoadOp::Clear(Default::default())
                    },
                    store: StoreOp::Store,
                },
            },
        )];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("ordered gamma-space UI/model material layer"),
            color_attachments: &attachments,
            depth_stencil_attachment: needs_depth
                .then(|| model_depth_attachment(draw.model_depth.unwrap(), lifetime)),
            timestamp_writes: crate::gpu_timing::ui_pass_timestamps(
                draw.world,
                if mode.0.is_some() {
                    crate::RuntimeStage::GpuUiModel
                } else {
                    crate::RuntimeStage::GpuUiRaster
                },
            ),
            occlusion_query_set: None,
        });
        if !encoded && let Some(rect) = damage {
            pass.set_render_pipeline(
                draw.clear
                    .expect("partial replay requires the clear pipeline"),
            );
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.draw(0..3, 0..1);
            if let Some(profile) = draw.world.get_resource::<super::profile::UiProfile>() {
                profile.record_draw(crate::RuntimeStage::GpuUiRaster, 0);
            }
        }
        pass.set_render_pipeline(pipeline);
        draw_batches(
            &mut pass,
            draw.gpu,
            draw.vertices,
            draw.indices,
            draw.viewport,
            group,
            draw.skip,
            damage,
            draw.world
                .get_resource::<super::profile::UiProfile>()
                .map(|profile| {
                    (
                        profile,
                        if mode.0.is_some() {
                            crate::RuntimeStage::GpuUiModel
                        } else {
                            crate::RuntimeStage::GpuUiRaster
                        },
                    )
                }),
        );
        if needs_depth {
            lifetime.encoded();
        }
        encoded = true;
    }
    LayerDrawn { encoded, complete }
}

pub(super) struct LayerDrawn {
    pub(super) encoded: bool,
    /// False when a still-compiling pipeline left a group out, so the layer must not be retained.
    pub(super) complete: bool,
}

/// Draw `batches` into `pass`, each under its own scissor and page bind group.
#[allow(clippy::too_many_arguments)] // Optional diagnostics accompany the existing draw state.
pub(super) fn draw_batches<'w>(
    pass: &mut bevy::render::render_phase::TrackedRenderPass<'w>,
    gpu: &'w UiGpu,
    vertices: &'w Buffer,
    indices: &'w Buffer,
    viewport: Option<&Viewport>,
    batches: &[(usize, &UiRenderBatch, render_model::UiTextureLocation)],
    skip: Option<&Range<u32>>,
    damage: Option<UiScissor>,
    profile: Option<(&super::profile::UiProfile, crate::RuntimeStage)>,
) {
    if let Some((profile, stage)) = profile {
        profile.record_pass(stage);
    }
    if let Some(viewport) = viewport {
        pass.set_camera_viewport(viewport);
    }
    pass.set_vertex_buffer(0, vertices.slice(..));
    pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
    for (_, batch, location) in batches {
        let Some(scissor) = clipped_scissor(batch.scissor, damage) else {
            continue;
        };
        let binding = gpu.textures.buckets[location.bucket]
            .bind_group
            .as_ref()
            .unwrap();
        pass.set_bind_group(0, binding, &[]);
        pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
        for range in retained_batch_ranges(batch, skip).into_iter().flatten() {
            if let Some((profile, stage)) = profile {
                profile.record_draw(stage, range.end - range.start);
            }
            pass.draw_indexed(range, 0, location.layer..location.layer + 1);
        }
    }
    pass.set_scissor_rect(0, 0, gpu.viewport_size[0], gpu.viewport_size[1]);
}

/// Partial replay needs an ordinary first pass and every pipeline ready before preserving pixels.
pub(super) fn damage_for_passes(
    damage: UiDamage,
    first: Option<&UiRenderBatch>,
    clear_ready: bool,
    materials_ready: bool,
) -> UiDamage {
    if matches!(damage, UiDamage::Rect(_))
        && !(clear_ready
            && materials_ready
            && first.is_some_and(|batch| {
                batch.isolated_depth_scope.is_none()
                    && batch.depth_test == 0
                    && batch.depth_write == 0
            }))
    {
        return UiDamage::Full;
    }
    damage
}

/// Intersects the batch scissor with damage without widening either draw's coverage.
pub(super) fn clipped_scissor(scissor: UiScissor, damage: Option<UiScissor>) -> Option<UiScissor> {
    let Some(damage) = damage else {
        return Some(scissor);
    };
    let left = scissor.x.max(damage.x);
    let top = scissor.y.max(damage.y);
    let right = scissor
        .x
        .checked_add(scissor.width)?
        .min(damage.x.checked_add(damage.width)?);
    let bottom = scissor
        .y
        .checked_add(scissor.height)?
        .min(damage.y.checked_add(damage.height)?);
    (left < right && top < bottom).then(|| UiScissor::new(left, top, right - left, bottom - top))
}

#[cfg(test)]
#[path = "layer_tests.rs"]
mod tests;
