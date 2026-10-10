use super::{CameraMotionBlur, prepare::BlurView};
use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::query::QueryItem,
    prelude::*,
    render::{
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::*,
        renderer::RenderContext,
        view::{ViewDepthTexture, ViewTarget},
    },
};

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct MotionBlurLabel;
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(super) struct SharpNametagsLabel;

/// Removes nodes entirely when no view requests exposure; unchanged graphs do no work.
pub(crate) fn sync_graph(
    world: &mut World,
    mut views: Local<Option<QueryState<&CameraMotionBlur>>>,
) {
    let enabled = views
        .get_or_insert_with(|| world.query())
        .iter(world)
        .next()
        .is_some();
    configure_graph(world, enabled);
}

pub(super) fn configure_graph(world: &mut World, enabled: bool) {
    let Some(graph) = world
        .get_resource::<RenderGraph>()
        .and_then(|g| g.get_sub_graph(Core3d))
    else {
        return;
    };
    let installed = graph.get_node_state(MotionBlurLabel).is_ok();
    if installed == enabled {
        return;
    }
    if !enabled {
        let graph = world
            .resource_mut::<RenderGraph>()
            .into_inner()
            .get_sub_graph_mut(Core3d)
            .unwrap();
        let _ = graph.remove_node(MotionBlurLabel);
        let _ = graph.remove_node(SharpNametagsLabel);
        return;
    }
    if graph
        .get_node_state(crate::ui_render::UiWorldLabel)
        .is_err()
    {
        return;
    }
    let blur = ViewNodeRunner::new(MotionBlurNode, world);
    let tags = ViewNodeRunner::new(
        crate::chunk::transparent::gamma_pass::GammaTransparentPass {
            nametags_only: true,
        },
        world,
    );
    let graph = world
        .resource_mut::<RenderGraph>()
        .into_inner()
        .get_sub_graph_mut(Core3d)
        .unwrap();
    graph.add_node(MotionBlurLabel, blur);
    graph.add_node(SharpNametagsLabel, tags);
    graph.add_node_edges((
        Node3d::MainTransparentPass,
        MotionBlurLabel,
        SharpNametagsLabel,
        crate::ui_render::UiWorldLabel,
    ));
}

struct MotionBlurNode;
impl ViewNode for MotionBlurNode {
    type ViewQuery = (
        &'static CameraMotionBlur,
        &'static BlurView,
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static ViewDepthTexture,
    );

    fn run(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext,
        (_, state, target, scene, depth): QueryItem<Self::ViewQuery>,
        world: &World,
    ) -> Result<(), NodeRunError> {
        if !state.active {
            return Ok(());
        }
        let cache = world.resource::<PipelineCache>();
        let Some(pipeline) = cache.get_render_pipeline(state.pipeline) else {
            return Ok(());
        };
        let Some(binding) = state.binding(target.main_texture_view().id(), depth.view().id())
        else {
            return Ok(());
        };
        // Resolve only world colour, then write exposure back before sharp projected UI and hands.
        if scene.texture.sample_count() == 1 {
            context.command_encoder().copy_texture_to_texture(
                scene.texture.as_image_copy(),
                target.main_texture().as_image_copy(),
                scene.texture.size(),
            );
        } else {
            let attachments = [Some(
                scene.resolve_attachment(target.main_texture_view(), StoreOp::Store),
            )];
            context
                .command_encoder()
                .begin_render_pass(&RenderPassDescriptor {
                    label: Some("camera exposure scene resolve"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                        world,
                        crate::RuntimeStage::GpuPost,
                    ),
                    occlusion_query_set: None,
                });
        }
        let attachments = [Some(scene.color_attachment(target, false))];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("camera motion blur"),
            color_attachments: &attachments,
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuPost,
            ),
            occlusion_query_set: None,
        });
        let viewport = state.viewport();
        let Some(rect) = crate::render_bounds::scissor(
            render_model::UiScissor::new(viewport.x, viewport.y, viewport.z, viewport.w),
            crate::render_bounds::extent(scene.color_view(false)),
        ) else {
            return Ok(());
        };
        pass.set_viewport(
            rect.x as f32,
            rect.y as f32,
            rect.width as f32,
            rect.height as f32,
            0.0,
            1.0,
        );
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, binding, &[]);
        pass.draw(0..3, 0..1);
        Ok(())
    }
}
