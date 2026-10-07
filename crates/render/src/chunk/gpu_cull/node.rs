//! Graph nodes that run the early and late cull, and the count-driven terrain draws.

use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    render::{
        render_graph::{NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode},
        render_phase::DrawFunctionId,
        render_resource::{RenderPassDescriptor, StoreOp},
        renderer::RenderContext,
        view::ViewDepthTexture,
    },
};

use super::{
    GpuCull, GpuCullFrame,
    model::{CullPhase, CullStream, args_region, count_index},
};
use crate::chunk::*;

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
        let Some((args, counts, capacity)) = cull
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
        let indices = match stream {
            CullStream::Model => &arena.model_index_buffer,
            _ => &arena.index_buffer,
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect_count(
            args,
            u64::from(args_region(capacity, phase, stream)) * 4,
            counts,
            u64::from(count_index(phase, stream)) * 4,
            capacity * stream.draws_per_record(),
        );
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
        cull.kernels.encode_cull(
            render_context.command_encoder(),
            &groups[0],
            cull.slot_count(),
        );
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
        &'static ViewDepthTexture,
        Option<&'static MainPassResolutionOverride>,
    );

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        render_context: &mut RenderContext<'w>,
        (camera, target, depth, resolution_override): (
            &'w ExtractedCamera,
            &'w ViewTarget,
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
        let encoder = render_context.command_encoder();
        if let Some(prepared) = &cull.pyramid {
            cull.kernels
                .encode_pyramid(encoder, &prepared.pyramid, &prepared.bindings);
        }
        cull.kernels
            .encode_cull(encoder, &groups[1], cull.slot_count());

        let mut pass = render_context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("terrain late cull pass"),
            color_attachments: &[Some(target.get_color_attachment())],
            depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuTerrainOpaque,
            ),
            occlusion_query_set: None,
        });
        if let Some(viewport) =
            Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override)
        {
            pass.set_camera_viewport(&viewport);
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
        Ok(())
    }
}
