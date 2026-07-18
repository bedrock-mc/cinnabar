use crate::chunk::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::chunk) struct GeometryStreamCounts {
    pub(in crate::chunk) cube: u32,
    pub(in crate::chunk) cube_lighting: u32,
    pub(in crate::chunk) model: u32,
    pub(in crate::chunk) model_lighting: u32,
    pub(in crate::chunk) model_draw: u32,
    pub(in crate::chunk) transparent_model_draw: u32,
    pub(in crate::chunk) liquid: u32,
    pub(in crate::chunk) liquid_lighting: u32,
}

pub(in crate::chunk) const SHARED_GEOMETRY_ALIGNMENT_WORDS: u32 =
    (PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct GeometryStreamLayout {
    pub(in crate::chunk) model_offset: u32,
    pub(in crate::chunk) model_lighting_offset: u32,
    pub(in crate::chunk) model_draw_offset: u32,
    pub(in crate::chunk) transparent_model_draw_offset: u32,
    pub(in crate::chunk) liquid_offset: u32,
    pub(in crate::chunk) liquid_lighting_offset: u32,
    pub(in crate::chunk) cube_lighting_offset: u32,
    pub(in crate::chunk) word_count: u32,
}

impl GeometryStreamCounts {
    pub(in crate::chunk) fn layout(self) -> Option<GeometryStreamLayout> {
        let model_offset = 0;
        let model_lighting_offset = self
            .model
            .checked_mul((PACKED_MODEL_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?;
        let model_lighting_end = model_lighting_offset.checked_add(
            self.model_lighting
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?,
        )?;
        let model_draw_offset = model_lighting_end;
        let model_draw_end = model_draw_offset
            .checked_add(self.model_draw.checked_mul(
                (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
            )?)?;
        let transparent_model_draw_offset = model_draw_end;
        let transparent_model_draw_end = transparent_model_draw_offset
            .checked_add(self.transparent_model_draw.checked_mul(
                (PACKED_MODEL_DRAW_REF_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
            )?)?;
        let liquid_offset = if self.liquid == 0 && self.liquid_lighting == 0 {
            transparent_model_draw_end
        } else {
            checked_align_up(transparent_model_draw_end, SHARED_GEOMETRY_ALIGNMENT_WORDS)?
        };
        let liquid_lighting_offset =
            liquid_offset
                .checked_add(self.liquid.checked_mul(
                    (PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32,
                )?)?;
        let cube_lighting_offset = liquid_lighting_offset.checked_add(
            self.liquid_lighting
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?,
        )?;
        let word_count = cube_lighting_offset.checked_add(
            self.cube_lighting
                .checked_mul((PACKED_QUAD_LIGHTING_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32)?,
        )?;
        Some(GeometryStreamLayout {
            model_offset,
            model_lighting_offset,
            model_draw_offset,
            transparent_model_draw_offset,
            liquid_offset,
            liquid_lighting_offset,
            cube_lighting_offset,
            word_count,
        })
    }

    pub(in crate::chunk) fn shared_word_count(self) -> Option<u32> {
        Some(self.layout()?.word_count)
    }
}

pub(in crate::chunk) fn checked_align_up(value: u32, alignment: u32) -> Option<u32> {
    debug_assert!(alignment.is_power_of_two());
    value
        .checked_add(alignment.checked_sub(1)?)
        .map(|value| value & !(alignment - 1))
}

pub(in crate::chunk) fn transparent_geometry_update_requires_cow(
    old: &ArenaAllocation,
    required: GeometryStreamCounts,
) -> bool {
    if !old.gpu.has_transparent_liquid {
        return false;
    }
    let Some(old_liquid) = old.liquid_range.as_ref() else {
        return false;
    };
    let Some(stream) = old.geometry_stream_range.as_ref() else {
        return true;
    };
    let Some(layout) = required.layout() else {
        return true;
    };
    let Some(liquid_start) = stream.start.checked_add(layout.liquid_offset) else {
        return true;
    };
    let Some(liquid_end) = liquid_start.checked_add(
        required
            .liquid
            .saturating_mul((PACKED_LIQUID_QUAD_BYTES / GEOMETRY_STREAM_WORD_BYTES) as u32),
    ) else {
        return true;
    };
    layout.word_count > old.geometry_stream_capacity
        || liquid_start != old_liquid.start
        || liquid_end < old_liquid.end
}

pub(in crate::chunk) fn buffer_byte_len(item_count: usize, item_bytes: u64) -> u64 {
    u64::try_from(item_count)
        .unwrap_or(u64::MAX)
        .saturating_mul(item_bytes)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::chunk) struct ArenaRequiredLengths {
    pub(in crate::chunk) quads: usize,
    pub(in crate::chunk) geometry_stream_words: usize,
    pub(in crate::chunk) origins: usize,
    pub(in crate::chunk) biome_words: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(in crate::chunk) struct GpuUploadReservation {
    pub(in crate::chunk) items: usize,
    pub(in crate::chunk) incremental_bytes: u64,
    pub(in crate::chunk) growth_copy_bytes: u64,
}

impl GpuUploadReservation {
    pub(in crate::chunk) fn try_reserve(
        &mut self,
        budget: ChunkUploadBudget,
        incremental_bytes: u64,
        projected_growth_copy_bytes: u64,
        growth_copy_ceiling: u64,
    ) -> bool {
        let next = Self {
            items: self.items.saturating_add(1),
            incremental_bytes: self.incremental_bytes.saturating_add(incremental_bytes),
            growth_copy_bytes: self.growth_copy_bytes.max(projected_growth_copy_bytes),
        };
        let first_growth_crossing = self.items == 0
            && incremental_bytes <= budget.max_bytes_per_frame
            && projected_growth_copy_bytes > 0
            && projected_growth_copy_bytes <= growth_copy_ceiling;
        if next.items > budget.max_per_frame
            || (next.total_bytes() > budget.max_bytes_per_frame && !first_growth_crossing)
        {
            return false;
        }
        *self = next;
        true
    }

    pub(in crate::chunk) const fn total_bytes(self) -> u64 {
        self.incremental_bytes
            .saturating_add(self.growth_copy_bytes)
    }
}

/// A finite upper bound for one atomic whole-arena growth copy on this
/// adapter. Every legal growth plan fits this allowance, so it cannot starve
/// behind the smaller adaptive incremental-upload budget.
pub(in crate::chunk) fn arena_growth_copy_ceiling(limits: ArenaLimits) -> u64 {
    buffer_byte_len(limits.max_quad_items, PACKED_QUAD_BYTES)
        .saturating_add(buffer_byte_len(
            limits.max_geometry_stream_words,
            GEOMETRY_STREAM_WORD_BYTES,
        ))
        .saturating_add(buffer_byte_len(limits.max_origin_items, CHUNK_ORIGIN_BYTES))
        .saturating_add(buffer_byte_len(limits.max_biome_words, BIOME_WORD_BYTES))
}

pub(in crate::chunk) fn planned_arena_growth_copy_bytes(
    capacities: ArenaRequiredLengths,
    required: ArenaRequiredLengths,
    limits: ArenaLimits,
) -> Option<u64> {
    let plans = [
        plan_arena_growth(
            capacities.quads,
            required.quads,
            PACKED_QUAD_BYTES,
            limits.max_quad_items,
        )
        .ok()?,
        plan_arena_growth(
            capacities.geometry_stream_words,
            required.geometry_stream_words,
            GEOMETRY_STREAM_WORD_BYTES,
            limits.max_geometry_stream_words,
        )
        .ok()?,
        plan_arena_growth(
            capacities.origins,
            required.origins,
            CHUNK_ORIGIN_BYTES,
            limits.max_origin_items,
        )
        .ok()?,
        plan_arena_growth(
            capacities.biome_words,
            required.biome_words,
            BIOME_WORD_BYTES,
            limits.max_biome_words,
        )
        .ok()?,
    ];
    Some(plans.into_iter().flatten().fold(0_u64, |total, growth| {
        total.saturating_add(growth.gpu_copy_bytes)
    }))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn account_chunk_gpu_uploads(
    budget: ChunkUploadBudget,
    chunk_updates: usize,
    quad_incremental_bytes: u64,
    origin_incremental_bytes: u64,
    biome_incremental_bytes: u64,
    quad_gpu_copy_bytes: u64,
    origin_gpu_copy_bytes: u64,
    biome_gpu_copy_bytes: u64,
) -> ChunkGpuUploadStats {
    let incremental_bytes = quad_incremental_bytes
        .saturating_add(origin_incremental_bytes)
        .saturating_add(biome_incremental_bytes);
    let gpu_copy_bytes = quad_gpu_copy_bytes
        .saturating_add(origin_gpu_copy_bytes)
        .saturating_add(biome_gpu_copy_bytes);
    ChunkGpuUploadStats {
        chunk_updates,
        chunk_budget: budget.max_per_frame,
        incremental_bytes,
        gpu_copy_bytes,
        full_shadow_bytes: 0,
        total_bytes: incremental_bytes.saturating_add(gpu_copy_bytes),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct ArenaGrowthPlan {
    pub(in crate::chunk) new_capacity: usize,
    pub(in crate::chunk) gpu_copy_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct ArenaGrowthError;

pub(in crate::chunk) fn plan_arena_growth(
    current_capacity: usize,
    required_len: usize,
    item_bytes: u64,
    max_items: usize,
) -> Result<Option<ArenaGrowthPlan>, ArenaGrowthError> {
    if required_len > max_items {
        return Err(ArenaGrowthError);
    }
    if required_len <= current_capacity {
        return Ok(None);
    }
    let new_capacity = required_len
        .checked_next_power_of_two()
        .unwrap_or(max_items)
        .min(max_items);
    Ok(Some(ArenaGrowthPlan {
        new_capacity,
        gpu_copy_bytes: buffer_byte_len(current_capacity, item_bytes),
    }))
}

pub(in crate::chunk) fn ensure_quad_capacity(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(
        arena.quad_capacity,
        arena.quad_len,
        PACKED_QUAD_BYTES,
        arena.limits.max_quad_items,
    ) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        "packed chunk quads",
        growth.new_capacity as u64 * PACKED_QUAD_BYTES,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        &arena.quad_buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    arena.quad_capacity = growth.new_capacity;
    arena.quad_buffer = next;
    growth.gpu_copy_bytes
}

pub(in crate::chunk) fn ensure_geometry_stream_capacities(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    ensure_stream_capacity(
        &mut arena.geometry_stream_buffer,
        &mut arena.geometry_stream_capacity,
        arena.geometry_stream_len,
        arena.limits.max_geometry_stream_words,
        GEOMETRY_STREAM_WORD_BYTES,
        "packed chunk geometry streams",
        render_device,
        render_queue,
    )
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn ensure_stream_capacity(
    buffer: &mut Buffer,
    capacity: &mut usize,
    required_len: usize,
    max_items: usize,
    item_bytes: u64,
    label: &'static str,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(*capacity, required_len, item_bytes, max_items) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        label,
        growth.new_capacity as u64 * item_bytes,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    *capacity = growth.new_capacity;
    *buffer = next;
    growth.gpu_copy_bytes
}

pub(in crate::chunk) fn write_stream_records<T: bytemuck::Pod>(
    render_queue: &RenderQueue,
    buffer: &Buffer,
    item_bytes: u64,
    writes: Vec<(u32, Vec<T>)>,
) {
    for (offset, records) in writes {
        if !records.is_empty() {
            render_queue.write_buffer(
                buffer,
                u64::from(offset) * item_bytes,
                bytemuck::cast_slice(&records),
            );
        }
    }
}

pub(in crate::chunk) fn ensure_origin_capacity(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(
        arena.origin_capacity,
        arena.origin_len,
        CHUNK_ORIGIN_BYTES,
        arena.limits.max_origin_items,
    ) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        "packed chunk origins",
        growth.new_capacity as u64 * CHUNK_ORIGIN_BYTES,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        &arena.origin_buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    arena.origin_capacity = growth.new_capacity;
    arena.origin_buffer = next;
    growth.gpu_copy_bytes
}

pub(in crate::chunk) fn ensure_biome_capacity(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
) -> u64 {
    let Ok(Some(growth)) = plan_arena_growth(
        arena.biome_capacity,
        arena.biome_len,
        BIOME_WORD_BYTES,
        arena.limits.max_biome_words,
    ) else {
        return 0;
    };
    let next = create_storage_buffer(
        render_device,
        "packed chunk biome records",
        growth.new_capacity as u64 * BIOME_WORD_BYTES,
    );
    copy_gpu_buffer(
        render_device,
        render_queue,
        &arena.biome_buffer,
        &next,
        growth.gpu_copy_bytes,
    );
    arena.biome_capacity = growth.new_capacity;
    arena.biome_buffer = next;
    growth.gpu_copy_bytes
}

pub(in crate::chunk) fn copy_gpu_buffer(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    source: &Buffer,
    destination: &Buffer,
    bytes: u64,
) {
    if bytes == 0 {
        return;
    }
    let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("grow packed chunk arena"),
    });
    encoder.copy_buffer_to_buffer(source, 0, destination, 0, bytes);
    render_queue.submit([encoder.finish()]);
}
