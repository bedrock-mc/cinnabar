use crate::chunk::*;

pub(in crate::chunk) fn drawable_allocation_identity(
    frame_probe: &ActiveFrameProbe,
    entity: Entity,
    allocation: &GpuChunkAllocation,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> Option<FrameAllocationIdentity> {
    if !chunk_tint_identity_is_active(allocation.tint_identity, active_tint_identity) {
        return None;
    }
    let identity = FrameAllocationIdentity {
        entity,
        key: allocation.key,
        generation: allocation.generation,
    };
    frame_probe.accepts(entity, identity).then_some(identity)
}

pub(in crate::chunk) fn prepare_indirect_batch_draws<'a>(
    allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    frame_probe: &ActiveFrameProbe,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> (
    Vec<DrawIndexedIndirectArgs>,
    Vec<(Entity, FrameAllocationIdentity)>,
) {
    let mut commands = Vec::new();
    let mut drawn = Vec::new();
    for (entity, allocation) in allocations {
        let Some(identity) =
            drawable_allocation_identity(frame_probe, entity, allocation, active_tint_identity)
        else {
            continue;
        };
        let Some(command) = indexed_indirect_command(allocation) else {
            continue;
        };
        commands.push(command);
        drawn.push((entity, identity));
    }
    (commands, drawn)
}

pub(in crate::chunk) fn prepare_model_indirect_batch_draws<'a>(
    allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    frame_probe: &ActiveFrameProbe,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> (
    Vec<DrawIndexedIndirectArgs>,
    Vec<(Entity, FrameAllocationIdentity)>,
) {
    let mut commands = Vec::new();
    let mut drawn = Vec::new();
    for (entity, allocation) in allocations {
        let Some(identity) =
            drawable_allocation_identity(frame_probe, entity, allocation, active_tint_identity)
        else {
            continue;
        };
        let Some(command) = model_mdi_draw_command(allocation) else {
            continue;
        };
        commands.push(command);
        drawn.push((entity, identity));
    }
    (commands, drawn)
}

pub(in crate::chunk) fn prepare_depth_liquid_indirect_batch_draws<'a>(
    allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    frame_probe: &ActiveFrameProbe,
    active_tint_identity: ChunkBiomeTintIdentity,
) -> (
    Vec<DrawIndexedIndirectArgs>,
    Vec<(Entity, FrameAllocationIdentity)>,
) {
    let mut commands = Vec::new();
    let mut drawn = Vec::new();
    for (entity, allocation) in allocations {
        let Some(identity) =
            drawable_allocation_identity(frame_probe, entity, allocation, active_tint_identity)
        else {
            continue;
        };
        let Some(command) = depth_liquid_mdi_draw_command(allocation) else {
            continue;
        };
        commands.push(command);
        drawn.push((entity, identity));
    }
    (commands, drawn)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn prepare_chunk_indirect_batches(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    allocations: Query<&GpuChunkAllocation>,
    biome_tints: Res<ChunkBiomeTints>,
    frame_probe: Res<ActiveFrameProbe>,
    mut batches: ResMut<ChunkIndirectBatches>,
    mut model_batches: ResMut<ChunkModelIndirectBatches>,
    mut depth_liquid_batches: ResMut<ChunkDepthLiquidIndirectBatches>,
    mut arena: ResMut<ChunkGpuArena>,
) {
    let mut all_commands = Vec::new();
    for batch in batches.0.values_mut() {
        let (indirect_commands, drawn_allocations) = prepare_indirect_batch_draws(
            batch
                .visible_entities
                .iter()
                .filter_map(|&entity| allocations.get(entity).ok().map(|item| (entity, item))),
            &frame_probe,
            biome_tints.table_identity(),
        );
        batch.drawn_allocations = drawn_allocations;
        batch.indirect_offset = all_commands.len() as u64 * INDEXED_INDIRECT_BYTES;
        let Ok(command_count) = u32::try_from(indirect_commands.len()) else {
            batch.command_count = 0;
            continue;
        };
        batch.command_count = command_count;
        all_commands.extend(indirect_commands);
    }
    for batch in model_batches.0.values_mut() {
        let (indirect_commands, drawn_allocations) = prepare_model_indirect_batch_draws(
            batch
                .visible_entities
                .iter()
                .filter_map(|&entity| allocations.get(entity).ok().map(|item| (entity, item))),
            &frame_probe,
            biome_tints.table_identity(),
        );
        batch.drawn_allocations = drawn_allocations;
        batch.indirect_offset = all_commands.len() as u64 * INDEXED_INDIRECT_BYTES;
        let Ok(command_count) = u32::try_from(indirect_commands.len()) else {
            batch.command_count = 0;
            continue;
        };
        batch.command_count = command_count;
        all_commands.extend(indirect_commands);
    }
    for batch in depth_liquid_batches.0.values_mut() {
        let (indirect_commands, drawn_allocations) = prepare_depth_liquid_indirect_batch_draws(
            batch
                .visible_entities
                .iter()
                .filter_map(|&entity| allocations.get(entity).ok().map(|item| (entity, item))),
            &frame_probe,
            biome_tints.table_identity(),
        );
        batch.drawn_allocations = drawn_allocations;
        batch.indirect_offset = all_commands.len() as u64 * INDEXED_INDIRECT_BYTES;
        let Ok(command_count) = u32::try_from(indirect_commands.len()) else {
            batch.command_count = 0;
            continue;
        };
        batch.command_count = command_count;
        all_commands.extend(indirect_commands);
    }

    if all_commands.is_empty() {
        return;
    }
    if all_commands.len() > arena.indirect_capacity {
        arena.indirect_capacity = all_commands.len().next_power_of_two();
        arena.indirect_buffer = create_indirect_buffer(&render_device, arena.indirect_capacity);
    }
    render_queue.write_buffer(
        &arena.indirect_buffer,
        0,
        bytemuck::cast_slice(&all_commands),
    );
}

pub(in crate::chunk) fn sorted_visible_entities<T>(
    visible: impl IntoIterator<Item = (Entity, T)>,
) -> Vec<(Entity, T)> {
    let mut visible = visible.into_iter().collect::<Vec<_>>();
    visible.sort_by_key(|(render_entity, _)| *render_entity);
    visible
}

pub(in crate::chunk) type DrawChunkCommands = (SetItemPipeline, DrawPackedChunk);
pub(in crate::chunk) type DrawChunkIndirectCommands = (SetItemPipeline, DrawPackedChunksIndirect);
pub(in crate::chunk) type DrawModelCommands = (SetItemPipeline, DrawPackedModel);
pub(in crate::chunk) type DrawModelIndirectCommands = (SetItemPipeline, DrawPackedModelsIndirect);
pub(in crate::chunk) type DrawTransparentModelCommands =
    (SetItemPipeline, DrawPackedTransparentModel);
pub(in crate::chunk) type DrawDepthLiquidCommands = (SetItemPipeline, DrawDepthLiquid);
pub(in crate::chunk) type DrawDepthLiquidIndirectCommands =
    (SetItemPipeline, DrawDepthLiquidsIndirect);
pub(in crate::chunk) type DrawTransparentLiquidCommands = (SetItemPipeline, DrawTransparentLiquid);
pub(in crate::chunk) type DrawTransparentLiquidIndirectCommands =
    (SetItemPipeline, DrawTransparentLiquidIndirect);
pub(in crate::chunk) type OpaqueChunkViewQuery = (Entity, Read<ViewUniformOffset>);

pub(in crate::chunk) fn record_visibility_direct_submission(
    probe: &ActiveVisibilityFrameProbe,
    view: Entity,
    key: SubChunkKey,
) -> bool {
    probe.record_direct(view, key)
}

pub(in crate::chunk) fn record_visibility_mdi_submissions(
    probe: &ActiveVisibilityFrameProbe,
    view: Entity,
    keys: impl IntoIterator<Item = SubChunkKey>,
) -> usize {
    probe.record_mdi(view, keys)
}

// Both supported paths use `first_instance` to select packed quad records and
// `base_vertex / 4` to select the per-draw origin. Direct drawing is the
// fallback only on adapters that expose BASE_VERTEX.
pub(in crate::chunk) struct DrawPackedChunk;

pub(in crate::chunk) struct DrawTransparentLiquid;

pub(in crate::chunk) struct DrawDepthLiquid;

impl<P: PhaseItem> RenderCommand<P> for DrawDepthLiquid {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(command) = depth_liquid_direct_draw_command(allocation) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            command.first_index..command.first_index + command.index_count,
            command.base_vertex,
            command.first_instance..command.first_instance + command.instance_count,
        );
        frame_probe.record_direct_streams(item.entity(), identity, ChunkStreamMask::LIQUID);
        record_visibility_direct_submission(
            visibility_probe.into_inner(),
            view_entity,
            allocation.key,
        );
        RenderCommandResult::Success
    }
}

