use crate::chunk::*;

pub(in crate::chunk) const MODEL_INDEX_COUNT: u32 = 6;

#[derive(Component, Clone)]
pub(in crate::chunk) struct GpuChunkAllocation {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    pub(in crate::chunk) quad_range: Range<u32>,
    pub(in crate::chunk) cube_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_range: Option<Range<u32>>,
    pub(in crate::chunk) model_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) transparent_model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) has_depth_liquid: bool,
    pub(in crate::chunk) has_transparent_liquid: bool,
    pub(in crate::chunk) depth_liquid_range: Option<Range<u32>>,
    pub(in crate::chunk) metadata_index: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::chunk) struct StreamAddresses {
    pub(in crate::chunk) cube: Option<Range<u32>>,
    pub(in crate::chunk) cube_lighting: Option<Range<u32>>,
    pub(in crate::chunk) model: Option<Range<u32>>,
    pub(in crate::chunk) model_lighting: Option<Range<u32>>,
    pub(in crate::chunk) model_draw: Option<Range<u32>>,
    pub(in crate::chunk) transparent_model_draw: Option<Range<u32>>,
    pub(in crate::chunk) liquid: Option<Range<u32>>,
    pub(in crate::chunk) liquid_lighting: Option<Range<u32>>,
}

pub(in crate::chunk) fn direct_stream_addresses(
    allocation: &GpuChunkAllocation,
) -> StreamAddresses {
    StreamAddresses {
        cube: (!allocation.quad_range.is_empty()).then(|| allocation.quad_range.clone()),
        cube_lighting: allocation.cube_lighting_range.clone(),
        model: allocation.model_range.clone(),
        model_lighting: allocation.model_lighting_range.clone(),
        model_draw: allocation.model_draw_range.clone(),
        transparent_model_draw: allocation.transparent_model_draw_range.clone(),
        liquid: allocation.liquid_range.clone(),
        liquid_lighting: allocation.liquid_lighting_range.clone(),
    }
}

pub(in crate::chunk) fn mdi_stream_addresses(allocation: &GpuChunkAllocation) -> StreamAddresses {
    direct_stream_addresses(allocation)
}

pub(in crate::chunk) fn cube_lighting_record_address(
    addresses: &StreamAddresses,
    global_quad: u32,
) -> Option<u32> {
    if !cube_stream_addresses_valid(addresses) {
        return None;
    }
    let cube = addresses.cube.as_ref()?;
    let lighting = addresses.cube_lighting.as_ref()?;
    if global_quad < cube.start || global_quad >= cube.end || !lighting.start.is_multiple_of(2) {
        return None;
    }
    let local = global_quad.checked_sub(cube.start)?;
    let record = lighting.start.checked_div(2)?.checked_add(local)?;
    (record < lighting.end.checked_div(2)?).then_some(record)
}

pub(in crate::chunk) fn cube_stream_addresses_valid(addresses: &StreamAddresses) -> bool {
    match (addresses.cube.as_ref(), addresses.cube_lighting.as_ref()) {
        (None, None) => true,
        (Some(cube), Some(lighting)) => {
            !cube.is_empty()
                && !lighting.is_empty()
                && lighting.start.is_multiple_of(2)
                && lighting.end.is_multiple_of(2)
                && cube.end.checked_sub(cube.start)
                    == lighting
                        .end
                        .checked_sub(lighting.start)
                        .and_then(|words| words.checked_div(2))
        }
        _ => false,
    }
}

