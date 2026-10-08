//! World name text retains its colour space and depth policy after spatial edge smoothing.

use super::*;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::Transparent3d,
    ecs::query::QueryItem,
    render::{
        camera::ExtractedCamera,
        render_graph::{NodeRunError, RenderGraphContext, ViewNode},
        render_phase::{PhaseItem, ViewSortedRenderPhases},
        renderer::RenderContext,
        view::ExtractedView,
    },
};

#[derive(RenderLabel, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct NametagsAfterSmaaLabel;

pub(super) struct NametagsAfterSmaa;

impl ViewNode for NametagsAfterSmaa {
    type ViewQuery = (
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

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (_, camera, view, target, depth, msaa, scene, resolution, enhanced): QueryItem<
            'w,
            '_,
            Self::ViewQuery,
        >,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
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
            crate::chunk::transparent::gamma_pass::admitted(view.hdr, *msaa, enhanced.is_some());
        for (range, deferred) in
            crate::chunk::transparent::gamma_pass::contiguous_ranges(&phase.items, |item| {
                crate::nametag_render::deferred_by_smaa(true, Some(draw), item.draw_function())
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
            });
            if let Some(viewport) =
                Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
            {
                pass.set_camera_viewport(&viewport);
            }
            phase.render_range(&mut pass, world, graph.view_entity(), range)?;
        }
        Ok(())
    }
}
