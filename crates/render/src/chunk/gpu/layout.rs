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

#[cfg(test)]
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
    pub(in crate::chunk) fn try_reserve_permitted(&mut self, incremental_bytes: u64) -> bool {
        let limits = PublicationServiceConfig::PHASE2_GATE;
        self.try_reserve_within(
            limits.maximum_frame_items,
            limits.maximum_frame_bytes,
            incremental_bytes,
        )
    }

    pub(in crate::chunk) fn try_reserve(
        &mut self,
        budget: ChunkUploadBudget,
        incremental_bytes: u64,
    ) -> bool {
        self.try_reserve_within(
            budget.max_per_frame,
            budget.max_bytes_per_frame,
            incremental_bytes,
        )
    }

    fn try_reserve_within(
        &mut self,
        max_items: usize,
        max_bytes: u64,
        incremental_bytes: u64,
    ) -> bool {
        let (Some(items), Some(incremental_bytes)) = (
            self.items.checked_add(1),
            self.incremental_bytes.checked_add(incremental_bytes),
        ) else {
            return false;
        };
        let next = Self {
            items,
            incremental_bytes,
            ..*self
        };
        if next.items > max_items || next.total_bytes() > max_bytes {
            return false;
        }
        *self = next;
        true
    }

    /// Migration copy bytes this frame may still spend under the literal frame ceiling.
    pub(in crate::chunk) fn migration_allowance(self) -> u64 {
        let frame_room = PublicationServiceConfig::PHASE2_GATE
            .maximum_frame_bytes
            .saturating_sub(self.total_bytes());
        ARENA_MIGRATION_FRAME_BYTES
            .saturating_sub(self.growth_copy_bytes)
            .min(frame_room)
    }

    pub(in crate::chunk) const fn total_bytes(self) -> u64 {
        self.incremental_bytes
            .saturating_add(self.growth_copy_bytes)
    }
}

/// GPU-local arena migration copy allowed per frame. Growth of any legal size
/// therefore completes in finitely many frames under the literal frame ceiling.
pub(in crate::chunk) const ARENA_MIGRATION_FRAME_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) enum ArenaStream {
    Quads,
    GeometryStreams,
    Origins,
    Biomes,
}

impl ArenaStream {
    const fn item_bytes(self) -> u64 {
        match self {
            Self::Quads => PACKED_QUAD_BYTES,
            Self::GeometryStreams => GEOMETRY_STREAM_WORD_BYTES,
            Self::Origins => CHUNK_ORIGIN_BYTES,
            Self::Biomes => BIOME_WORD_BYTES,
        }
    }

    const fn label(self) -> &'static str {
        match self {
            Self::Quads => "packed chunk quads",
            Self::GeometryStreams => "packed chunk geometry streams",
            Self::Origins => "packed chunk origins",
            Self::Biomes => "packed chunk biome records",
        }
    }

    fn buffer_and_capacity(self, arena: &mut ChunkGpuArena) -> (&mut Buffer, &mut usize) {
        match self {
            Self::Quads => (&mut arena.quad_buffer, &mut arena.quad_capacity),
            Self::GeometryStreams => (
                &mut arena.geometry_stream_buffer,
                &mut arena.geometry_stream_capacity,
            ),
            Self::Origins => (&mut arena.origin_buffer, &mut arena.origin_capacity),
            Self::Biomes => (&mut arena.biome_buffer, &mut arena.biome_capacity),
        }
    }
}

/// A buffer replacement copied across frames; the old buffer keeps serving
/// draws until the final slice lands and the two are swapped.
pub(in crate::chunk) struct ArenaMigration {
    pub(in crate::chunk) stream: ArenaStream,
    pub(in crate::chunk) buffer: Buffer,
    pub(in crate::chunk) new_capacity: usize,
    pub(in crate::chunk) copy_bytes: u64,
    pub(in crate::chunk) copied_bytes: u64,
}

impl ArenaMigration {
    /// Once a slice has landed, a write to the old buffer could be lost.
    pub(in crate::chunk) const fn blocks_arena_writes(&self) -> bool {
        self.copied_bytes > 0
    }
}

pub(in crate::chunk) fn arena_capacities(arena: &ChunkGpuArena) -> ArenaRequiredLengths {
    ArenaRequiredLengths {
        quads: arena.quad_capacity,
        geometry_stream_words: arena.geometry_stream_capacity,
        origins: arena.origin_capacity,
        biome_words: arena.biome_capacity,
    }
}

