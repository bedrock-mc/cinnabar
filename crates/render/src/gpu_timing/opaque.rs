//! Metal timestamps must belong to the render pass whose work they measure.

use bevy::{
    camera::Viewport,
    core_pipeline::core_3d::{AlphaMask3d, MainOpaquePass3dNode, Opaque3d},
    ecs::query::QueryItem,
    prelude::*,
    render::{
        diagnostic::RecordDiagnostics,
        render_graph::{Node, NodeRunError, RenderGraphContext, ViewNode, ViewNodeRunner},
        render_phase::{TrackedRenderPass, ViewBinnedRenderPhases},
        render_resource::{CommandEncoderDescriptor, PipelineCache, RenderPassDescriptor, StoreOp},
        renderer::{RenderContext, RenderDevice},
    },
};

/// Uses the original single-pass opaque, alpha-mask and skybox draw sequence on Metal.
pub(super) fn replacement(world: &mut World) -> Option<Box<dyn Node>> {
    if !cfg!(target_os = "macos")
        || !world
            .get_resource::<RenderDevice>()?
            .features()
            .contains(wgpu::Features::TIMESTAMP_QUERY)
    {
        return None;
    }
    Some(Box::new(ViewNodeRunner::new(OpaqueTimingNode, world)))
}

pub(super) struct OpaqueTimingNode;

impl ViewNode for OpaqueTimingNode {
    type ViewQuery = <MainOpaquePass3dNode as ViewNode>::ViewQuery;

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (camera, view, target, depth, sky_pipeline, sky_group, offset, resolution): QueryItem<
            'w,
            '_,
            Self::ViewQuery,
        >,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let opaque = world
            .get_resource::<ViewBinnedRenderPhases<Opaque3d>>()
            .and_then(|phases| phases.get(&view.retained_view_entity));
        let alpha = world
            .get_resource::<ViewBinnedRenderPhases<AlphaMask3d>>()
            .and_then(|phases| phases.get(&view.retained_view_entity));
        let (Some(opaque), Some(alpha)) = (opaque, alpha) else {
            return Ok(());
        };
        let diagnostics = context.diagnostic_recorder();
        let colors = [Some(target.get_color_attachment())];
        let depth = Some(depth.get_attachment(StoreOp::Store));
        let view_entity = graph.view_entity();
        context.add_command_buffer_generation_task(move |device| {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("timed main opaque pass"),
            });
            let pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("timed main opaque pass"),
                color_attachments: &colors,
                depth_stencil_attachment: depth,
                timestamp_writes: super::render_pass_timestamps(
                    world,
                    crate::RuntimeStage::GpuOpaque,
                ),
                occlusion_query_set: None,
            });
            let mut pass = TrackedRenderPass::new(&device, pass);
            let diagnostic_span = diagnostics.pass_span(&mut pass, "main_opaque_pass_3d");
            if let Some(viewport) =
                Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
            {
                pass.set_camera_viewport(&viewport);
            }
            if !opaque.is_empty()
                && let Err(error) = opaque.render(&mut pass, world, view_entity)
            {
                error!("Opaque phase draw failed: {error:?}");
            }
            if !alpha.is_empty()
                && let Err(error) = alpha.render(&mut pass, world, view_entity)
            {
                error!("Alpha-mask phase draw failed: {error:?}");
            }
            if let (Some(pipeline), Some(group)) = (sky_pipeline, sky_group) {
                let cache = world.resource::<PipelineCache>();
                if let Some(pipeline) = cache.get_render_pipeline(pipeline.0) {
                    pass.set_render_pipeline(pipeline);
                    pass.set_bind_group(0, &group.0.0, &[offset.offset, group.0.1]);
                    pass.draw(0..3, 0..1);
                }
            }
            diagnostic_span.end(&mut pass);
            drop(pass);
            encoder.finish()
        });
        Ok(())
    }
}
