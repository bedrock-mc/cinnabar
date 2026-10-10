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
        render_phase::{DrawFunctionId, TrackedRenderPass},
        render_resource::{CommandEncoderDescriptor, RenderPassDescriptor, StoreOp},
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
        let nametags_only = self.nametags_only;
        // Each drawn range of equal colour space, in sorted order.
        let ranges = move || {
            contiguous_ranges(&phase.items, move |item| {
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
            })
            .filter(move |(_, (_, deferred))| *deferred == nametags_only)
            .map(|(range, (gamma, _))| (range, gamma))
        };
        #[cfg(feature = "tracy")]
        if !nametags_only && let Some(client) = tracy_client::Client::running() {
            use tracy_client::plot_name;
            let mut counts = [0_usize; 3];
            for (range, gamma) in ranges() {
                counts[usize::from(gamma)] += range.len();
                counts[2] += 1;
            }
            let switches = phase
                .items
                .windows(2)
                .filter(|pair| pair[0].pipeline != pair[1].pipeline)
                .count();
            client.plot(plot_name!("transparent linear items"), counts[0] as f64);
            client.plot(plot_name!("transparent gamma items"), counts[1] as f64);
            client.plot(plot_name!("transparent passes"), counts[2] as f64);
            client.plot(plot_name!("transparent pipeline switches"), switches as f64);
        }
        let view_entity = graph.view_entity();
        // `None` keeps the full target; `Some(None)` is a camera viewport outside the attachment.
        let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
            .map(|viewport| {
                crate::render_bounds::viewport(
                    &viewport,
                    crate::render_bounds::extent(scene.color_view(false)),
                )
            });
        // Each range is its own pass, so each encodes on its own task; attachments are taken
        // here in graph order, as only a target's first use may clear it.
        for (range, gamma) in ranges() {
            let colour = scene.color_attachment(target, gamma);
            let depth = depth.get_attachment(StoreOp::Store);
            let task_viewport = viewport.clone();
            render_context.add_command_buffer_generation_task(move |device| {
                let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                    label: Some("sorted ordinary transparency"),
                });
                let pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("sorted ordinary transparent colour-space range"),
                    color_attachments: &[Some(colour)],
                    depth_stencil_attachment: Some(depth),
                    timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                        world,
                        crate::RuntimeStage::GpuTransparent,
                    ),
                    occlusion_query_set: None,
                });
                let mut pass = TrackedRenderPass::new(&device, pass);
                let draw = match &task_viewport {
                    Some(Some(viewport)) => {
                        pass.set_camera_viewport(viewport);
                        true
                    }
                    Some(None) => false,
                    None => true,
                };
                if draw && let Err(error) = phase.render_range(&mut pass, world, view_entity, range)
                {
                    bevy::log::error!("Error rendering sorted transparency: {error:?}");
                }
                drop(pass);
                encoder.finish()
            });
            // An empty clamped viewport draws nothing; like a single pass, only the first range
            // begins, so a target's first use still clears it.
            if matches!(viewport, Some(None)) {
                break;
            }
        }
        Ok(())
    }
}

/// Native transparent families output encoded colour and blend through the compatible UNORM view.
fn native_draws(world: &World) -> [Option<DrawFunctionId>; 8] {
    use crate::chunk::transparent::mixed::DrawMixedTerrainCommands;
    let nametags = crate::nametag_render::draw_function(world);
    let primitives = crate::primitive_shapes::draw_function(world);
    let draws = world.resource::<DrawFunctions<Transparent3d>>().read();
    [
        Some(draws.id::<DrawTransparentLiquidCommands>()),
        Some(draws.id::<DrawTransparentLiquidDirectCommands>()),
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