impl RenderCommand<Transparent3d> for DrawTransparentLiquid {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<TransparentSortRuntime>,
        SRes<TransparentSortMetrics>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &Transparent3d,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, runtime, metrics, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let runtime = runtime.into_inner();
        if runtime.view_entity != Some(item.entity()) {
            return RenderCommandResult::Skip;
        }
        let (Some(bind_group), Some(snapshot)) = (&arena.bind_group, runtime.state.committed())
        else {
            return RenderCommandResult::Skip;
        };
        let PhaseItemExtraIndex::IndirectParametersIndex {
            range: ref_range, ..
        } = item.extra_index.clone()
        else {
            return RenderCommandResult::Skip;
        };
        let Some(args) = transparent_draw_range_args(snapshot.buffer_slot(), ref_range.clone())
        else {
            return RenderCommandResult::Skip;
        };
        if args.instance_count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            args.first_index..args.first_index + args.index_count,
            args.base_vertex,
            args.first_instance..args.first_instance + args.instance_count,
        );
        let frame_probe = frame_probe.into_inner();
        if frame_probe.is_active()
            && let Some(draw) = transparent_frame_draw_for_range(snapshot, arena, ref_range)
        {
            frame_probe.record_transparent_draw(snapshot.generation(), [draw]);
        }
        let generation = snapshot.generation().get();
        record_encoded_transparent_generation(metrics.into_inner(), ViewSortGeneration(generation));
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) struct DrawTransparentLiquidIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawTransparentLiquidIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<TransparentSortRuntime>,
        SRes<TransparentSortMetrics>,
        SRes<ActiveFrameProbe>,
    );
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, runtime, metrics, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let runtime = runtime.into_inner();
        if runtime.view_entity != Some(item.entity()) {
            return RenderCommandResult::Skip;
        }
        let (Some(bind_group), Some(snapshot)) = (&arena.bind_group, runtime.state.committed())
        else {
            return RenderCommandResult::Skip;
        };
        if snapshot.refs().is_empty() {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed_indirect(&arena.transparent_indirect_buffer, 0);
        let frame_probe = frame_probe.into_inner();
        if frame_probe.is_active() {
            frame_probe.record_transparent_draw(
                snapshot.generation(),
                transparent_frame_draws(snapshot, arena),
            );
        }
        let generation = snapshot.generation().get();
        record_encoded_transparent_generation(metrics.into_inner(), ViewSortGeneration(generation));
        RenderCommandResult::Success
    }
}

