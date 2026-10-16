use super::TerrainUnitBatches;
use crate::chunk::*;

pub(in crate::chunk) type DrawTerrainUnits<const STREAM: usize> = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawUnits<STREAM>,
);

pub(in crate::chunk) type DrawTerrainOccluders = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawUnits<0, true>,
);

pub(in crate::chunk) struct DrawUnits<const STREAM: usize, const OCCLUDERS: bool = false>;

impl<P: PhaseItem, const STREAM: usize, const OCCLUDERS: bool> RenderCommand<P>
    for DrawUnits<STREAM, OCCLUDERS>
{
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<TerrainUnitBatches>,
        Option<SRes<crate::gpu_timing::GpuTimestamps>>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = ();

    fn render<'w>(
        _: &P,
        (entity, offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<()>,
        (arena, batches, timestamps): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (arena, batches) = (arena.into_inner(), batches.into_inner());
        let (Some(group), Some(indices), Some(view)) = (
            &arena.bind_group,
            &batches.index_buffer,
            batches.views.get(&entity),
        ) else {
            return RenderCommandResult::Skip;
        };
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "terrain.unit_draw",
            stream = STREAM,
            draws = view.streams[STREAM].pages.len()
        )
        .entered();
        // Same per-stream span the legacy commands record, so batched terrain stays attributable.
        crate::gpu_timing::time_draw(
            timestamps.map(|timestamps| timestamps.into_inner()),
            super::super::opaque::STREAM_STAGES[STREAM],
            pass,
            move |pass| {
                pass.set_bind_group(0, group, &[offset.offset]);
                pass.set_index_buffer(indices.slice(..), IndexFormat::Uint32);
                for page in if OCCLUDERS {
                    &view.occluders.pages
                } else {
                    &view.streams[STREAM].pages
                } {
                    pass.set_bind_group(2, &page.group, &[]);
                    pass.draw_indexed(
                        0..super::index_count(),
                        0,
                        0..page
                            .count
                            .div_ceil(render_model::terrain_units::UNITS_PER_INSTANCE),
                    );
                }
            },
        );
        RenderCommandResult::Success
    }
}
