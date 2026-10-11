use super::SceneTarget;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::{AlphaMask3d, Opaque3d},
    prelude::*,
    render::{
        camera::ExtractedCamera,
        diagnostic::RecordDiagnostics,
        render_phase::ViewBinnedRenderPhases,
        render_resource::{RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};

type SceneOpaqueQuery = (
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    Option<&'static SceneTarget>,
    &'static ViewDepthTexture,
    Option<&'static MainPassResolutionOverride>,
    Option<&'static bevy::core_pipeline::skybox::SkyboxPipelineId>,
    Option<&'static bevy::core_pipeline::skybox::SkyboxBindGroup>,
    Option<&'static bevy::render::view::ViewUniformOffset>,
);

/// Draws opaque and cutout geometry with the current camera's colour and depth attachments.
pub(crate) fn scene_opaque(
    world: &World,
    query: bevy::render::renderer::ViewQuery<SceneOpaqueQuery>,
    mut context: RenderContext,
) {
    let entity = query.entity();
    let (camera, view, target, scene, depth, resolution, sky_pipeline, sky_group, offset) =
        query.into_inner();
    let (Some(opaque), Some(cutout)) = (
        world.get_resource::<ViewBinnedRenderPhases<Opaque3d>>(),
        world.get_resource::<ViewBinnedRenderPhases<AlphaMask3d>>(),
    ) else {
        return;
    };
    let (Some(opaque), Some(cutout)) = (
        opaque.get(&view.retained_view_entity),
        cutout.get(&view.retained_view_entity),
    ) else {
        return;
    };
    let color = scene.map_or_else(
        || target.get_color_attachment(),
        |scene| scene.color_attachment(target, false),
    );
    let depth = depth.get_attachment(StoreOp::Store);
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution).map(
        |viewport| {
            crate::render_bounds::viewport(&viewport, crate::render_bounds::extent(color.view))
        },
    );
    let sky = scene.is_none() && (sky_pipeline.is_some() || sky_group.is_some());
    if crate::gpu_timing::draw_categories(
        world,
        &mut context,
        entity,
        opaque,
        cutout,
        sky,
        color.clone(),
        depth.clone(),
        viewport.clone(),
    ) {
        return;
    }
    let diagnostics = context.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let colors = [Some(color)];
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("main opaque and cutout scene"),
        color_attachments: &colors,
        depth_stencil_attachment: Some(depth),
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            crate::RuntimeStage::GpuOpaque,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    let span = diagnostics.pass_span(&mut pass, "main_opaque_pass_3d");
    match &viewport {
        Some(Some(viewport)) => pass.set_camera_viewport(viewport),
        Some(None) => {
            span.end(&mut pass);
            return;
        }
        None => {}
    }
    if !opaque.is_empty()
        && let Err(error) = opaque.render(&mut pass, world, entity)
    {
        bevy::log::error!("Error rendering the opaque scene: {error:?}");
    }
    if !cutout.is_empty()
        && let Err(error) = cutout.render(&mut pass, world, entity)
    {
        bevy::log::error!("Error rendering the cutout scene: {error:?}");
    }
    if scene.is_none()
        && let (Some(pipeline), Some(group), Some(offset)) = (sky_pipeline, sky_group, offset)
        && let Some(pipeline) = world
            .resource::<bevy::render::render_resource::PipelineCache>()
            .get_render_pipeline(pipeline.0)
    {
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, &group.0.0, &[offset.offset, group.0.1]);
        pass.draw(0..3, 0..1);
    }
    span.end(&mut pass);
}

type SceneFinishQuery = (&'static ViewTarget, &'static SceneTarget);

/// Resolves the final scene samples before single-sample post-processing.
pub(crate) fn scene_finish(
    world: &World,
    query: bevy::render::renderer::ViewQuery<SceneFinishQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let (target, scene) = query.into_inner();
    let context = &mut context;
    scene.finish(context, world, target);
    Ok(())
}