impl<P: PhaseItem> RenderCommand<P> for DrawPackedChunk {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(base_vertex) = metadata_base_vertex(allocation.metadata_index) else {
            return RenderCommandResult::Skip;
        };
        let addresses = direct_stream_addresses(allocation);
        if !cube_stream_addresses_valid(&addresses) || !shared_stream_ranges_disjoint(&addresses) {
            return RenderCommandResult::Skip;
        }
        let Some(cube_range) = addresses.cube.as_ref() else {
            return RenderCommandResult::Skip;
        };
        if cube_lighting_record_address(&addresses, cube_range.start).is_none()
            || cube_lighting_record_address(&addresses, cube_range.end.saturating_sub(1)).is_none()
        {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            0..STATIC_QUAD_INDICES.len() as u32,
            base_vertex,
            cube_range.clone(),
        );
        frame_probe.record_direct_draw(item.entity(), identity);
        record_visibility_direct_submission(
            visibility_probe.into_inner(),
            view_entity,
            allocation.key,
        );
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) struct DrawPackedModel;

impl<P: PhaseItem> RenderCommand<P> for DrawPackedModel {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(draw) = model_direct_draw_command(allocation) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            draw.first_index..draw.first_index + draw.index_count,
            draw.base_vertex,
            draw.first_instance..draw.first_instance + draw.instance_count,
        );
        frame_probe.record_direct_streams(item.entity(), identity, ChunkStreamMask::MODEL);
        record_visibility_direct_submission(
            visibility_probe.into_inner(),
            view_entity,
            allocation.key,
        );
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) struct DrawPackedTransparentModel;

