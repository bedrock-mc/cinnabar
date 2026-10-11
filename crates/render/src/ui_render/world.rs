//! Direct world-projected UI pass with the native text depth modes.
use super::*;

type UiViewQuery = (
    &'static ViewTarget,
    &'static crate::scene_target::SceneTarget,
    &'static MainEntity,
    &'static ExtractedCamera,
    Option<&'static ViewDepthStencilTexture>,
    Option<&'static MainPassResolutionOverride>,
);

/// Draws projected UI in authored order after world filtering.
pub(super) fn ui_world(
    world: &World,
    view: bevy::render::renderer::ViewQuery<UiViewQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    draw_ui_view(view.entity(), &mut context, view.into_inner(), world)
}

/// Draw projected batches in authored order under their native depth modes.
fn draw_ui_view(
    view_entity: Entity,
    context: &mut RenderContext,
    (target, scene_target, _, camera, depth, resolution_override): QueryItem<UiViewQuery>,
    world: &World,
) -> Result<(), BevyError> {
    let (Some(gpu), Some(pipeline_cache)) = (
        world.get_resource::<UiGpu>(),
        world.get_resource::<PipelineCache>(),
    ) else {
        return Ok(());
    };
    let (Some(vertices), Some(indices), Some(_)) = (
        &gpu.vertex_buffer,
        &gpu.index_buffer,
        overlay_pipeline_pair(&gpu.batches, &gpu.view_pipelines, view_entity),
    ) else {
        return Ok(());
    };
    if gpu.textures.buckets.len() != gpu.textures.allocated_buckets().len()
        || gpu
            .textures
            .buckets
            .iter()
            .any(|bucket| bucket.bind_group.is_none())
    {
        return Ok(());
    }
    let Some(batches) = resolved_batches(
        gpu.accepted_revision,
        &gpu.batches,
        &gpu.textures.locations,
        gpu.textures.allocated_buckets(),
    ) else {
        return Ok(());
    };
    // Separate passes keep depth/no-depth and read-only/writable attachment states compatible.
    // Preserve the authored plate/text order across every depth mode.
    let mut batches = batches
        .filter(|(_, batch, _)| batch.world_projection != 0)
        .peekable();
    while let Some((_, first, _)) = batches.peek().copied() {
        let depth_test = first.depth_test != 0;
        let depth_write = first.depth_write != 0;
        let matches_depth = |batch: &UiRenderBatch| {
            (batch.depth_test != 0, batch.depth_write != 0) == (depth_test, depth_write)
        };
        let depth = depth.filter(|depth| world_depth_compatible(depth, scene_target));
        let pair = if depth_test || depth_write {
            depth.and_then(|_| {
                gpu.world_view_pipelines
                    .get(&(view_entity, depth_test, depth_write))
            })
        } else {
            gpu.world_view_pipelines.get(&(view_entity, false, false))
        };
        let Some((alpha, invert)) = pair else {
            while batches
                .next_if(|(_, batch, _)| matches_depth(batch))
                .is_some()
            {}
            continue;
        };
        let Some(pipeline) = pipeline_cache.get_render_pipeline(*alpha) else {
            while batches
                .next_if(|(_, batch, _)| matches_depth(batch))
                .is_some()
            {}
            continue;
        };
        let attachments = [Some(scene_target.color_attachment(target, false))];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("retained world-projected UI"),
            color_attachments: &attachments,
            depth_stencil_attachment: if depth_test || depth_write {
                depth.map(|depth| RenderPassDepthStencilAttachment {
                    view: depth.attachment.depth_stencil_views().attachment_view(),
                    depth_ops: depth_write.then_some(Operations {
                        load: LoadOp::Load,
                        store: StoreOp::Store,
                    }),
                    stencil_ops: None,
                })
            } else {
                None
            },
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuUi,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_render_pipeline(pipeline);
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
        let mut invert_bound = false;
        while let Some((_, batch, location)) = batches.next_if(|(_, batch, _)| matches_depth(batch))
        {
            let binding = gpu.textures.buckets[location.bucket]
                .bind_group
                .as_ref()
                .unwrap();
            pass.set_bind_group(0, binding, &[]);
            let wants_invert = batch.blend_mode == UI_BLEND_INVERT;
            if wants_invert != invert_bound {
                let id = if wants_invert { *invert } else { *alpha };
                let Some(pipeline) = pipeline_cache.get_render_pipeline(id) else {
                    // The invert variant is still compiling; skip its batches
                    // this frame rather than drawing them with the wrong blend.
                    continue;
                };
                pass.set_render_pipeline(pipeline);
                invert_bound = wants_invert;
            }
            let Some(scissor) = crate::render_bounds::scissor(
                batch.scissor,
                crate::render_bounds::extent(scene_target.color_view(false)),
            ) else {
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

/// Only bind depth with the owning color target's dimensions and sample count.
fn world_depth_compatible(
    depth: &ViewDepthStencilTexture,
    target: &crate::scene_target::SceneTarget,
) -> bool {
    let color = &target.texture;
    depth.texture().format() == CORE_3D_DEPTH_FORMAT
        && depth.texture().sample_count() == color.sample_count()
        && depth.texture().size() == color.size()
}
