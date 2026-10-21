//! The ordered sky pass owns colour initialization before any world geometry.

use super::{AtmosphereGpu, AtmospherePipelineKey};
use bevy::prelude::IntoScheduleConfigs;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{Core3d, Core3dSystems},
    prelude::{Msaa, Resource, World},
    render::{
        camera::ExtractedCamera,
        render_resource::{PipelineCache, RenderPassDescriptor, StoreOp},
        renderer::{RenderContext, ViewQuery},
        view::{ViewDepthStencilTexture, ViewTarget, ViewUniformOffset},
    },
};

#[derive(Resource)]
struct SkyPassInstalled;

/// Places all sky layers after prepasses and before ordinary or direct terrain.
pub(super) fn install(world: &mut World) {
    if world.contains_resource::<SkyPassInstalled>() {
        return;
    }
    if world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    atmosphere,
                    Some(render_scene::RuntimeStage::GpuSky),
                    "AtmospherePass",
                )
                .after(Core3dSystems::Prepass)
                .before(Core3dSystems::MainPass),
            );
        })
        .is_ok()
    {
        world.insert_resource(SkyPassInstalled);
    }
}

type AtmosphereQuery = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    Option<&'static crate::scene_target::SceneTarget>,
    &'static ViewDepthStencilTexture,
    &'static ViewUniformOffset,
    &'static Msaa,
    Option<&'static MainPassResolutionOverride>,
);

/// Draws the stock sky, stars and finite pack cube before world geometry can cover them,
/// except under a shader pack that draws the view's sky and world itself.
fn atmosphere(world: &World, query: ViewQuery<AtmosphereQuery>, mut context: RenderContext) {
    #[cfg(feature = "shaderpacks")]
    let view = query.entity();
    let (camera, target, scene, depth, offset, msaa, resolution) = query.into_inner();
    if world
        .get_resource::<render_scene::PanoramaScene>()
        .is_some_and(|scene| !scene.game_visible())
    {
        return;
    }
    #[cfg(feature = "shaderpacks")]
    if crate::shaderpack::plugin::draws_world(world, view) {
        return;
    }
    let Some(gpu) = world.get_resource::<AtmosphereGpu>() else {
        return;
    };
    let Some(bind_group) = gpu.bind_group.as_ref() else {
        return;
    };
    let cache = world.resource::<PipelineCache>();
    let key = AtmospherePipelineKey {
        msaa: *msaa,
        hdr: camera.hdr,
        stars: false,
        skybox: false,
    };
    let Some(stock) = gpu
        .pipelines
        .get(&key)
        .and_then(|id| cache.get_render_pipeline(*id))
    else {
        return;
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
            render_scene::RuntimeStage::GpuSky,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    match viewport {
        Some(Some(viewport)) => pass.set_camera_viewport(&viewport),
        Some(None) => return,
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
}