impl<P: PhaseItem> RenderCommand<P> for DrawPackedTransparentModel {
    type Param = (SRes<ChunkGpuArena>, SRes<ActiveFrameProbe>);
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = Read<GpuChunkAllocation>;

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        allocation: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, frame_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let frame_probe = frame_probe.into_inner();
        let (Some(bind_group), Some(allocation)) = (&arena.bind_group, allocation) else {
            return RenderCommandResult::Skip;
        };
        let identity = FrameAllocationIdentity {
            entity: item.entity(),
            key: allocation.key,
            generation: allocation.generation,
        };
        if !frame_probe.accepts(item.entity(), identity) {
            return RenderCommandResult::Skip;
        }
        let Some(draw) = transparent_model_direct_draw_command(allocation) else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
        pass.draw_indexed(
            draw.first_index..draw.first_index + draw.index_count,
            draw.base_vertex,
            draw.first_instance..draw.first_instance + draw.instance_count,
        );
        frame_probe.record_direct_streams(item.entity(), identity, ChunkStreamMask::MODEL);
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) struct DrawPackedChunksIndirect;

pub(in crate::chunk) struct DrawPackedModelsIndirect;

pub(in crate::chunk) fn indirect_batch_draw_args(
    batches: &ChunkIndirectBatches,
    item_entity: Entity,
) -> Option<(u64, u32)> {
    let batch = batches.0.get(&item_entity)?;
    (batch.command_count != 0).then_some((batch.indirect_offset, batch.command_count))
}

impl<P: PhaseItem> RenderCommand<P> for DrawPackedChunksIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ChunkIndirectBatches>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let batches = batches.into_inner();
        let frame_probe = frame_probe.into_inner();
        let Some((indirect_offset, command_count)) =
            indirect_batch_draw_args(batches, item.entity())
        else {
            return RenderCommandResult::Skip;
        };
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(&arena.indirect_buffer, indirect_offset, command_count);
        if let Some(batch) = batches.0.get(&item.entity()) {
            frame_probe.record_mdi_draws(batch.drawn_allocations.iter().copied());
            record_visibility_mdi_submissions(
                visibility_probe.into_inner(),
                view_entity,
                batch
                    .drawn_allocations
                    .iter()
                    .map(|(_, identity)| identity.key),
            );
        }
        RenderCommandResult::Success
    }
}

impl<P: PhaseItem> RenderCommand<P> for DrawPackedModelsIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ChunkModelIndirectBatches>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let batches = batches.into_inner();
        let frame_probe = frame_probe.into_inner();
        let Some(batch) = batches.0.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };
        let (indirect_offset, command_count) = (batch.indirect_offset, batch.command_count);
        if command_count == 0 {
            return RenderCommandResult::Skip;
        }
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.model_index_buffer.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(&arena.indirect_buffer, indirect_offset, command_count);
        frame_probe.record_mdi_streams(
            batch.drawn_allocations.iter().copied(),
            ChunkStreamMask::MODEL,
        );
        record_visibility_mdi_submissions(
            visibility_probe.into_inner(),
            view_entity,
            batch
                .drawn_allocations
                .iter()
                .map(|(_, identity)| identity.key),
        );
        RenderCommandResult::Success
    }
}

pub(in crate::chunk) struct DrawDepthLiquidsIndirect;

impl<P: PhaseItem> RenderCommand<P> for DrawDepthLiquidsIndirect {
    type Param = (
        SRes<ChunkGpuArena>,
        SRes<ChunkDepthLiquidIndirectBatches>,
        SRes<ActiveFrameProbe>,
        SRes<ActiveVisibilityFrameProbe>,
    );
    type ViewQuery = OpaqueChunkViewQuery;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        (view_entity, view_offset): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (arena, batches, frame_probe, visibility_probe): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let arena = arena.into_inner();
        let batches = batches.into_inner();
        let frame_probe = frame_probe.into_inner();
        let Some(batch) = batches.0.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };
        if batch.command_count == 0 {
            return RenderCommandResult::Skip;
        }
        let Some(bind_group) = &arena.bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        pass.set_index_buffer(arena.index_buffer.slice(..), IndexFormat::Uint32);
        pass.multi_draw_indexed_indirect(
            &arena.indirect_buffer,
            batch.indirect_offset,
            batch.command_count,
        );
        frame_probe.record_mdi_streams(
            batch.drawn_allocations.iter().copied(),
            ChunkStreamMask::LIQUID,
        );
        record_visibility_mdi_submissions(
            visibility_probe.into_inner(),
            view_entity,
            batch
                .drawn_allocations
                .iter()
                .map(|(_, identity)| identity.key),
        );
        RenderCommandResult::Success
    }
}