pub(in crate::chunk) fn shared_stream_ranges_disjoint(addresses: &StreamAddresses) -> bool {
    let ranges = [
        addresses.model.as_ref(),
        addresses.model_lighting.as_ref(),
        addresses.model_draw.as_ref(),
        addresses.transparent_model_draw.as_ref(),
        addresses.liquid.as_ref(),
        addresses.liquid_lighting.as_ref(),
        addresses.cube_lighting.as_ref(),
    ];
    ranges.iter().enumerate().all(|(index, left)| {
        left.is_none_or(|left| {
            !left.is_empty()
                && ranges[index + 1..]
                    .iter()
                    .flatten()
                    .all(|right| left.end <= right.start || right.end <= left.start)
        })
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) enum ChunkDrawMode {
    Direct,
    MultiDrawIndirect,
    Unsupported,
}

pub(in crate::chunk) fn select_chunk_draw_mode(
    downlevel_flags: DownlevelFlags,
    features: WgpuFeatures,
    is_dx12: bool,
    debug_assertions: bool,
) -> ChunkDrawMode {
    if !downlevel_flags.contains(DownlevelFlags::BASE_VERTEX) {
        ChunkDrawMode::Unsupported
    } else if downlevel_flags.contains(DownlevelFlags::INDIRECT_EXECUTION)
        && features.contains(WgpuFeatures::INDIRECT_FIRST_INSTANCE)
        // wgpu 27's DX12 indirect validator expands each indexed command
        // from 20 to 32 bytes for special constants, but its debug batching
        // assertion still assumes the unexpanded stride. Preserve MDI in
        // release builds and use the equivalent direct path while that
        // validator is active.
        && !(debug_assertions && is_dx12)
    {
        ChunkDrawMode::MultiDrawIndirect
    } else {
        ChunkDrawMode::Direct
    }
}

pub(in crate::chunk) const fn diagnostic_draw_mode(draw_mode: ChunkDrawMode) -> OpaqueDrawMode {
    match draw_mode {
        ChunkDrawMode::Direct => OpaqueDrawMode::Direct,
        ChunkDrawMode::MultiDrawIndirect => OpaqueDrawMode::MultiDrawIndirect,
        ChunkDrawMode::Unsupported => OpaqueDrawMode::Unsupported,
    }
}

pub(in crate::chunk) fn opaque_allocation_is_drawable(allocation: &GpuChunkAllocation) -> bool {
    indexed_indirect_command(allocation).is_some()
        || model_direct_draw_command(allocation).is_some()
        || depth_liquid_direct_draw_command(allocation).is_some()
}

pub(in crate::chunk) fn extracted_camera_identity(
    main_entity: &MainEntity,
    view: &ExtractedView,
) -> ExtractedCameraIdentity {
    let world_from_view = view.world_from_view.to_matrix();
    let clip_from_world = view
        .clip_from_world
        .unwrap_or(view.clip_from_view * world_from_view.inverse());
    ExtractedCameraIdentity {
        stable_id: main_entity.id().to_bits(),
        pose_hash: hash_f32_words(world_from_view.to_cols_array()),
        frustum_hash: hash_f32_words(clip_from_world.to_cols_array()),
    }
}

#[derive(SystemParam)]
pub(in crate::chunk) struct QueueFrameProbeParams<'w> {
    pub(in crate::chunk) frame_probe: Res<'w, ActiveFrameProbe>,
    pub(in crate::chunk) input: Res<'w, VisibilityDiagnosticsInput>,
    pub(in crate::chunk) visibility_probe: Res<'w, ActiveVisibilityFrameProbe>,
    pub(in crate::chunk) camera_identity_tracker: ResMut<'w, ExtractedCameraIdentityTracker>,
}

pub(in crate::chunk) fn adapter_metadata_field(value: String) -> String {
    if value.trim().is_empty() {
        "unavailable".to_owned()
    } else {
        value
    }
}

pub(in crate::chunk) fn resolve_surface_present_mode(
    requested: bevy::window::PresentMode,
    supported: &[wgpu::PresentMode],
) -> Option<wgpu::PresentMode> {
    let fallbacks: &[wgpu::PresentMode] = match requested {
        bevy::window::PresentMode::Fifo => &[wgpu::PresentMode::Fifo],
        bevy::window::PresentMode::Immediate => {
            &[wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo]
        }
        _ => return None,
    };
    fallbacks
        .iter()
        .copied()
        .find(|candidate| supported.contains(candidate))
}

pub(in crate::chunk) fn window_present_mode_name(
    mode: bevy::window::PresentMode,
) -> Option<&'static str> {
    match mode {
        bevy::window::PresentMode::Fifo => Some("Fifo"),
        bevy::window::PresentMode::Immediate => Some("Immediate"),
        _ => None,
    }
}

pub(in crate::chunk) fn surface_present_mode_name(mode: wgpu::PresentMode) -> Option<&'static str> {
    match mode {
        wgpu::PresentMode::Fifo => Some("Fifo"),
        wgpu::PresentMode::Immediate => Some("Immediate"),
        _ => None,
    }
}