/// The first stream whose capacity cannot hold `required`, or an error when
/// `required` exceeds the adapter.
pub(in crate::chunk) fn first_arena_growth(
    capacities: ArenaRequiredLengths,
    required: ArenaRequiredLengths,
    limits: ArenaLimits,
) -> Result<Option<(ArenaStream, ArenaGrowthPlan)>, ArenaGrowthError> {
    let streams = [
        (
            ArenaStream::Quads,
            capacities.quads,
            required.quads,
            limits.max_quad_items,
        ),
        (
            ArenaStream::GeometryStreams,
            capacities.geometry_stream_words,
            required.geometry_stream_words,
            limits.max_geometry_stream_words,
        ),
        (
            ArenaStream::Origins,
            capacities.origins,
            required.origins,
            limits.max_origin_items,
        ),
        (
            ArenaStream::Biomes,
            capacities.biome_words,
            required.biome_words,
            limits.max_biome_words,
        ),
    ];
    let mut first = None;
    for (stream, capacity, required, max_items) in streams {
        let plan = plan_arena_growth(capacity, required, stream.item_bytes(), max_items)?;
        if first.is_none() {
            first = plan.map(|plan| (stream, plan));
        }
    }
    Ok(first)
}

pub(in crate::chunk) fn begin_arena_migration(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    stream: ArenaStream,
    growth: ArenaGrowthPlan,
) {
    debug_assert!(arena.migration.is_none());
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "terrain.arena_allocate",
        stream = stream.label(),
        bytes = growth.new_capacity as u64 * stream.item_bytes(),
    )
    .entered();
    arena.migration = Some(ArenaMigration {
        stream,
        buffer: create_storage_buffer(
            render_device,
            stream.label(),
            growth.new_capacity as u64 * stream.item_bytes(),
        ),
        new_capacity: growth.new_capacity,
        copy_bytes: growth.gpu_copy_bytes,
        copied_bytes: 0,
    });
    super::telemetry::log_arena_capacity(arena, "migration started");
}

/// Copies at most `allowance` bytes of the active migration and swaps the
/// buffers once complete; returns the bytes copied.
pub(in crate::chunk) fn advance_arena_migration(
    arena: &mut ChunkGpuArena,
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    allowance: u64,
) -> u64 {
    let Some(mut migration) = arena.migration.take() else {
        return 0;
    };
    let slice = (migration.copy_bytes - migration.copied_bytes)
        .min(allowance & !(wgpu::COPY_BUFFER_ALIGNMENT - 1));
    let (buffer, capacity) = migration.stream.buffer_and_capacity(arena);
    if slice > 0 {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "terrain.migration",
            stream = migration.stream.label(),
            bytes = slice,
            offset = migration.copied_bytes,
        )
        .entered();
        let mut encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("migrate packed chunk arena"),
        });
        encoder.copy_buffer_to_buffer(
            buffer,
            migration.copied_bytes,
            &migration.buffer,
            migration.copied_bytes,
            slice,
        );
        let command = encoder.finish();
        {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!("terrain.migration_submit", bytes = slice).entered();
            render_queue.submit([command]);
        }
        migration.copied_bytes += slice;
    }
    if migration.copied_bytes == migration.copy_bytes {
        *buffer = migration.buffer;
        *capacity = migration.new_capacity;
        super::telemetry::log_arena_capacity(arena, "migration completed");
    } else {
        arena.migration = Some(migration);
    }
    slice
}

/// Writes geometry-stream words, mirrored into an in-flight migration target.
pub(in crate::chunk) fn write_geometry_stream_words(
    arena: &ChunkGpuArena,
    render_queue: &RenderQueue,
    offset_bytes: u64,
    bytes: &[u8],
) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("terrain.geometry_write", offset_bytes, bytes = bytes.len())
        .entered();
    render_queue.write_buffer(&arena.geometry_stream_buffer, offset_bytes, bytes);
    if let Some(migration) = arena
        .migration
        .as_ref()
        .filter(|migration| migration.stream == ArenaStream::GeometryStreams)
    {
        render_queue.write_buffer(&migration.buffer, offset_bytes, bytes);
    }
}

pub(in crate::chunk) fn account_chunk_gpu_uploads(
    budget: ChunkUploadBudget,
    chunk_updates: usize,
    incremental_bytes: u64,
    gpu_copy_bytes: u64,
) -> ChunkGpuUploadStats {
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
