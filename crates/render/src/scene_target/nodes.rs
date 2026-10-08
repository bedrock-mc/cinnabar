use super::SceneTarget;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::{AlphaMask3d, Opaque3d, Transmissive3d, ViewTransmissionTexture},
    ecs::query::QueryItem,
    prelude::*,
    render::{
        camera::ExtractedCamera,
        render_graph::{NodeRunError, RenderGraphContext, ViewNode},
        render_phase::{ViewBinnedRenderPhases, ViewSortedRenderPhases},
        render_resource::{RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};

/// Cinnabar's opaque, cutout and sky phase items share samples with later world passes.
pub(super) struct SceneOpaquePass;

impl ViewNode for SceneOpaquePass {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static SceneTarget,
        &'static ViewDepthTexture,
        Option<&'static MainPassResolutionOverride>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (camera, view, target, scene, depth, resolution): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let (Some(opaque), Some(cutout)) = (
            world.get_resource::<ViewBinnedRenderPhases<Opaque3d>>(),
            world.get_resource::<ViewBinnedRenderPhases<AlphaMask3d>>(),
        ) else {
            return Ok(());
        };
        let (Some(opaque), Some(cutout)) = (
            opaque.get(&view.retained_view_entity),
            cutout.get(&view.retained_view_entity),
        ) else {
            return Ok(());
        };
        let attachments = [Some(scene.color_attachment(target, false))];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("main opaque and cutout scene"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuOpaque,
            ),
            occlusion_query_set: None,
        });
        if let Some(viewport) =
            Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
        {
            pass.set_camera_viewport(&viewport);
        }
        if !opaque.is_empty()
            && let Err(error) = opaque.render(&mut pass, world, graph.view_entity())
        {
            bevy::log::error!("Error rendering the opaque scene: {error:?}");
        }
        if !cutout.is_empty()
            && let Err(error) = cutout.render(&mut pass, world, graph.view_entity())
        {
            bevy::log::error!("Error rendering the cutout scene: {error:?}");
        }
        Ok(())
    }
}

/// Optional screen-space transmission keeps source samples intact between its snapshots.
pub(super) struct SceneTransmissivePass;

impl ViewNode for SceneTransmissivePass {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ExtractedView,
        &'static Camera3d,
        &'static ViewTarget,
        &'static SceneTarget,
        Option<&'static ViewTransmissionTexture>,
        &'static ViewDepthTexture,
        Option<&'static MainPassResolutionOverride>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (camera, view, settings, target, scene, transmission, depth, resolution): QueryItem<
            'w,
            '_,
            Self::ViewQuery,
        >,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let Some(phases) = world.get_resource::<ViewSortedRenderPhases<Transmissive3d>>() else {
            return Ok(());
        };
        let Some(phase) = phases.get(&view.retained_view_entity) else {
            return Ok(());
        };
        let count = phase.items.len();
        if count == 0 {
            return Ok(());
        }
        let snapshots = settings.screen_space_specular_transmission_steps;
        let steps = snapshots.max(1).min(count);
        let width = count / steps;
        let extra = count % steps;
        let mut start = 0;
        for step in 0..steps {
            if snapshots > 0 {
                let transmission = transmission.expect("transmission texture must be prepared");
                if scene.texture.sample_count() > 1 {
                    let attachments = [Some(
                        scene.resolve_attachment(target.main_texture_view(), StoreOp::Store),
                    )];
                    context
                        .command_encoder()
                        .begin_render_pass(&RenderPassDescriptor {
                            label: Some("transmission scene snapshot"),
                            color_attachments: &attachments,
                            depth_stencil_attachment: None,
                            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                                world,
                                crate::RuntimeStage::GpuTransparent,
                            ),
                            occlusion_query_set: None,
                        });
                }
                let source = if scene.texture.sample_count() > 1 {
                    target.main_texture()
                } else {
                    &scene.texture
                };
                context.command_encoder().copy_texture_to_texture(
                    source.as_image_copy(),
                    transmission.texture.as_image_copy(),
                    scene.texture.size(),
                );
            }
            let end = start + width + usize::from(step < extra);
            let attachments = [Some(scene.color_attachment(target, false))];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("main transmissive scene"),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
                timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuTransparent,
                ),
                occlusion_query_set: None,
            });
            if let Some(viewport) =
                Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
            {
                pass.set_camera_viewport(&viewport);
            }
            if let Err(error) =
                phase.render_range(&mut pass, world, graph.view_entity(), start..end)
            {
                bevy::log::error!("Error rendering the transmissive scene: {error:?}");
            }
            start = end;
        }
        Ok(())
    }
}

/// Only this boundary consumes world colour samples before single-sample post-processing and HUD.
pub(super) struct SceneFinish;

impl ViewNode for SceneFinish {
    type ViewQuery = (&'static ViewTarget, &'static SceneTarget);

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, scene): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        scene.finish(context, world, target);
        Ok(())
    }
}
