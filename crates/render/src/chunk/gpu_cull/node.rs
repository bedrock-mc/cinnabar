//! Graph nodes that run the early and late cull, then submit compacted terrain draws.

use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    render::{
        render_graph::{NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode},
        render_phase::{DrawFunctionId, TrackedRenderPass},
        render_resource::{CommandEncoderDescriptor, RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::ViewDepthTexture,
    },
};

use super::{
    GpuCull, GpuCullFrame, GpuCullSubmission,
    model::{CullPhase, CullStream, args_region, count_index},
};
use crate::chunk::*;
use crate::gpu_timing::{SectionSpan, within_span};

/// Draws one stream's compacted args for one phase of the GPU-culled view.
pub(in crate::chunk) struct DrawGpuCulled<const STREAM: usize, const LATE: bool>;

pub(in crate::chunk) type DrawGpuCulledCommands<const STREAM: usize, const LATE: bool> = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    crate::enhanced::SetEnhancedViewBindGroup<2>,
    DrawGpuCulled<STREAM, LATE>,
);

impl<P: PhaseItem, const STREAM: usize, const LATE: bool> RenderCommand<P>
    for DrawGpuCulled<STREAM, LATE>
{
    type Param = (SRes<ChunkGpuArena>, Option<SRes<GpuCull>>);
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, cull): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let Some(draws) = cull
            .map(|cull| cull.into_inner())
            .and_then(|cull| cull.prepared_draws(view_entity))
        else {
            return RenderCommandResult::Skip;
        };
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        let stream = CullStream::ALL[STREAM];
        let phase = if LATE {
            CullPhase::Late
        } else {
            CullPhase::Early
        };
        let max_draw_count = draws.draw_bounds[stream as usize];
        if max_draw_count == 0 {
            return RenderCommandResult::Skip;
        }
        let indices = match stream {
            CullStream::Model => &arena.model_index_buffer,
            _ => &arena.index_buffer,
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        pass.set_vertex_buffer(0, draws.offsets.slice(..));
        let args_offset = u64::from(args_region(draws.capacity, phase, stream)) * 4;
        match draws.submission {
            GpuCullSubmission::Count => pass.multi_draw_indexed_indirect_count(
                draws.args,
                args_offset,
                draws.counts,
                u64::from(count_index(phase, stream)) * 4,
                max_draw_count,
            ),
            GpuCullSubmission::Fixed => {
                pass.multi_draw_indexed_indirect(draws.args, args_offset, max_draw_count);
            }
        }
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) fn install_commands(render_app: &mut SubApp) {
    render_app
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<0, false>>()
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<1, false>>()
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<2, false>>()
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<3, false>>()
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<0, true>>()
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<1, true>>()
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<2, true>>()
        .add_render_command::<Opaque3d, DrawGpuCulledCommands<3, true>>();
}

