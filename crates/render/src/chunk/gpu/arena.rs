use crate::chunk::*;

#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct ChunkGpuUploadStats {
    // Actual packed-quad/origin arena writes for the most recent render frame.
    // Indirect-command uploads are visibility work and are intentionally separate.
    pub(in crate::chunk) chunk_updates: usize,
    pub(in crate::chunk) chunk_budget: usize,
    pub(in crate::chunk) incremental_bytes: u64,
    pub(in crate::chunk) gpu_copy_bytes: u64,
    pub(in crate::chunk) full_shadow_bytes: u64,
    pub(in crate::chunk) total_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct ArenaLimits {
    pub(in crate::chunk) max_quad_items: usize,
    pub(in crate::chunk) max_geometry_stream_words: usize,
    pub(in crate::chunk) max_origin_items: usize,
    pub(in crate::chunk) max_biome_words: usize,
}

pub(in crate::chunk) fn arena_limits_from_device_limits(
    max_buffer_size: u64,
    max_storage_buffer_binding_size: u64,
) -> ArenaLimits {
    let storage_bytes = max_buffer_size.min(max_storage_buffer_binding_size);
    let max_quad_items = (storage_bytes / PACKED_QUAD_BYTES)
        .min(u64::from(u32::MAX))
        .try_into()
        .unwrap_or(usize::MAX);
    let max_geometry_stream_words = (storage_bytes / GEOMETRY_STREAM_WORD_BYTES)
        .min(u64::from(u32::MAX))
        .try_into()
        .unwrap_or(usize::MAX);
    let max_origin_items = (storage_bytes / CHUNK_ORIGIN_BYTES)
        .min((i32::MAX as u64) / 4)
        .try_into()
        .unwrap_or(usize::MAX);
    let max_biome_words = (storage_bytes / BIOME_WORD_BYTES)
        .min(i32::MAX as u64)
        .try_into()
        .unwrap_or(usize::MAX);
    ArenaLimits {
        max_quad_items,
        max_geometry_stream_words,
        max_origin_items,
        max_biome_words,
    }
}

#[derive(Resource)]
pub(in crate::chunk) struct ChunkGpuArena {
    pub(in crate::chunk) quad_buffer: Buffer,
    pub(in crate::chunk) geometry_stream_buffer: Buffer,
    pub(in crate::chunk) origin_buffer: Buffer,
    pub(in crate::chunk) biome_buffer: Buffer,
    pub(in crate::chunk) index_buffer: Buffer,
    pub(in crate::chunk) model_index_buffer: Buffer,
    pub(in crate::chunk) indirect_buffer: Buffer,
    pub(in crate::chunk) transparent_indirect_buffer: Buffer,
    pub(in crate::chunk) transparent_ref_buffer: Buffer,
    pub(in crate::chunk) bind_group: Option<BindGroup>,
    pub(in crate::chunk) bind_group_buffers: Option<ChunkBindGroupBuffers>,
    pub(in crate::chunk) quad_capacity: usize,
    pub(in crate::chunk) geometry_stream_capacity: usize,
    pub(in crate::chunk) origin_capacity: usize,
    pub(in crate::chunk) biome_capacity: usize,
    pub(in crate::chunk) indirect_capacity: usize,
    pub(in crate::chunk) quad_len: usize,
    pub(in crate::chunk) geometry_stream_len: usize,
    pub(in crate::chunk) origin_len: usize,
    pub(in crate::chunk) biome_len: usize,
    pub(in crate::chunk) limits: ArenaLimits,
    pub(in crate::chunk) free_quads: Vec<Range<u32>>,
    pub(in crate::chunk) free_geometry_stream_words: Vec<Range<u32>>,
    pub(in crate::chunk) free_origins: Vec<u32>,
    pub(in crate::chunk) free_biomes: Vec<Range<u32>>,
    pub(in crate::chunk) allocations: HashMap<Entity, ArenaAllocation>,
    pub(in crate::chunk) retired_allocations: Vec<RetiredArenaAllocation>,
    pub(in crate::chunk) pending_removals: BTreeSet<Entity>,
    pub(in crate::chunk) retirement_budget: TransparentRetirementBudget,
}