pub(in crate::chunk) fn publish_graphics_runtime_metadata(
    #[cfg(any(target_os = "macos", target_os = "ios"))] _marker: bevy::ecs::system::NonSendMarker,
    windows: Res<ExtractedWindows>,
    render_instance: Res<RenderInstance>,
    render_adapter: Res<RenderAdapter>,
    input: Res<VisibilityDiagnosticsInput>,
    diagnostics: Res<VisibilityDiagnostics>,
    mut published: Local<bool>,
) {
    if !input.enabled() || *published {
        return;
    }
    let Some(window) = windows
        .primary
        .and_then(|primary| windows.windows.get(&primary))
    else {
        return;
    };
    let Some(requested_present_mode) = window_present_mode_name(window.present_mode) else {
        return;
    };
    let surface_target = wgpu::SurfaceTargetUnsafe::RawHandle {
        raw_display_handle: window.handle.get_display_handle(),
        raw_window_handle: window.handle.get_window_handle(),
    };
    // SAFETY: This runs on the main thread where required, and the extracted window owns
    // valid raw handles for the same window Bevy configures immediately after this system.
    let Ok(surface) = (unsafe { render_instance.create_surface_unsafe(surface_target) }) else {
        return;
    };
    let capabilities = surface.get_capabilities(&render_adapter);
    let Some(effective_present_mode) =
        resolve_surface_present_mode(window.present_mode, &capabilities.present_modes)
            .and_then(surface_present_mode_name)
    else {
        return;
    };
    let adapter_info = render_adapter.get_info();
    diagnostics.publish_graphics_adapter(GraphicsAdapterMetadata {
        backend: format!("{:?}", adapter_info.backend),
        adapter: adapter_metadata_field(adapter_info.name),
        driver: adapter_metadata_field(adapter_info.driver),
        driver_info: adapter_metadata_field(adapter_info.driver_info),
        requested_present_mode: requested_present_mode.to_owned(),
        effective_present_mode: effective_present_mode.to_owned(),
        present_mode_proven: true,
    });
    *published = true;
}

pub(in crate::chunk) fn indexed_indirect_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    let addresses = mdi_stream_addresses(allocation);
    if !cube_stream_addresses_valid(&addresses) || !shared_stream_ranges_disjoint(&addresses) {
        return None;
    }
    let cube = addresses.cube.as_ref()?;
    let instance_count = cube.end.checked_sub(cube.start)?;
    if instance_count == 0 {
        return None;
    }
    cube_lighting_record_address(&addresses, cube.start)?;
    cube_lighting_record_address(&addresses, cube.end.checked_sub(1)?)?;
    let base_vertex = metadata_base_vertex(allocation.metadata_index)?;
    Some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex,
        first_instance: cube.start,
    })
}

pub(in crate::chunk) fn model_draw_command(
    allocation: &GpuChunkAllocation,
    addresses: StreamAddresses,
) -> Option<DrawIndexedIndirectArgs> {
    let model = addresses.model?;
    let model_lighting = addresses.model_lighting?;
    let model_draw = addresses.model_draw?;
    let expected_draw_start =
        if allocation.transparent_model_draw_range.as_ref() == Some(&model_draw) {
            allocation
                .model_draw_range
                .as_ref()
                .map_or(model_lighting.end, |range| range.end)
        } else {
            model_lighting.end
        };
    if model.is_empty()
        || !model.start.is_multiple_of(4)
        || !model.end.is_multiple_of(4)
        || model_lighting.is_empty()
        || !model_lighting.start.is_multiple_of(2)
        || !model_lighting.end.is_multiple_of(2)
        || model_draw.is_empty()
        || !model_draw.start.is_multiple_of(2)
        || !model_draw.end.is_multiple_of(2)
        || model.end != model_lighting.start
        || expected_draw_start != model_draw.start
    {
        return None;
    }
    let first_instance = model_draw.start.checked_div(2)?;
    let end_instance = model_draw.end.checked_div(2)?;
    let instance_count = end_instance.checked_sub(first_instance)?;
    (instance_count != 0).then_some(DrawIndexedIndirectArgs {
        index_count: MODEL_INDEX_COUNT,
        instance_count,
        first_index: 0,
        base_vertex: allocation
            .metadata_index
            .checked_mul(4)
            .and_then(|value| i32::try_from(value).ok())?,
        first_instance,
    })
}

pub(in crate::chunk) fn model_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    model_draw_command(allocation, direct_stream_addresses(allocation))
}

pub(in crate::chunk) fn model_mdi_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    model_draw_command(allocation, mdi_stream_addresses(allocation))
}

pub(in crate::chunk) fn transparent_model_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    let mut addresses = direct_stream_addresses(allocation);
    addresses.model_draw = addresses.transparent_model_draw.clone();
    model_draw_command(allocation, addresses)
}