/// Draw function ids for every stream of one phase, in [`CullStream::ALL`] order.
pub(in crate::chunk) fn draw_function_ids(
    functions: &bevy::render::render_phase::DrawFunctionsInternal<Opaque3d>,
    late: bool,
) -> [DrawFunctionId; 4] {
    if late {
        [
            functions.id::<DrawGpuCulledCommands<0, true>>(),
            functions.id::<DrawGpuCulledCommands<1, true>>(),
            functions.id::<DrawGpuCulledCommands<2, true>>(),
            functions.id::<DrawGpuCulledCommands<3, true>>(),
        ]
    } else {
        [
            functions.id::<DrawGpuCulledCommands<0, false>>(),
            functions.id::<DrawGpuCulledCommands<1, false>>(),
            functions.id::<DrawGpuCulledCommands<2, false>>(),
            functions.id::<DrawGpuCulledCommands<3, false>>(),
        ]
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(in crate::chunk) struct GpuCullEarlyLabel;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct GpuCullLateLabel;

pub(super) fn install_graph(world: &mut World) {
    let early = bevy::render::render_graph::ViewNodeRunner::new(EarlyCullNode, world);
    let late = bevy::render::render_graph::ViewNodeRunner::new(LateCullNode, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    graph.add_node(GpuCullEarlyLabel, early);
    graph.add_node(GpuCullLateLabel, late);
    graph.add_node_edges((
        Node3d::StartMainPass,
        GpuCullEarlyLabel,
        Node3d::MainOpaquePass,
    ));
    graph.add_node_edges((
        Node3d::MainOpaquePass,
        GpuCullLateLabel,
        Node3d::MainTransmissivePass,
    ));
    // A missing destination would leave a dangling output edge in the graph.
    if graph
        .get_node_state(crate::entity_shadow_render::EntityShadowLabel)
        .is_ok()
    {
        let _ = graph.try_add_node_edge(
            GpuCullLateLabel,
            crate::entity_shadow_render::EntityShadowLabel,
        );
    }
}

/// Culls with last frame's visibility before the main opaque pass draws the result.
#[derive(Default)]
struct EarlyCullNode;

impl ViewNode for EarlyCullNode {
    type ViewQuery = ();

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        _: (),
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let cull = world.resource::<GpuCull>();
        if cull.prepared_view != Some(graph.view_entity()) {
            return Ok(());
        }
        let Some(groups) = &cull.bind_groups else {
            return Ok(());
        };
        let encoder = render_context.command_encoder();
        cull.clear_fixed_args(encoder);
        cull.kernels
            .encode_cull(encoder, &groups[0], cull.slot_count());
        Ok(())
    }
}

/// Builds Hi-Z from the opaque depth, re-tests what the early pass skipped and draws it.
#[derive(Default)]
struct LateCullNode;

impl ViewNode for LateCullNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ViewTarget,
        &'static crate::scene_target::SceneTarget,
        &'static ViewDepthTexture,
        Option<&'static MainPassResolutionOverride>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (camera, target, scene_target, depth, resolution_override): (
            &'w ExtractedCamera,
            &'w ViewTarget,
            &'w crate::scene_target::SceneTarget,
            &'w ViewDepthTexture,
            Option<&'w MainPassResolutionOverride>,
        ),
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let view_entity = graph.view_entity();
        let cull = world.resource::<GpuCull>();
        let frame = world.resource::<GpuCullFrame>();
        let (Some(groups), Some(view)) = (&cull.bind_groups, frame.view) else {
            return Ok(());
        };
        if cull.prepared_view != Some(view_entity) || view.entity != view_entity {
            return Ok(());
        }
        #[cfg(feature = "tracy")]
        if let Some(client) = tracy_client::Client::running() {
            use tracy_client::plot_name;
            let bounds = cull.table.draw_bounds();
            client.plot(plot_name!("cull slots"), f64::from(cull.slot_count()));
            client.plot(plot_name!("cull bound solid"), f64::from(bounds[0]));
            client.plot(plot_name!("cull bound cutout"), f64::from(bounds[1]));
            client.plot(plot_name!("cull bound model"), f64::from(bounds[2]));
            client.plot(plot_name!("cull bound liquid"), f64::from(bounds[3]));
        }
        let pyramid = cull.pyramid.as_ref();
        // Spans and attachments are claimed in graph order; the task only records them.
        let spans = [
            pyramid.and_then(|_| SectionSpan::claim(world, "late cull hi-z pyramid")),
            SectionSpan::claim(world, "late cull dispatch"),
            SectionSpan::claim(world, "late cull draws"),
        ];
        let colour = scene_target.color_attachment(target, false);
        let depth = depth.get_attachment(StoreOp::Store);
        let timestamps =
            crate::gpu_timing::render_pass_timestamps(world, crate::RuntimeStage::GpuTerrainOpaque);
        // `None` keeps the full target; `Some(None)` is a camera viewport outside the attachment,
        // which still builds the pyramid and culls but draws nothing.
        let viewport =
            Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override)
                .map(|viewport| {
                    crate::render_bounds::viewport(
                        &viewport,
                        crate::render_bounds::extent(scene_target.color_view(false)),
                    )
                });
        render_context.add_command_buffer_generation_task(move |device| {
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("terrain late cull"),
            });
            if let Some(prepared) = pyramid {
                within_span(spans[0].as_ref(), &mut encoder, |encoder| {
                    cull.kernels
                        .encode_pyramid(encoder, &prepared.pyramid, &prepared.bindings);
                });
            }
            within_span(spans[1].as_ref(), &mut encoder, |encoder| {
                cull.kernels
                    .encode_cull(encoder, &groups[1], cull.slot_count());
            });
            within_span(spans[2].as_ref(), &mut encoder, |encoder| {
                let pass = encoder.begin_render_pass(&RenderPassDescriptor {
                    label: Some("terrain late cull pass"),
                    color_attachments: &[Some(colour)],
                    depth_stencil_attachment: Some(depth),
                    timestamp_writes: timestamps,
                    occlusion_query_set: None,
                });
                let mut pass = TrackedRenderPass::new(&device, pass);
                match &viewport {
                    Some(Some(viewport)) => pass.set_camera_viewport(viewport),
                    Some(None) => return,
                    None => {}
                }
                let draw_functions = world.resource::<DrawFunctions<Opaque3d>>();
                let mut draw_functions = draw_functions.write();
                draw_functions.prepare(world);
                for (draw_function, pipeline) in view.late_draws.into_iter().zip(view.pipelines) {
                    let item = <Opaque3d as bevy::render::render_phase::BinnedPhaseItem>::new(
                        Opaque3dBatchSetKey {
                            draw_function,
                            pipeline,
                            material_bind_group_index: None,
                            lightmap_slab: None,
                            vertex_slab: default(),
                            index_slab: None,
                        },
                        Opaque3dBinKey {
                            asset_id: AssetId::<Mesh>::invalid().untyped(),
                        },
                        (view_entity, view.main),
                        0..1,
                        PhaseItemExtraIndex::None,
                    );
                    let Some(draw) = draw_functions.get_mut(draw_function) else {
                        continue;
                    };
                    if let Err(error) = draw.draw(world, &mut pass, view_entity, &item) {
                        bevy::log::error!("late terrain cull draw failed: {error:?}");
                    }
                }
            });
            encoder.finish()
        });
        Ok(())
    }
}
