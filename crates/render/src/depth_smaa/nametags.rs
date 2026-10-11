//! World name text retains its colour space and depth policy after spatial edge smoothing.

use super::*;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::Transparent3d,
    render::{
        camera::ExtractedCamera,
        render_phase::{PhaseItem, ViewSortedRenderPhases},
        renderer::RenderContext,
        view::ExtractedView,
    },
};

#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct NametagsAfterSmaaLabel;

type NametagsAfterSmaaQuery = (
    &'static Smaa,
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    &'static ViewDepthTexture,
    &'static Msaa,
    &'static crate::scene_target::SceneTarget,
    Option<&'static MainPassResolutionOverride>,
    Option<&'static crate::EnhancedRendering>,
);

/// Draws world names after spatial filtering unless motion blur owns their final pass.
pub(crate) fn nametags_after_smaa(
    world: &World,
    query: bevy::render::renderer::ViewQuery<NametagsAfterSmaaQuery>,
    mut context: RenderContext,
) -> bevy::ecs::error::Result {
    let view_entity = query.entity();
    let (_, camera, view, target, depth, msaa, scene, resolution, enhanced) = query.into_inner();
    let context = &mut context;
    if crate::motion_blur::applies(world, view_entity) {
        return Ok(());
    }
    let Some(draw) = crate::nametag_render::draw_function(world) else {
        return Ok(());
    };
    let Some(phases) = world.get_resource::<ViewSortedRenderPhases<Transparent3d>>() else {
        return Ok(());
    };
    let Some(phase) = phases.get(&view.retained_view_entity) else {
        return Ok(());
    };
    let gamma =
        crate::chunk::transparent::gamma_pass::admitted(camera.hdr, *msaa, enhanced.is_some());
    for (range, deferred) in
        crate::chunk::transparent::gamma_pass::contiguous_ranges(phase.items.values(), |item| {
            crate::nametag_render::deferred_by_world_filter(true, Some(draw), item.draw_function())
        })
    {
        if !deferred {
            continue;
        }
        let attachments = [Some(scene.color_attachment(target, gamma))];
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("world text after spatial anti-aliasing"),
            color_attachments: &attachments,
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuUi,
            ),
            occlusion_query_set: None,
            multiview_mask: None,
        });
        if let Some(viewport) =
            Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
        {
            let Some(viewport) = crate::render_bounds::viewport(
                &viewport,
                crate::render_bounds::extent(scene.color_view(false)),
            ) else {
                return Ok(());
            };
            pass.set_camera_viewport(&viewport);
        }
        phase.render_range(&mut pass, world, view_entity, range)?;
    }
    Ok(())
}