pub(in crate::chunk) fn model_ref_count_for_witness(allocation: &GpuChunkAllocation) -> usize {
    allocation.model_range.as_ref().map_or(0, |range| {
        range.end.saturating_sub(range.start) as usize
            / (PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as usize
    })
}

pub(in crate::chunk) const LEGACY_FIXED_MODEL_QUADS_PER_REF: usize = 32;

pub(in crate::chunk) fn summarize_model_workload<'a>(
    allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
) -> ModelWorkloadCount {
    allocations
        .into_iter()
        .filter(|allocation| {
            model_direct_draw_command(allocation).is_some()
                || transparent_model_direct_draw_command(allocation).is_some()
        })
        .fold(ModelWorkloadCount::default(), |mut total, allocation| {
            let model_ref_count = model_ref_count_for_witness(allocation);
            let model_draw_ref_count = allocation
                .model_draw_range
                .as_ref()
                .into_iter()
                .chain(allocation.transparent_model_draw_range.as_ref())
                .fold(0_usize, |total, range| {
                    total.saturating_add(
                        range.end.saturating_sub(range.start) as usize
                            / (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as usize,
                    )
                });
            total.model_ref_count = total.model_ref_count.saturating_add(model_ref_count);
            total.model_draw_ref_count = total
                .model_draw_ref_count
                .saturating_add(model_draw_ref_count);
            total.legacy_fixed_slot_quad_invocations_avoided = total
                .legacy_fixed_slot_quad_invocations_avoided
                .saturating_add(
                    model_ref_count
                        .saturating_mul(LEGACY_FIXED_MODEL_QUADS_PER_REF)
                        .saturating_sub(model_draw_ref_count),
                );
            total
        })
}

pub(in crate::chunk) fn depth_liquid_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    if !allocation.has_depth_liquid {
        return None;
    }
    allocation.liquid_lighting_range.as_ref()?;
    let liquid = allocation.depth_liquid_range.as_ref()?;
    let instance_count = liquid.end.checked_sub(liquid.start)?;
    (instance_count != 0).then_some(DrawIndexedIndirectArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex: metadata_base_vertex(allocation.metadata_index)?,
        first_instance: liquid.start,
    })
}

pub(in crate::chunk) fn depth_liquid_direct_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    depth_liquid_draw_command(allocation)
}

pub(in crate::chunk) fn depth_liquid_mdi_draw_command(
    allocation: &GpuChunkAllocation,
) -> Option<DrawIndexedIndirectArgs> {
    depth_liquid_draw_command(allocation)
}

pub(in crate::chunk) fn metadata_base_vertex(metadata_index: u32) -> Option<i32> {
    metadata_index
        .checked_mul(4)
        .and_then(|value| i32::try_from(value).ok())
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
pub(in crate::chunk) struct GpuChunkOrigin {
    pub(in crate::chunk) value: [i32; 4],
    pub(in crate::chunk) cube_bases: [u32; 4],
}

pub(in crate::chunk) const _: () =
    assert!(std::mem::size_of::<GpuChunkOrigin>() == CHUNK_ORIGIN_BYTES as usize);

pub(in crate::chunk) fn gpu_chunk_origin(
    origin: [i32; 3],
    biome_start: u32,
    cube_quad_base: u32,
    cube_lighting_word_start: u32,
) -> Option<GpuChunkOrigin> {
    if !cube_lighting_word_start.is_multiple_of(2) {
        return None;
    }
    Some(GpuChunkOrigin {
        value: [
            origin[0],
            origin[1],
            origin[2],
            i32::try_from(biome_start).ok()?,
        ],
        cube_bases: [
            cube_quad_base,
            cube_lighting_word_start.checked_div(2)?,
            0,
            0,
        ],
    })
}

#[cfg(test)]
pub(in crate::chunk) fn build_indexed_indirect_commands<'a>(
    allocations: impl IntoIterator<Item = &'a GpuChunkAllocation>,
) -> Vec<DrawIndexedIndirectArgs> {
    allocations
        .into_iter()
        .filter_map(indexed_indirect_command)
        .collect()
}

pub(in crate::chunk) struct ChunkIndirectBatch {
    pub(in crate::chunk) visible_entities: Vec<Entity>,
    pub(in crate::chunk) drawn_allocations: Vec<(Entity, FrameAllocationIdentity)>,
    pub(in crate::chunk) indirect_offset: u64,
    pub(in crate::chunk) command_count: u32,
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkIndirectBatches(
    pub(in crate::chunk) HashMap<Entity, ChunkIndirectBatch>,
);

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkModelIndirectBatches(
    pub(in crate::chunk) HashMap<Entity, ChunkIndirectBatch>,
);

#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkDepthLiquidIndirectBatches(
    pub(in crate::chunk) HashMap<Entity, ChunkIndirectBatch>,
);