pub(in crate::chunk) fn init_chunk_gpu_arena(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
) {
    commands.insert_resource(ChunkGpuArena::new(&render_device));
}

impl ChunkGpuArena {
    pub(in crate::chunk) fn new(render_device: &RenderDevice) -> Self {
        let device_limits = render_device.limits();
        let limits = arena_limits_from_device_limits(
            device_limits.max_buffer_size,
            u64::from(device_limits.max_storage_buffer_binding_size),
        );
        Self {
            quad_buffer: create_storage_buffer(
                render_device,
                "packed chunk quads",
                PACKED_QUAD_BYTES,
            ),
            geometry_stream_buffer: create_storage_buffer(
                render_device,
                "packed chunk geometry streams",
                GEOMETRY_STREAM_WORD_BYTES,
            ),
            origin_buffer: create_storage_buffer(
                render_device,
                "packed chunk origins",
                CHUNK_ORIGIN_BYTES,
            ),
            biome_buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("packed chunk biome records"),
                contents: bytemuck::cast_slice(&FALLBACK_BIOME_RECORD),
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
            }),
            index_buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("shared chunk quad indices"),
                contents: bytemuck::cast_slice(&STATIC_QUAD_INDICES),
                usage: BufferUsages::INDEX,
            }),
            model_index_buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("shared model template quad indices"),
                contents: bytemuck::cast_slice(&STATIC_QUAD_INDICES),
                usage: BufferUsages::INDEX,
            }),
            indirect_buffer: create_indirect_buffer(render_device, 1),
            transparent_indirect_buffer: create_indirect_buffer(render_device, 1),
            transparent_ref_buffer: create_storage_buffer(
                render_device,
                "double-buffered transparent draw refs",
                TRANSPARENT_REF_BUFFER_BYTES as u64,
            ),
            bind_group: None,
            bind_group_buffers: None,
            quad_capacity: 1,
            geometry_stream_capacity: 1,
            origin_capacity: 1,
            biome_capacity: FALLBACK_BIOME_WORDS,
            indirect_capacity: 1,
            quad_len: 0,
            geometry_stream_len: 0,
            origin_len: 0,
            biome_len: FALLBACK_BIOME_WORDS,
            limits,
            free_quads: Vec::new(),
            free_geometry_stream_words: Vec::new(),
            free_origins: Vec::new(),
            free_biomes: Vec::new(),
            allocations: HashMap::new(),
            retired_allocations: Vec::new(),
            pending_removals: BTreeSet::new(),
            retirement_budget: TransparentRetirementBudget::with_limits(
                MAX_TRANSPARENT_RETIRED_ALLOCATIONS,
                MAX_TRANSPARENT_RETIRED_BYTES,
            ),
        }
    }
}

