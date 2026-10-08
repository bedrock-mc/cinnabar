use super::HandRigGpu;
use bevy::{
    ecs::query::QueryItem,
    prelude::*,
    render::{
        render_graph::{NodeRunError, RenderGraphContext, ViewNode},
        render_resource::{
            LoadOp, Operations, PipelineCache, RenderPassDepthStencilAttachment,
            RenderPassDescriptor, StoreOp,
        },
        renderer::RenderContext,
        view::ViewTarget,
    },
};

/// Draws the local player's first-person rig over the scene, in a private reverse-Z depth pass
/// with its own near-camera projection, so the arm never z-fights the world.
pub(super) struct HandRigViewNode;

impl ViewNode for HandRigViewNode {
    type ViewQuery = (
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static Msaa,
        Has<crate::EnhancedRendering>,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (target, scene_target, msaa, enhanced): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        let (Some(gpu), Some(cache)) = (
            world.get_resource::<HandRigGpu>(),
            world.get_resource::<PipelineCache>(),
        ) else {
            return Ok(());
        };
        let Some(lightmap) = world.get_resource::<crate::lighting::LightmapGpu>() else {
            return Ok(());
        };
        let color = &scene_target.texture;
        let extent = color.size();
        let (Some(depth), Some(binding), Some(pipeline)) = (
            &gpu.depth,
            &gpu.bind_group,
            gpu.pipeline.and_then(|id| cache.get_render_pipeline(id)),
        ) else {
            return Ok(());
        };
        // The depth target must match the view it composites over; a stale size skips this frame.
        if depth.size != [extent.width, extent.height]
            || depth.samples != color.sample_count()
            || depth.samples != msaa.samples()
            || gpu.maximum_vertex_count == 0
        {
            return Ok(());
        }
        let final_world_draw =
            !(render_model::enhanced_rendering_enabled() && enhanced) && msaa.samples() > 1;
        let attachments = [Some(if final_world_draw {
            scene_target.final_attachment(target.main_texture_view())
        } else {
            scene_target.color_attachment(target, false)
        })];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("first-person animated rig"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                view: &depth.view,
                depth_ops: Some(Operations {
                    load: LoadOp::Clear(0.0),
                    store: StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuHand,
            ),
            occlusion_query_set: None,
        });
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, binding, &[]);
        pass.set_bind_group(1, &lightmap.bind_group, &[]);
        pass.draw(0..gpu.maximum_vertex_count, 0..gpu.instance_count.max(1));
        Ok(())
    }
}
