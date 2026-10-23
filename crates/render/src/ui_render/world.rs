//! World-projected UI shares compatible passes while each material keeps its native depth mode.
use super::*;
use bevy::{ecs::query::QueryItem, render::render_resource::RenderPassDepthStencilAttachment};

pub(crate) type UiViewQuery = (
    &'static ViewTarget,
    &'static crate::scene_target::SceneTarget,
    &'static MainEntity,
    &'static ExtractedCamera,
    Option<&'static ViewDepthStencilTexture>,
    Option<&'static MainPassResolutionOverride>,
    Option<&'static crate::render_scale::RenderScaleOutput>,
);

/// Draws projected UI in authored order after world filtering.
pub(crate) fn ui_world<const AFTER_PACK: bool>(
    world: &World,
    view: bevy::render::renderer::ViewQuery<UiViewQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    if !crate::shaderpack::native_pass_enabled(world, view.entity(), AFTER_PACK) {
        return Ok(());
    }
    let Some(gpu) = world.get_resource::<UiGpu>() else {
        return Ok(());
    };
    draw_ui_view(
        view.entity(),
        &mut context,
        view.into_inner(),
        world,
        gpu,
        &gpu.batches,
    )
}

/// Shares projected batches across compatible attachments, preserving material depth behavior.
pub(crate) fn draw_ui_view(
    view_entity: Entity,
    context: &mut RenderContext,
    (target, scene_target, _, camera, depth, resolution_override, output): QueryItem<UiViewQuery>,
    world: &World,
    gpu: &UiGpu,
    batches: &[UiRenderBatch],
) -> Result<(), BevyError> {
    let Some(pipeline_cache) = world.get_resource::<PipelineCache>() else {
        return Ok(());
    };
    let (Some(vertices), Some(indices), Some(_)) = (
        &gpu.vertex_buffer,
        &gpu.index_buffer,
        overlay_pipeline_pair(batches, &gpu.view_pipelines, view_entity),
    ) else {
        return Ok(());
    };
    let Some(batches) = resolved_batches(
        gpu.accepted_revision,
        batches,
        &gpu.textures.locations,
        gpu.textures.allocated_buckets(),
    ) else {
        return Ok(());
    };
    let batches = batches.filter(|(_, batch, _)| {
        batch.world_projection != 0
            && gpu
                .textures
                .page_binding(batch.texture_page as usize)
                .is_some()
    });
    let depth = depth.filter(|depth| world_depth_compatible(depth, scene_target));
    let ready = |test: bool, write: bool, attached: bool| {
        if (test || write) && depth.is_none() {
            return None;
        }
        let &(alpha, invert) =
            gpu.world_view_pipelines
                .get(&(view_entity, test, write, attached))?;
        // Preserve the original readiness gate, including for invert-only batches.
        let alpha = (alpha, pipeline_cache.get_render_pipeline(alpha)?);
        let invert = pipeline_cache
            .get_render_pipeline(invert)
            .map(|pipeline| (invert, pipeline));
        Some((alpha, invert))
    };
    // Two independent depth flags give four material modes; resolve each only once per view.
    let materials: [_; 4] = std::array::from_fn(|mode| ready(mode & 2 != 0, mode & 1 != 0, false));
    let attached = ready(false, false, true);
    let pipeline = |batch: &UiRenderBatch, shared: bool| {
        let mode = usize::from(batch.depth_test != 0) * 2 + usize::from(batch.depth_write != 0);
        let wants_invert = batch.blend_mode == UI_BLEND_INVERT;
        let (alpha, invert) = materials[mode]?;
        let original = if wants_invert { invert } else { Some(alpha) }?;
        if shared && mode == 0 {
            let (alpha, invert) = attached?;
            if wants_invert { invert } else { Some(alpha) }
        } else {
            Some(original)
        }
    };
    // Read-only depth is enough unless a ready material writes it. No-depth frames stay so.
    let depth_write = batches
        .clone()
        .filter_map(|(_, batch, _)| {
            ((batch.depth_test != 0 || batch.depth_write != 0) && pipeline(batch, false).is_some())
                .then_some(batch.depth_write != 0)
        })
        .reduce(|left, right| left || right);
    // Keep no-depth draws visible while the compatible variants are still compiling.
    let share_depth = depth_write.is_some()
        && batches.clone().all(|(_, batch, _)| {
            batch.depth_test != 0
                || batch.depth_write != 0
                || pipeline(batch, false).is_none()
                || pipeline(batch, true).is_some()
        });
    let mut batches = batches
        .filter_map(|(_, batch, location)| {
            pipeline(batch, share_depth).map(|(id, pipeline)| {
                (
                    batch,
                    location,
                    id,
                    pipeline,
                    share_depth || batch.depth_test != 0 || batch.depth_write != 0,
                )
            })
        })
        .peekable();
    while let Some(needs_depth) = batches.peek().map(|batch| batch.4) {
        #[cfg(test)]
        gpu.world_passes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let attachments = [Some(scene_target.color_attachment(target, false))];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("retained world-projected UI"),
            color_attachments: &attachments,
            depth_stencil_attachment: depth_write.filter(|_| needs_depth).and_then(|write| {
                depth.map(|depth| RenderPassDepthStencilAttachment {
                    view: depth.attachment.depth_stencil_views().attachment_view(),
                    depth_ops: write.then_some(Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                })
            }),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                render_scene::RuntimeStage::GpuUi,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) = overlay_viewport(camera.viewport.as_ref(), resolution_override) {
            let Some(viewport) = crate::render_bounds::viewport(
                &viewport,
                crate::render_bounds::extent(scene_target.color_view(false)),
            ) else {
                return Ok(());
            };
            pass.set_camera_viewport(&viewport);
        }
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        let mut bound = None;
        while let Some((batch, location, id, pipeline, _)) =
            batches.next_if(|batch| batch.4 == needs_depth)
        {
            if bound != Some(id) {
                pass.set_render_pipeline(pipeline);
                bound = Some(id);
            }
            pass.set_bind_group(
                0,
                gpu.textures
                    .page_binding(batch.texture_page as usize)
                    .unwrap(),
                &[],
            );
            let extent = crate::render_bounds::extent(scene_target.color_view(false));
            let scissor = match output {
                Some(output) => {
                    super::layer::scaled_scissor(batch.scissor, output.0.to_array(), extent)
                }
                None => crate::render_bounds::scissor(batch.scissor, extent),
            };
            let Some(scissor) = scissor else {
                continue;
            };
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
            for range in retained_batch_ranges(batch, None).into_iter().flatten() {
                pass.draw_indexed(range, 0, location.layer..location.layer + 1);
            }
        }
    }
    Ok(())
}

/// Only binds depth with the owning color target's dimensions and sample count.
fn world_depth_compatible(
    depth: &ViewDepthStencilTexture,
    target: &crate::scene_target::SceneTarget,
) -> bool {
    let color = &target.texture;
    depth.texture().format() == CORE_3D_DEPTH_FORMAT
        && depth.texture().sample_count() == color.sample_count()
        && depth.texture().size() == color.size()
}