#[derive(Clone)]
pub(in crate::chunk) struct ArenaAllocation {
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    pub(in crate::chunk) cube_range: Option<Range<u32>>,
    pub(in crate::chunk) cube_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_range: Option<Range<u32>>,
    pub(in crate::chunk) model_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) transparent_model_draw_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_range: Option<Range<u32>>,
    pub(in crate::chunk) liquid_lighting_range: Option<Range<u32>>,
    pub(in crate::chunk) quad_capacity: u32,
    pub(in crate::chunk) geometry_stream_range: Option<Range<u32>>,
    pub(in crate::chunk) geometry_stream_capacity: u32,
    pub(in crate::chunk) biome_range: Range<u32>,
    pub(in crate::chunk) biome_capacity: u32,
    pub(in crate::chunk) gpu: GpuChunkAllocation,
}

#[derive(Clone)]
pub(in crate::chunk) struct RetiredArenaAllocation {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) identity: GpuChunkAllocation,
    pub(in crate::chunk) quad: Option<(Range<u32>, u32)>,
    pub(in crate::chunk) geometry: Option<(Range<u32>, u32)>,
    pub(in crate::chunk) biome: Option<(Range<u32>, u32)>,
    pub(in crate::chunk) origin: Option<u32>,
    pub(in crate::chunk) release_epoch: Option<u64>,
}

impl RetiredArenaAllocation {
    pub(in crate::chunk) fn geometry_only(
        entity: Entity,
        allocation: &ArenaAllocation,
    ) -> Option<Self> {
        Some(Self {
            entity,
            identity: allocation.gpu.clone(),
            quad: None,
            geometry: Some((
                allocation.geometry_stream_range.clone()?,
                allocation.geometry_stream_capacity,
            )),
            biome: None,
            origin: None,
            release_epoch: None,
        })
    }

    pub(in crate::chunk) fn full(entity: Entity, allocation: ArenaAllocation) -> Self {
        let metadata_index = allocation.gpu.metadata_index;
        Self {
            entity,
            identity: allocation.gpu,
            quad: allocation
                .cube_range
                .map(|range| (range, allocation.quad_capacity)),
            geometry: allocation
                .geometry_stream_range
                .map(|range| (range, allocation.geometry_stream_capacity)),
            biome: (allocation.biome_capacity != 0)
                .then_some((allocation.biome_range, allocation.biome_capacity)),
            origin: Some(metadata_index),
            release_epoch: None,
        }
    }

    pub(in crate::chunk) fn owned_bytes(&self) -> u64 {
        let quad = self
            .quad
            .as_ref()
            .map_or(0, |(_, capacity)| u64::from(*capacity) * PACKED_QUAD_BYTES);
        let geometry = self.geometry.as_ref().map_or(0, |(_, capacity)| {
            u64::from(*capacity) * GEOMETRY_STREAM_WORD_BYTES
        });
        let biome = self
            .biome
            .as_ref()
            .map_or(0, |(_, capacity)| u64::from(*capacity) * BIOME_WORD_BYTES);
        let origin = self.origin.map_or(0, |_| CHUNK_ORIGIN_BYTES);
        quad.saturating_add(geometry)
            .saturating_add(biome)
            .saturating_add(origin)
    }

    pub(in crate::chunk) fn can_release(&self, completed_epoch: u64) -> bool {
        self.release_epoch
            .is_some_and(|epoch| epoch <= completed_epoch)
    }
}

impl ArenaAllocation {
    pub(in crate::chunk) fn expected_streams(&self) -> ChunkStreamMask {
        let mut mask = ChunkStreamMask::default();
        if self.cube_range.is_some() || self.cube_lighting_range.is_some() {
            mask = mask | ChunkStreamMask::CUBE;
        }
        if self.model_range.is_some()
            || self.model_lighting_range.is_some()
            || self.model_draw_range.is_some()
            || self.transparent_model_draw_range.is_some()
        {
            mask = mask | ChunkStreamMask::MODEL;
        }
        if self.liquid_range.is_some() || self.liquid_lighting_range.is_some() {
            mask = mask | ChunkStreamMask::LIQUID;
        }
        mask
    }
}
