use bevy::{
    prelude::World,
    render::{
        camera::ExtractedCamera,
        render_resource::{PipelineCache, RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::{ViewDepthStencilTexture, ViewTarget, ViewUniformOffset},
    },
};

use super::gpu::FarTerrainView;
use crate::scene_target::SceneTarget;

type FarTerrainQuery = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    &'static SceneTarget,
    &'static ViewDepthStencilTexture,
    &'static ViewUniformOffset,
    &'static FarTerrainView,
);

/// One instanced draw per visible tile; no pass at all when nothing is visible.
pub(super) fn far_terrain(
    world: &World,
    query: bevy::render::renderer::ViewQuery<FarTerrainQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let (camera, target, scene, depth, offset, state) = query.into_inner();
    let (Some((_, bind_group)), Some(pipeline), Some(lightmap)) = (
        state.bind_group.as_ref(),
        world
            .resource::<PipelineCache>()
            .get_render_pipeline(state.pipeline),
        world.get_resource::<crate::lighting::LightmapGpu>(),
    ) else {
        return Ok(());
    };
    if state.visible.is_empty()
        || world
            .get_resource::<render_scene::PanoramaScene>()
            .is_some_and(|panorama| !panorama.game_visible())
    {
        return Ok(());
    }
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("far terrain"),
        color_attachments: &[Some(scene.color_attachment(target, false))],
        depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuFarTerrain,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    if let Some(viewport) = camera.viewport.as_ref() {
        let extent = crate::render_bounds::extent(scene.color_view(false));
        let Some(viewport) = crate::render_bounds::viewport(viewport, extent) else {
            return Ok(());
        };
        pass.set_camera_viewport(&viewport);
    }
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[offset.offset]);
    pass.set_bind_group(1, &lightmap.bind_group, &[]);
    for draw in &state.visible {
        pass.draw(
            draw.quads.start * 6..draw.quads.end * 6,
            draw.slot..draw.slot + 1,
        );
    }
    Ok(())
}
