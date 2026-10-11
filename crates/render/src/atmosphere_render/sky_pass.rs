//! The ordered sky pass owns colour initialization before any world geometry.

use super::{AtmosphereGpu, AtmospherePipelineKey};
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::query::QueryItem,
    prelude::{Msaa, World},
    render::{
        camera::ExtractedCamera,
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{PipelineCache, RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::{ExtractedView, ViewDepthTexture, ViewTarget, ViewUniformOffset},
    },
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct AtmospherePassLabel;

/// Places all sky layers ahead of the boundary used by both ordinary and direct terrain.
pub(super) fn install_graph(world: &mut World) {
    let node = ViewNodeRunner::new(AtmosphereNode, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    if graph.get_node_state(AtmospherePassLabel).is_ok() {
        return;
    }
    graph.add_node(AtmospherePassLabel, node);
    graph.add_node_edges((
        Node3d::EndPrepasses,
        AtmospherePassLabel,
        Node3d::StartMainPass,
    ));
}

struct AtmosphereNode;

impl ViewNode for AtmosphereNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ExtractedView,
        &'static ViewTarget,
        Option<&'static crate::scene_target::SceneTarget>,
        &'static ViewDepthTexture,
        &'static ViewUniformOffset,
        &'static Msaa,
        Option<&'static MainPassResolutionOverride>,
    );

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (camera, view, target, scene, depth, offset, msaa, resolution): QueryItem<
            'w,
            '_,
            Self::ViewQuery,
        >,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        if world
            .get_resource::<crate::PanoramaScene>()
            .is_some_and(|scene| !scene.game_visible())
        {
            return Ok(());
        }
        let Some(gpu) = world.get_resource::<AtmosphereGpu>() else {
            return Ok(());
        };
        let Some(bind_group) = gpu.bind_group.as_ref() else {
            return Ok(());
        };
        let cache = world.resource::<PipelineCache>();
        let key = AtmospherePipelineKey {
            msaa: *msaa,
            hdr: view.hdr,
            stars: false,
            skybox: false,
        };
        let Some(stock) = gpu
            .pipelines
            .get(&key)
            .and_then(|id| cache.get_render_pipeline(*id))
        else {
            return Ok(());
        };
        let colour = scene.map_or_else(
            || target.get_color_attachment(),
            |scene| scene.color_attachment(target, false),
        );
        let extent = crate::render_bounds::extent(colour.view);
        let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
            .map(|viewport| crate::render_bounds::viewport(&viewport, extent));
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("sky before world geometry"),
            color_attachments: &[Some(colour)],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuSky,
            ),
            occlusion_query_set: None,
        });
        match viewport {
            Some(Some(viewport)) => pass.set_camera_viewport(&viewport),
            Some(None) => return Ok(()),
            None => {}
        }
        pass.set_render_pipeline(stock);
        pass.set_bind_group(0, bind_group, &[offset.offset]);
        pass.draw(0..3, 0..1);
        for (stars, skybox) in [(true, false), (false, true)] {
            if skybox && !gpu.skybox_active {
                continue;
            }
            let key = AtmospherePipelineKey {
                stars,
                skybox,
                ..key
            };
            if let Some(pipeline) = gpu
                .pipelines
                .get(&key)
                .and_then(|id| cache.get_render_pipeline(*id))
            {
                pass.set_render_pipeline(pipeline);
                if stars {
                    pass.draw(3..3 + gpu.star_vertex_count, 0..1);
                } else {
                    pass.draw(0..3, 0..1);
                }
            }
        }
        Ok(())
    }
}
