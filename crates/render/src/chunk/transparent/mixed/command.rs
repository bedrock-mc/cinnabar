use super::plan::MixedStream;
use super::*;
use crate::chunk::transparent::liquid::transparent_frame_draw_for_range;

pub(in crate::chunk) type DrawMixedTerrainCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuTerrainTransparent as usize },
    (
        crate::lighting::SetWorldLightmap,
        crate::enhanced::SetEnhancedViewBindGroup<2>,
        DrawMixedTerrain,
    ),
>;

pub(in crate::chunk) struct DrawMixedTerrain;

impl RenderCommand<Transparent3d> for DrawMixedTerrain {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<PipelineCache>,
        SRes<MixedTerrainRuntime>,
        SRes<TransparentSortRuntime>,
        SRes<TransparentModelSortRuntime>,
        SRes<TransparentSortMetrics>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &Transparent3d,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, pipelines, mixed, water, models, metrics, frame_probe): SystemParamItem<
            'w,
            '_,
            Self::Param,
        >,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (arena, pipelines, mixed, water, models, frame_probe) = (
            arena.into_inner(),
            pipelines.into_inner(),
            mixed.into_inner(),
            water.into_inner(),
            models.into_inner(),
            frame_probe.into_inner(),
        );
        let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = &item.extra_index else {
            return RenderCommandResult::Skip;
        };
        if range.end.checked_sub(range.start) != Some(1) {
            return RenderCommandResult::Skip;
        }
        let (Some(draw), Some(allocation), Some(snapshot), Some(bind_group)) = (
            mixed.frame.get(range.start as usize),
            allocation,
            water.state.committed(),
            &arena.bind_group,
        ) else {
            return RenderCommandResult::Skip;
        };
        let identity = &draw.identity;
        if draw.view_entity != view_entity
            || water.view_entity != Some(view_entity)
            || identity.model.entity != item.entity()
            || identity.model.generation != allocation.generation
            || identity.model.key != allocation.key
            || allocation.model_range.as_ref() != Some(&identity.model.model_range)
            || allocation.transparent_model_draw_range.as_ref() != Some(&identity.model.draw_range)
            || snapshot.generation() != draw.water_generation
            || snapshot.buffer_slot() != draw.water_slot
            || snapshot.key.asset_identity != identity.asset_identity
            || snapshot.key.tint_identity != identity.tint_identity
            || allocation.tint_identity != identity.tint_identity
            || models
                .draw_orders
                .get(&identity.model)
                .is_none_or(|order| order.revision != identity.model_revision)
        {
            return RenderCommandResult::Skip;
        }
        let frame_identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), frame_identity) {
            return RenderCommandResult::Skip;
        }
        let (Some(pipeline), Some(model_args)) = (
            pipelines.get_render_pipeline(draw.pipeline),
            transparent_model_direct_draw_command(allocation),
        ) else {
            return RenderCommandResult::Skip;
        };
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        for segment in draw.segments.iter() {
            match segment.stream {
                MixedStream::Model => {
                    pass.draw_indexed(
                        model_args.first_index..model_args.first_index + model_args.index_count,
                        model_args.base_vertex,
                        model_args.first_instance + segment.range.start
                            ..model_args.first_instance + segment.range.end,
                    );
                }
                MixedStream::Water => {
                    let start = draw.water_range.start;
                    let Some(args) = transparent_draw_range_args(
                        draw.water_slot,
                        arena.transparent_slot_refs,
                        start + segment.range.start..start + segment.range.end,
                    ) else {
                        return RenderCommandResult::Skip;
                    };
                    pass.draw_indexed(
                        args.first_index..args.first_index + args.index_count,
                        args.base_vertex,
                        args.first_instance..args.first_instance + args.instance_count,
                    );
                }
            }
        }
        frame_probe.record_direct_streams(item.entity(), frame_identity, ChunkStreamMask::MODEL);
        if frame_probe.is_active()
            && let Some(water_draw) =
                transparent_frame_draw_for_range(snapshot, arena, draw.water_range.clone())
        {
            frame_probe.record_transparent_draw(snapshot.generation(), [water_draw]);
        }
        record_encoded_transparent_generation(metrics.into_inner(), snapshot.generation());
        RenderCommandResult::Success
    }
}
