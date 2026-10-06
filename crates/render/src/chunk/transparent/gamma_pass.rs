//! Ordinary world transparency blends encoded colour, not linear colour.
//!
//! Vanilla selects UNORM format 0x57, while
//! the renderer uses that format for the colour attachment. The near-version ordinary
//! RenderChunk/Transparent Metal fragment writes gamma RGB without a transfer.
//! Opaque sRGB-target bytes already have that encoding: copy them unchanged into
//! a UNORM scratch target and preserve the globally sorted transparent phase.
//! HDR/Enhanced and MSAA retain their previous path; their parity is incomplete.

mod target;
#[cfg(test)]
mod tests;

use crate::chunk::*;
use bevy::{
    camera::{CameraMainTextureUsages, MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::{
        MainTransparentPass3dNode,
        graph::{Core3d, Node3d},
    },
    ecs::query::QueryItem,
    render::{
        render_graph::{NodeRunError, RenderGraph, RenderGraphContext, ViewNode, ViewNodeRunner},
        render_phase::DrawFunctionId,
        render_resource::{
            LoadOp, Operations, RenderPassColorAttachment, RenderPassDescriptor, StoreOp,
        },
        renderer::RenderContext,
        view::ViewDepthTexture,
    },
};
use target::{GammaTarget, prepare_gamma_targets};

pub(crate) fn admitted(hdr: bool, msaa: Msaa, enhanced: bool) -> bool {
    !hdr && msaa == Msaa::Off && !(render_model::ENHANCED_RENDERING_ENABLED && enhanced)
}

pub(in crate::chunk) fn install(app: &mut App) {
    app.add_systems(Last, admit_copy_destination);
    app.sub_app_mut(RenderApp).add_systems(
        Render,
        prepare_gamma_targets
            .in_set(RenderSystems::PrepareResources)
            .after(bevy::render::view::prepare_view_targets),
    );
}

pub(in crate::chunk) fn install_graph(world: &mut World) {
    let runner = ViewNodeRunner::new(GammaTransparentPass, world);
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

fn admit_copy_destination(mut cameras: Query<&mut CameraMainTextureUsages, With<Camera3d>>) {
    for mut usages in &mut cameras {
        usages.0 |= TextureUsages::COPY_DST;
    }
}

#[derive(Default)]
struct GammaTransparentPass;

type GammaView = (
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    &'static ViewDepthTexture,
    Option<&'static MainPassResolutionOverride>,
    &'static Msaa,
    Option<&'static crate::EnhancedRendering>,
    Option<&'static GammaTarget>,
);

impl ViewNode for GammaTransparentPass {
    type ViewQuery = GammaView;

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (camera, view, target, depth, resolution, msaa, enhanced, scratch): QueryItem<
            'w,
            '_,
            GammaView,
        >,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        if !admitted(view.hdr, *msaa, enhanced.is_some()) {
            return MainTransparentPass3dNode.run(
                graph,
                render_context,
                (camera, view, target, depth, resolution),
                world,
            );
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
        let draws = native_draws(world);
        if !phase
            .items
            .iter()
            .any(|item| draws.contains(&Some(item.draw_function())))
        {
            return MainTransparentPass3dNode.run(
                graph,
                render_context,
                (camera, view, target, depth, resolution),
                world,
            );
        }
        // Native pipelines require UNORM. Missing admission must not accidentally
        // submit them to the ordinary sRGB attachment with incompatible format.
        let scratch = scratch.expect("ordinary gamma transparent target must be prepared");
        assert!(
            target
                .main_texture()
                .usage()
                .contains(TextureUsages::COPY_DST)
        );
        copy_scene(render_context, target.main_texture(), &scratch.texture);
        for (range, gamma) in contiguous_ranges(&phase.items, |item| {
            draws.contains(&Some(item.draw_function()))
        }) {
            let colour_view = if gamma {
                &scratch.gamma_view
            } else {
                &scratch.srgb_view
            };
            let colour = RenderPassColorAttachment {
                view: colour_view,
                depth_slice: None,
                resolve_target: None,
                ops: Operations {
                    load: LoadOp::Load,
                    store: StoreOp::Store,
                },
            };
            let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("sorted ordinary transparent colour-space range"),
                color_attachments: &[Some(colour)],
                depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            if let Some(viewport) =
                Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution)
            {
                pass.set_camera_viewport(&viewport);
            }
            phase.render_range(&mut pass, world, graph.view_entity(), range)?;
        }
        copy_scene(render_context, &scratch.texture, target.main_texture());
        Ok(())
    }
}

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

fn copy_scene(context: &mut RenderContext, source: &Texture, destination: &Texture) {
    context.command_encoder().copy_texture_to_texture(
        source.as_image_copy(),
        destination.as_image_copy(),
        source.size(),
    );
}

// No auxiliary per-item storage: ranges retain sort order and never cross draw
// colour-space boundaries. Bevy batches only identical draw-function families.
fn contiguous_ranges<'a, T>(
    items: &'a [T],
    mut gamma: impl FnMut(&T) -> bool + 'a,
) -> impl Iterator<Item = (Range<usize>, bool)> + 'a {
    let mut start = 0;
    std::iter::from_fn(move || {
        let first = items.get(start)?;
        let mode = gamma(first);
        let mut end = start + 1;
        while end < items.len() && gamma(&items[end]) == mode {
            end += 1;
        }
        let range = start..end;
        start = end;
        Some((range, mode))
    })
}
