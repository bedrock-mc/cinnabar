//! Ordinary world transparency blends encoded colour while retaining sorted draw order.

#[cfg(test)]
mod tests;

use crate::chunk::*;
use crate::scene_target::SceneTarget;
use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::query::QueryItem,
    render::{
        render_graph::{NodeRunError, RenderGraph, RenderGraphContext, ViewNode, ViewNodeRunner},
        render_phase::DrawFunctionId,
        render_resource::{RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::ViewDepthTexture,
    },
};

/// Encoded blending applies to ordinary LDR views at every sample count.
pub(crate) fn admitted(hdr: bool, _msaa: Msaa, enhanced: bool) -> bool {
    !(hdr || render_model::enhanced_rendering_enabled() && enhanced)
}

/// Shares the main colour attachment with opaque geometry and later hand passes.
pub(in crate::chunk) fn install(app: &mut App) {
    crate::scene_target::install(app);
}

/// Preserves graph dependencies while selecting the blend colour space per sorted range.
pub(in crate::chunk) fn install_graph(world: &mut World) {
    crate::scene_target::install_graph(world);
    let runner = ViewNodeRunner::new(GammaTransparentPass::default(), world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    let Ok(node) = graph.get_node_state_mut(Node3d::MainTransparentPass) else {
        return;
    };
    // Replacing the node alone preserves every previously installed graph edge.
    node.node = Box::new(runner);
    node.type_name = std::any::type_name::<ViewNodeRunner<GammaTransparentPass>>();
}

#[derive(Default)]
pub(crate) struct GammaTransparentPass {
    pub(crate) nametags_only: bool,
}

type GammaView = (
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    &'static ViewDepthTexture,
    Option<&'static MainPassResolutionOverride>,
    &'static Msaa,
    Option<&'static crate::EnhancedRendering>,
    &'static SceneTarget,
    Option<&'static bevy::anti_alias::smaa::Smaa>,
);

impl ViewNode for GammaTransparentPass {
    type ViewQuery = GammaView;

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (camera, view, target, depth, resolution, msaa, enhanced, scene, smaa): QueryItem<
            'w,
            '_,
            GammaView,
        >,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let blur = crate::motion_blur::applies(world, graph.view_entity());
        if self.nametags_only && !blur {
            return Ok(());
        }
        let Some(phases) = world.get_resource::<ViewSortedRenderPhases<Transparent3d>>() else {
            return Ok(());
        };
        let Some(phase) = phases.get(&view.retained_view_entity) else {
            return Ok(());
        };
        if phase.items.is_empty() {
            return Ok(());
        }
        let gamma = admitted(view.hdr, *msaa, enhanced.is_some());
        let draws = gamma.then(|| native_draws(world));
        let filtered = blur || smaa.is_some();
        let nametag = filtered
            .then(|| crate::nametag_render::draw_function(world))
            .flatten();
        for (range, (gamma, deferred)) in contiguous_ranges(&phase.items, |item| {
            (
                draws
                    .as_ref()
                    .is_some_and(|draws| draws.contains(&Some(item.draw_function()))),
                crate::nametag_render::deferred_by_world_filter(
                    filtered,
                    nametag,
                    item.draw_function(),
                ),
            )
        }) {
            if deferred != self.nametags_only {
                continue;
            }
            let colour = scene.color_attachment(target, gamma);
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("sorted ordinary transparent colour-space range"),
                color_attachments: &[Some(colour)],
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
                let Some(viewport) = crate::render_bounds::viewport(
                    &viewport,
                    crate::render_bounds::extent(scene.color_view(false)),
                ) else {
                    return Ok(());
                };
                pass.set_camera_viewport(&viewport);
            }
            phase.render_range(&mut pass, world, graph.view_entity(), range)?;
        }
        Ok(())
    }
}

/// Native transparent families output encoded colour and blend through the compatible UNORM view.
fn native_draws(world: &World) -> [Option<DrawFunctionId>; 7] {
    use crate::chunk::transparent::mixed::DrawMixedTerrainCommands;
    let nametags = crate::nametag_render::draw_function(world);
    let primitives = crate::primitive_shapes::draw_function(world);
    let draws = world.resource::<DrawFunctions<Transparent3d>>().read();
    [
        Some(draws.id::<DrawTransparentLiquidCommands>()),
        Some(draws.id::<DrawTransparentLiquidIndirectCommands>()),
        Some(draws.id::<DrawTransparentModelCommands>()),
        Some(draws.id::<DrawMixedTerrainCommands>()),
        draws.get_id::<crate::actor_render::phase::DrawTransparentActorCommands>(),
        nametags,
        primitives,
    ]
}

/// Keeps sorted items contiguous without auxiliary per-item storage or crossing colour spaces.
pub(crate) fn contiguous_ranges<'a, T, M: Copy + PartialEq + 'a>(
    items: &'a [T],
    mut classify: impl FnMut(&T) -> M + 'a,
) -> impl Iterator<Item = (Range<usize>, M)> + 'a {
    let mut start = 0;
    std::iter::from_fn(move || {
        let first = items.get(start)?;
        let mode = classify(first);
        let mut end = start + 1;
        while end < items.len() && classify(&items[end]) == mode {
            end += 1;
        }
        let range = start..end;
        start = end;
        Some((range, mode))
    })
}