pub(in crate::chunk) fn create_storage_buffer(
    render_device: &RenderDevice,
    label: &'static str,
    size: u64,
) -> Buffer {
    render_device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(in crate::chunk) fn create_indirect_buffer(
    render_device: &RenderDevice,
    command_capacity: usize,
) -> Buffer {
    render_device.create_buffer(&BufferDescriptor {
        label: Some("packed chunk indirect commands"),
        size: command_capacity as u64 * INDEXED_INDIRECT_BYTES,
        usage: BufferUsages::INDIRECT | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

#[derive(Debug, Clone, Copy)]
pub(in crate::chunk) struct GpuUpdateCandidate {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) generation: u64,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
}

pub(in crate::chunk) const MAX_GPU_UPDATE_FAIRNESS_ENTRIES: usize = 65_536;
pub(in crate::chunk) const GPU_UPDATE_OVERDUE_FRAMES: u32 = 2;

#[derive(Resource)]
pub(in crate::chunk) struct GpuUpdateFairness {
    pub(in crate::chunk) wait_ages: HashMap<Entity, u32>,
    pub(in crate::chunk) limit: usize,
}

impl Default for GpuUpdateFairness {
    fn default() -> Self {
        Self::with_limit(MAX_GPU_UPDATE_FAIRNESS_ENTRIES)
    }
}

impl GpuUpdateFairness {
    pub(in crate::chunk) fn with_limit(limit: usize) -> Self {
        Self {
            wait_ages: HashMap::new(),
            limit,
        }
    }

    pub(in crate::chunk) fn wait_age(&self, entity: Entity) -> u32 {
        self.wait_ages.get(&entity).copied().unwrap_or(0)
    }

    pub(in crate::chunk) fn finish_frame(&mut self, active: &[Entity], successful: &[Entity]) {
        let active_set = active.iter().copied().collect::<HashSet<_>>();
        let successful = successful.iter().copied().collect::<HashSet<_>>();
        self.wait_ages
            .retain(|entity, _| active_set.contains(entity));
        for &entity in &successful {
            self.wait_ages.remove(&entity);
        }
        for &entity in active.iter().filter(|entity| !successful.contains(entity)) {
            if let Some(age) = self.wait_ages.get_mut(&entity) {
                *age = age.saturating_add(1);
            } else if self.wait_ages.len() < self.limit {
                self.wait_ages.insert(entity, 1);
            }
        }
    }

    #[cfg(test)]
    pub(in crate::chunk) fn reset(&mut self) {
        self.wait_ages.clear();
    }

    #[cfg(test)]
    pub(in crate::chunk) fn len(&self) -> usize {
        self.wait_ages.len()
    }

    #[cfg(test)]
    pub(in crate::chunk) fn is_empty(&self) -> bool {
        self.wait_ages.is_empty()
    }
}

pub(in crate::chunk) const fn chunk_tint_identity_is_active(
    record: ChunkBiomeTintIdentity,
    active: ChunkBiomeTintIdentity,
) -> bool {
    record.stream() == active.stream() && record.revision() == active.revision()
}

pub(in crate::chunk) fn plan_gpu_chunk_updates(
    mut candidates: Vec<GpuUpdateCandidate>,
    allocations: &HashMap<Entity, ArenaAllocation>,
    camera_position: Vec3,
    active_tint_identity: ChunkBiomeTintIdentity,
    fairness: &GpuUpdateFairness,
) -> Vec<Entity> {
    candidates.retain(|candidate| {
        chunk_tint_identity_is_active(candidate.tint_identity, active_tint_identity)
            && allocations.get(&candidate.entity).is_none_or(|allocation| {
                allocation.generation != candidate.generation
                    || allocation.tint_identity != candidate.tint_identity
            })
    });
    candidates.sort_by(|left, right| {
        let left_age = fairness.wait_age(left.entity);
        let right_age = fairness.wait_age(right.entity);
        let left_overdue = left_age >= GPU_UPDATE_OVERDUE_FRAMES;
        let right_overdue = right_age >= GPU_UPDATE_OVERDUE_FRAMES;
        right_overdue
            .cmp(&left_overdue)
            .then_with(|| {
                if left_overdue && right_overdue {
                    right_age.cmp(&left_age)
                } else {
                    std::cmp::Ordering::Equal
                }
            })
            .then_with(|| {
                ChunkUploadPriority::from_camera(left.key, camera_position)
                    .distance_squared()
                    .total_cmp(
                        &ChunkUploadPriority::from_camera(right.key, camera_position)
                            .distance_squared(),
                    )
            })
            .then_with(|| left.key.cmp(&right.key))
    });
    candidates
        .into_iter()
        .map(|candidate| candidate.entity)
        .collect()
}

pub(in crate::chunk) fn commit_chunk_range_plan(
    arena: &mut ChunkGpuArena,
    mut plan: ChunkRangePlan,
) -> ChunkRangePlan {
    arena.quad_len = plan.quad_len;
    arena.free_quads = std::mem::take(&mut plan.free_quads);
    arena.geometry_stream_len = plan.geometry_stream_len;
    arena.free_geometry_stream_words = std::mem::take(&mut plan.free_geometry_stream_words);
    arena.biome_len = plan.biome_len;
    arena.free_biomes = std::mem::take(&mut plan.free_biomes);
    plan
}

pub(in crate::chunk) fn checked_geometry_range(start: u32, count: u32) -> Option<Range<u32>> {
    if count == 0 {
        return None;
    }
    start.checked_add(count).map(|end| start..end)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct ChunkRangePlan {
    pub(in crate::chunk) quad_start: u32,
    pub(in crate::chunk) quad_capacity: u32,
    pub(in crate::chunk) geometry_stream_start: u32,
    pub(in crate::chunk) geometry_stream_capacity: u32,
    pub(in crate::chunk) model_start: u32,
    pub(in crate::chunk) model_lighting_start: u32,
    pub(in crate::chunk) model_draw_start: u32,
    pub(in crate::chunk) transparent_model_draw_start: u32,
    pub(in crate::chunk) liquid_start: u32,
    pub(in crate::chunk) liquid_lighting_start: u32,
    pub(in crate::chunk) cube_lighting_start: u32,
    pub(in crate::chunk) biome_start: u32,
    pub(in crate::chunk) biome_capacity: u32,
    pub(in crate::chunk) quad_len: usize,
    pub(in crate::chunk) free_quads: Vec<Range<u32>>,
    pub(in crate::chunk) geometry_stream_len: usize,
    pub(in crate::chunk) free_geometry_stream_words: Vec<Range<u32>>,
    pub(in crate::chunk) biome_len: usize,
    pub(in crate::chunk) free_biomes: Vec<Range<u32>>,
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn plan_chunk_range_update(
    mut quad_len: usize,
    current_free_quads: &[Range<u32>],
    mut geometry_stream_len: usize,
    current_free_geometry_stream_words: &[Range<u32>],
    mut biome_len: usize,
    current_free_biomes: &[Range<u32>],
    required: GeometryStreamCounts,
    biome_required: u32,
    old: Option<&ArenaAllocation>,
    preserve_old_geometry: bool,
    limits: ArenaLimits,
) -> Option<ChunkRangePlan> {
    let mut free_quads = current_free_quads.to_vec();
    let (quad_start, quad_capacity) = allocate_range_for_update(
        &mut quad_len,
        &mut free_quads,
        required.cube,
        old.and_then(|old| {
            old.cube_range
                .as_ref()
                .map(|range| (range.start, old.quad_capacity))
        }),
        limits.max_quad_items,
        0,
    )?;

    let geometry_layout = required.layout()?;
    let geometry_words_required = geometry_layout.word_count;
    let mut free_geometry_stream_words = current_free_geometry_stream_words.to_vec();
    let (geometry_stream_start, geometry_stream_capacity) = allocate_aligned_range_for_update(
        &mut geometry_stream_len,
        &mut free_geometry_stream_words,
        geometry_words_required,
        (!preserve_old_geometry)
            .then_some(old)
            .flatten()
            .and_then(|old| {
                old.geometry_stream_range
                    .as_ref()
                    .map(|range| (range.start, old.geometry_stream_capacity))
            }),
        limits.max_geometry_stream_words,
        0,
        SHARED_GEOMETRY_ALIGNMENT_WORDS,
    )?;
    let model_start = geometry_stream_start.checked_add(geometry_layout.model_offset)?;
    let model_lighting_start =
        geometry_stream_start.checked_add(geometry_layout.model_lighting_offset)?;
    let model_draw_start = geometry_stream_start.checked_add(geometry_layout.model_draw_offset)?;
    let transparent_model_draw_start =
        geometry_stream_start.checked_add(geometry_layout.transparent_model_draw_offset)?;
    let liquid_start = geometry_stream_start.checked_add(geometry_layout.liquid_offset)?;
    let liquid_lighting_start =
        geometry_stream_start.checked_add(geometry_layout.liquid_lighting_offset)?;
    let cube_lighting_start =
        geometry_stream_start.checked_add(geometry_layout.cube_lighting_offset)?;

    let mut free_biomes = current_free_biomes.to_vec();
    let (biome_start, biome_capacity) = allocate_range_for_update(
        &mut biome_len,
        &mut free_biomes,
        biome_required,
        old.map(|old| (old.biome_range.start, old.biome_capacity)),
        limits.max_biome_words,
        0,
    )?;
    Some(ChunkRangePlan {
        quad_start,
        quad_capacity,
        geometry_stream_start,
        geometry_stream_capacity,
        model_start,
        model_lighting_start,
        model_draw_start,
        transparent_model_draw_start,
        liquid_start,
        liquid_lighting_start,
        cube_lighting_start,
        biome_start,
        biome_capacity,
        quad_len,
        free_quads,
        geometry_stream_len,
        free_geometry_stream_words,
        biome_len,
        free_biomes,
    })
}

pub(in crate::chunk) fn allocate_range_for_update(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    old: Option<(u32, u32)>,
    max_items: usize,
    empty_start: u32,
) -> Option<(u32, u32)> {
    if let Some((start, capacity)) = old
        && required != 0
        && required <= capacity
    {
        return Some((start, capacity));
    }
    if let Some((start, capacity)) = old
        && capacity != 0
    {
        release_quad_range(len, free, start..start.saturating_add(capacity));
    }
    if required == 0 {
        return Some((empty_start, 0));
    }
    allocate_quad_range(len, free, required, max_items).map(|start| (start, required))
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn allocate_aligned_range_for_update(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    old: Option<(u32, u32)>,
    max_items: usize,
    empty_start: u32,
    alignment: u32,
) -> Option<(u32, u32)> {
    if let Some((start, capacity)) = old
        && required != 0
        && required <= capacity
        && start.is_multiple_of(alignment)
    {
        return Some((start, capacity));
    }
    if let Some((start, capacity)) = old
        && capacity != 0
    {
        release_quad_range(len, free, start..start.saturating_add(capacity));
    }
    if required == 0 {
        return Some((empty_start, 0));
    }
    allocate_aligned_quad_range(len, free, required, max_items, alignment)
        .map(|start| (start, required))
}

pub(in crate::chunk) fn allocate_aligned_quad_range(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    max_items: usize,
    alignment: u32,
) -> Option<u32> {
    for index in 0..free.len() {
        let start = checked_align_up(free[index].start, alignment)?;
        let end = start.checked_add(required)?;
        if end > free[index].end {
            continue;
        }
        let source = free.remove(index);
        insert_free_quad_range(free, source.start..start);
        insert_free_quad_range(free, end..source.end);
        return Some(start);
    }

    let current = u32::try_from(*len).ok()?;
    let start = checked_align_up(current, alignment)?;
    let end = start.checked_add(required)?;
    if end as usize > max_items {
        return None;
    }
    insert_free_quad_range(free, current..start);
    *len = end as usize;
    Some(start)
}

pub(in crate::chunk) fn allocate_quad_range(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    required: u32,
    max_items: usize,
) -> Option<u32> {
    if let Some(start) = take_free_quad_range(free, required) {
        return Some(start);
    }
    let required = required as usize;
    let next = len.checked_add(required)?;
    if next > max_items || *len > u32::MAX as usize {
        return None;
    }
    let start = *len as u32;
    *len = next;
    Some(start)
}

pub(in crate::chunk) fn release_quad_range(
    len: &mut usize,
    free: &mut Vec<Range<u32>>,
    freed: Range<u32>,
) {
    insert_free_quad_range(free, freed);
    while let Some(last) = free.last() {
        if last.end as usize != *len {
            break;
        }
        *len = last.start as usize;
        free.pop();
    }
}

pub(in crate::chunk) fn insert_free_quad_range(free: &mut Vec<Range<u32>>, mut freed: Range<u32>) {
    if freed.is_empty() {
        return;
    }

    let index = free.partition_point(|range| range.end < freed.start);
    while index < free.len() && free[index].start <= freed.end {
        let adjacent = free.remove(index);
        freed.start = freed.start.min(adjacent.start);
        freed.end = freed.end.max(adjacent.end);
    }
    free.insert(index, freed);
}

pub(in crate::chunk) fn take_free_quad_range(
    free: &mut Vec<Range<u32>>,
    required: u32,
) -> Option<u32> {
    let index = free
        .iter()
        .position(|range| range.end - range.start >= required)?;
    let start = free[index].start;
    free[index].start += required;
    if free[index].is_empty() {
        free.remove(index);
    }
    Some(start)
}

pub(in crate::chunk) fn allocate_origin(arena: &mut ChunkGpuArena) -> Option<u32> {
    if let Some(index) = arena.free_origins.pop() {
        return Some(index);
    }
    if arena.origin_len >= arena.limits.max_origin_items || arena.origin_len > u32::MAX as usize {
        return None;
    }
    let index = arena.origin_len as u32;
    arena.origin_len += 1;
    Some(index)
}

pub(in crate::chunk) fn release_origin(arena: &mut ChunkGpuArena, index: u32) {
    arena.free_origins.push(index);
    while arena.origin_len > 0 {
        let tail = (arena.origin_len - 1) as u32;
        let Some(position) = arena.free_origins.iter().position(|free| *free == tail) else {
            break;
        };
        arena.free_origins.swap_remove(position);
        arena.origin_len -= 1;
    }
}

pub(in crate::chunk) fn free_allocation(arena: &mut ChunkGpuArena, entity: Entity) {
    if let Some(allocation) = arena.allocations.remove(&entity) {
        if let Some(range) = allocation.cube_range {
            release_quad_range(
                &mut arena.quad_len,
                &mut arena.free_quads,
                range.start..range.start + allocation.quad_capacity,
            );
        }
        if let Some(range) = allocation.geometry_stream_range {
            release_quad_range(
                &mut arena.geometry_stream_len,
                &mut arena.free_geometry_stream_words,
                range.start..range.start + allocation.geometry_stream_capacity,
            );
        }
        if allocation.biome_capacity != 0 {
            let freed = allocation.biome_range.start
                ..allocation.biome_range.start + allocation.biome_capacity;
            release_quad_range(&mut arena.biome_len, &mut arena.free_biomes, freed);
        }
        release_origin(arena, allocation.gpu.metadata_index);
    }
}

pub(in crate::chunk) fn release_completed_transparent_retirements(
    arena: &mut ChunkGpuArena,
    completed_epoch: u64,
) {
    let mut retained = Vec::with_capacity(arena.retired_allocations.len());
    for retirement in std::mem::take(&mut arena.retired_allocations) {
        if !retirement.can_release(completed_epoch) {
            retained.push(retirement);
            continue;
        }
        let bytes = retirement.owned_bytes();
        if let Some((range, capacity)) = retirement.quad {
            release_quad_range(
                &mut arena.quad_len,
                &mut arena.free_quads,
                range.start..range.start + capacity,
            );
        }
        if let Some((range, capacity)) = retirement.geometry {
            release_quad_range(
                &mut arena.geometry_stream_len,
                &mut arena.free_geometry_stream_words,
                range.start..range.start + capacity,
            );
        }
        if let Some((range, capacity)) = retirement.biome {
            release_quad_range(
                &mut arena.biome_len,
                &mut arena.free_biomes,
                range.start..range.start + capacity,
            );
        }
        if let Some(origin) = retirement.origin {
            release_origin(arena, origin);
        }
        arena.retirement_budget.release(1, bytes);
    }
    arena.retired_allocations = retained;
}
