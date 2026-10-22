use crate::chunk::*;
use render_scene::{ChunkRenderInstance, ChunkUploadPriority};

mod free_ranges;
mod priority;

pub(in crate::chunk) use free_ranges::FreeRanges;

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
    pub(in crate::chunk) migrations_started: usize,
    pub(in crate::chunk) uploads_waiting_for_growth: usize,
    pub(in crate::chunk) candidates_examined: usize,
    pub(in crate::chunk) model_streams_validated: usize,
    pub(in crate::chunk) range_plans: usize,
    pub(in crate::chunk) candidates_deferred: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct OriginAllocationPlan {
    pub(in crate::chunk) projected_origin_len: usize,
}

/// Plans one origin allocation without mutating the arena.
///
/// Fresh allocations preserve one available origin slot for a later
/// copy-on-write replacement whenever the adapter can hold both the fresh
/// allocation and that reserve. A one-entry adapter has no room for that
/// reserve, so its single fresh allocation is allowed but cannot be replaced
/// until its origin is released.
pub(in crate::chunk) fn plan_origin_allocation(
    origin_len: usize,
    free_origin_count: usize,
    max_origin_items: usize,
    replacing: bool,
) -> Option<OriginAllocationPlan> {
    if origin_len > max_origin_items || origin_len > u32::MAX as usize {
        return None;
    }
    let available_slots = free_origin_count.saturating_add(max_origin_items - origin_len);
    let reserved_slots = usize::from(!replacing && max_origin_items > 1);
    if available_slots <= reserved_slots {
        return None;
    }
    Some(OriginAllocationPlan {
        projected_origin_len: origin_len.saturating_add(usize::from(free_origin_count == 0)),
    })
}

#[derive(Resource)]
pub(in crate::chunk) struct ChunkGpuArena {
    pub(in crate::chunk) quad_buffer: Buffer,
    pub(in crate::chunk) geometry_stream_buffer: Buffer,
    pub(in crate::chunk) origin_buffer: Buffer,
    pub(in crate::chunk) biome_buffer: Buffer,
    pub(in crate::chunk) index_buffer: Buffer,
    pub(in crate::chunk) model_index_buffer: Buffer,
    /// Vertex buffer 0 of CPU-planned terrain draws; see [`identity_vertex_offsets`].
    pub(in crate::chunk) vertex_offset_buffer: Buffer,
    pub(in crate::chunk) indirect_buffer: Buffer,
    pub(in crate::chunk) transparent_indirect_buffer: Buffer,
    pub(in crate::chunk) transparent_ref_buffer: Buffer,
    /// Per-slot capacity of `transparent_ref_buffer`, which is also the second slot's offset.
    pub(in crate::chunk) transparent_slot_refs: usize,
    pub(in crate::chunk) bind_group: Option<BindGroup>,
    pub(in crate::chunk) transparent_bind_group: Option<BindGroup>,
    pub(in crate::chunk) bind_group_buffers: Option<ChunkBindGroupBuffers>,
    pub(in crate::chunk) quad_capacity: usize,
    pub(in crate::chunk) geometry_stream_capacity: usize,
    pub(in crate::chunk) origin_capacity: usize,
    pub(in crate::chunk) biome_capacity: usize,
    pub(in crate::chunk) indirect_capacity: usize,
    /// What `indirect_buffer` currently holds, so an unchanged frame skips its write.
    pub(in crate::chunk) uploaded_indirect_bytes: Vec<u8>,
    pub(in crate::chunk) quad_len: usize,
    pub(in crate::chunk) geometry_stream_len: usize,
    pub(in crate::chunk) origin_len: usize,
    pub(in crate::chunk) biome_len: usize,
    pub(in crate::chunk) limits: ArenaLimits,
    pub(in crate::chunk) free_quads: FreeRanges,
    pub(in crate::chunk) free_geometry_stream_words: FreeRanges,
    pub(in crate::chunk) free_origins: Vec<u32>,
    pub(in crate::chunk) free_biomes: FreeRanges,
    pub(in crate::chunk) shared_biome_ranges: SharedBiomeRanges,
    pub(in crate::chunk) allocations: HashMap<Entity, ArenaAllocation>,
    /// The transparent water of `allocations`, updated wherever an allocation changes.
    pub(in crate::chunk) transparent_liquids: TransparentLiquidResidents,
    pub(in crate::chunk) retired_allocations: Vec<RetiredArenaAllocation>,
    pub(in crate::chunk) pending_removals: BTreeSet<Entity>,
    pub(in crate::chunk) retirement_budget: TransparentRetirementBudget,
    pub(in crate::chunk) migration: Option<ArenaMigration>,
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
            device_limits.max_storage_buffer_binding_size,
        );
        let arena = Self {
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
            vertex_offset_buffer: identity_vertex_offsets(render_device, 1),
            indirect_buffer: create_indirect_buffer(render_device, 1),
            transparent_indirect_buffer: create_indirect_buffer(render_device, 1),
            transparent_ref_buffer: transparent_ref_buffer(
                render_device,
                INITIAL_TRANSPARENT_SLOT_REFS,
            ),
            transparent_slot_refs: INITIAL_TRANSPARENT_SLOT_REFS,
            bind_group: None,
            transparent_bind_group: None,
            bind_group_buffers: None,
            quad_capacity: 1,
            geometry_stream_capacity: 1,
            origin_capacity: 1,
            biome_capacity: FALLBACK_BIOME_WORDS,
            indirect_capacity: 1,
            uploaded_indirect_bytes: Vec::new(),
            quad_len: 0,
            geometry_stream_len: 0,
            origin_len: 0,
            biome_len: FALLBACK_BIOME_WORDS,
            limits,
            free_quads: FreeRanges::default(),
            free_geometry_stream_words: FreeRanges::default(),
            free_origins: Vec::new(),
            free_biomes: FreeRanges::default(),
            shared_biome_ranges: SharedBiomeRanges::default(),
            allocations: HashMap::new(),
            transparent_liquids: TransparentLiquidResidents::default(),
            retired_allocations: Vec::new(),
            pending_removals: BTreeSet::new(),
            retirement_budget: TransparentRetirementBudget::with_limits(
                MAX_TRANSPARENT_RETIRED_ALLOCATIONS,
                MAX_TRANSPARENT_RETIRED_BYTES,
            ),
            migration: None,
        };
        super::telemetry::log_initial_arena_capacity(&arena, render_device);
        arena
    }
}

/// Vertex buffer 0 for CPU-planned terrain draws over `origin_capacity` origin slots: entry `i`
/// is `(i, 0)`, so fetching at a draw's base vertex yields its vertex index while the builtin
/// instance index keeps its first instance. GPU-culled draws bind the cull's draw offsets instead.
pub(in crate::chunk) fn identity_vertex_offsets(
    render_device: &RenderDevice,
    origin_capacity: usize,
) -> Buffer {
    let corners = crate::chunk::gpu_cull::model::OFFSET_CORNERS;
    let entries = (0..origin_capacity as u32 * corners)
        .flat_map(|vertex| [vertex, 0])
        .collect::<Vec<u32>>();
    render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("terrain identity vertex offsets"),
        contents: bytemuck::cast_slice(&entries),
        usage: BufferUsages::VERTEX,
    })
}

fn storage_buffer_descriptor(label: &'static str, size: u64) -> BufferDescriptor<'static> {
    BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    }
}

pub(in crate::chunk) fn create_storage_buffer(
    render_device: &RenderDevice,
    label: &'static str,
    size: u64,
) -> Buffer {
    render_device.create_buffer(&storage_buffer_descriptor(label, size))
}

/// [`create_storage_buffer`], or `None` when the device is out of memory.
pub(in crate::chunk) fn try_create_storage_buffer(
    render_device: &RenderDevice,
    label: &'static str,
    size: u64,
) -> Option<Buffer> {
    crate::gpu_allocation::buffer(render_device, &storage_buffer_descriptor(label, size))
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
    pub(in crate::chunk) priority: ChunkUploadPriority,
}

impl GpuUpdateCandidate {
    pub(in crate::chunk) fn from_instance(entity: Entity, instance: &ChunkRenderInstance) -> Self {
        Self {
            entity,
            key: instance.key,
            generation: instance.generation,
            tint_identity: instance.tint_identity,
            priority: instance.priority,
        }
    }
}

pub(in crate::chunk) const MAX_GPU_UPDATE_FAIRNESS_ENTRIES: usize = 65_536;
pub(in crate::chunk) const GPU_UPDATE_OVERDUE_FRAMES: u32 = 2;
/// An update waiting this long goes before every nearer one, oldest first, so a backlog that
/// keeps refilling near the camera still serves it; younger overdue updates go nearest first.
pub(in crate::chunk) const GPU_UPDATE_STARVED_FRAMES: u32 = 60;

#[derive(Resource)]
pub(in crate::chunk) struct GpuUpdateFairness {
    pub(in crate::chunk) wait_ages: HashMap<Entity, u32>,
    pub(in crate::chunk) urgent_waiters: HashSet<Entity>,
    pub(in crate::chunk) limit: usize,
    pub(in crate::chunk) recover_untracked: bool,
    pub(in crate::chunk) last_tint_identity: Option<ChunkBiomeTintIdentity>,
    pub(in crate::chunk) model_validation: super::upload::model_validation::ModelValidationCache,
    examination_cursor: usize,
    priority_frame: u64,
    priorities: Mutex<priority::GpuUpdatePriorities>,
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
            urgent_waiters: HashSet::new(),
            last_tint_identity: None,
            recover_untracked: false,
            limit,
            model_validation: Default::default(),
            examination_cursor: 0,
            priority_frame: 0,
            priorities: Mutex::default(),
        }
    }

    pub(in crate::chunk) fn wait_age(&self, entity: Entity) -> u32 {
        self.wait_ages.get(&entity).copied().unwrap_or(0)
    }

    pub(in crate::chunk) fn is_urgent(&self, entity: Entity) -> bool {
        self.urgent_waiters.contains(&entity)
    }

    /// Examines a priority prefix and a rotating tail, each bounded by the service item ceiling.
    pub(in crate::chunk) fn examination_order<'a>(
        &mut self,
        selected: &'a [Entity],
    ) -> impl Iterator<Item = Entity> + use<'a> {
        let limit = PublicationServiceConfig::DEFAULT.maximum_frame_items;
        let (priority, tail) = selected.split_at(selected.len().min(limit));
        let start = self.examination_cursor % tail.len().max(1);
        let count = tail.len().min(limit);
        self.examination_cursor = (start + count) % tail.len().max(1);
        let (before, after) = tail.split_at(start);
        priority
            .iter()
            .chain(after.iter().chain(before).take(count))
            .copied()
    }

    pub(in crate::chunk) fn finish_frame(
        &mut self,
        active: &[Entity],
        successful: &[Entity],
        urgent: &[Entity],
    ) {
        self.priority_frame = self.priority_frame.saturating_add(1);
        let active_set = active.iter().copied().collect::<HashSet<_>>();
        let successful = successful.iter().copied().collect::<HashSet<_>>();
        self.wait_ages
            .retain(|entity, _| active_set.contains(entity));
        self.urgent_waiters
            .retain(|entity| active_set.contains(entity));
        for &entity in &successful {
            self.wait_ages.remove(&entity);
            self.urgent_waiters.remove(&entity);
        }
        self.recover_untracked = false;
        for &entity in active.iter().filter(|entity| !successful.contains(entity)) {
            if let Some(age) = self.wait_ages.get_mut(&entity) {
                *age = age.saturating_add(1);
            } else if self.wait_ages.len() < self.limit {
                self.wait_ages.insert(entity, 1);
            } else {
                self.recover_untracked = true;
            }
        }
        for &entity in urgent.iter().filter(|entity| !successful.contains(entity)) {
            if self.wait_ages.contains_key(&entity) {
                self.urgent_waiters.insert(entity);
            }
        }
        self.model_validation.retain_waiters(&self.wait_ages);
    }

    #[cfg(test)]
    pub(in crate::chunk) fn reset(&mut self) {
        self.wait_ages.clear();
        self.urgent_waiters.clear();
        self.last_tint_identity = None;
        self.recover_untracked = false;
        self.model_validation = Default::default();
        self.examination_cursor = 0;
        self.priority_frame = 0;
        self.priorities = Mutex::default();
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

/// Reconciles retained priorities without changing admission or the bounded examination order.
pub(in crate::chunk) fn plan_gpu_chunk_updates(
    candidates: Vec<GpuUpdateCandidate>,
    allocations: &HashMap<Entity, ArenaAllocation>,
    camera_position: Vec3,
    active_tint_identity: ChunkBiomeTintIdentity,
    fairness: &GpuUpdateFairness,
) -> Vec<Entity> {
    priority::plan_updates(
        candidates,
        allocations,
        camera_position,
        active_tint_identity,
        fairness,
    )
}

/// Ranges for a fresh allocation, found without copying the free lists; the
/// arena is only mutated by [`commit_fresh_chunk_ranges`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct FreshChunkRanges {
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
    pub(in crate::chunk) geometry_stream_len: usize,
    pub(in crate::chunk) biome_len: usize,
}

/// Where a fresh best-fit allocation of `required` would land and the
/// resulting arena length.
fn probe_range(
    len: usize,
    free: &FreeRanges,
    required: u32,
    max_items: usize,
) -> Option<(u32, usize)> {
    if required == 0 {
        return Some((0, len));
    }
    if let Some((_, start)) = free.best_fit(required, 1) {
        return Some((start, len));
    }
    let next = len.checked_add(required as usize)?;
    if next > max_items || len > u32::MAX as usize {
        return None;
    }
    Some((len as u32, next))
}

fn probe_aligned_range(
    len: usize,
    free: &FreeRanges,
    required: u32,
    max_items: usize,
    alignment: u32,
) -> Option<(u32, usize)> {
    if required == 0 {
        return Some((0, len));
    }
    if let Some((_, start)) = free.best_fit(required, alignment) {
        return Some((start, len));
    }
    let start = checked_align_up(u32::try_from(len).ok()?, alignment)?;
    let end = start.checked_add(required)?;
    if end as usize > max_items {
        return None;
    }
    Some((start, end as usize))
}

/// Plans a fresh allocation by probing the free lists in place; the arena is
/// only mutated when [`commit_fresh_chunk_ranges`] replays the same choice.
pub(in crate::chunk) fn plan_fresh_chunk_ranges(
    arena: &ChunkGpuArena,
    required: GeometryStreamCounts,
    biome_required: u32,
) -> Option<FreshChunkRanges> {
    let limits = arena.limits;
    let (quad_start, quad_len) = probe_range(
        arena.quad_len,
        &arena.free_quads,
        required.cube,
        limits.max_quad_items,
    )?;
    let layout = required.layout()?;
    let (geometry_stream_start, geometry_stream_len) = probe_aligned_range(
        arena.geometry_stream_len,
        &arena.free_geometry_stream_words,
        layout.word_count,
        limits.max_geometry_stream_words,
        SHARED_GEOMETRY_ALIGNMENT_WORDS,
    )?;
    let (biome_start, biome_len) = probe_range(
        arena.biome_len,
        &arena.free_biomes,
        biome_required,
        limits.max_biome_words,
    )?;
    Some(FreshChunkRanges {
        quad_start,
        quad_capacity: required.cube,
        geometry_stream_start,
        geometry_stream_capacity: layout.word_count,
        model_start: geometry_stream_start.checked_add(layout.model_offset)?,
        model_lighting_start: geometry_stream_start.checked_add(layout.model_lighting_offset)?,
        model_draw_start: geometry_stream_start.checked_add(layout.model_draw_offset)?,
        transparent_model_draw_start: geometry_stream_start
            .checked_add(layout.transparent_model_draw_offset)?,
        liquid_start: geometry_stream_start.checked_add(layout.liquid_offset)?,
        liquid_lighting_start: geometry_stream_start.checked_add(layout.liquid_lighting_offset)?,
        cube_lighting_start: geometry_stream_start.checked_add(layout.cube_lighting_offset)?,
        biome_start,
        biome_capacity: biome_required,
        quad_len,
        geometry_stream_len,
        biome_len,
    })
}

/// Performs the allocation [`plan_fresh_chunk_ranges`] probed.
pub(in crate::chunk) fn commit_fresh_chunk_ranges(
    arena: &mut ChunkGpuArena,
    ranges: &FreshChunkRanges,
) {
    let limits = arena.limits;
    let quad = allocate_range_for_update(
        &mut arena.quad_len,
        &mut arena.free_quads,
        ranges.quad_capacity,
        None,
        limits.max_quad_items,
        0,
    );
    let geometry = allocate_aligned_range_for_update(
        &mut arena.geometry_stream_len,
        &mut arena.free_geometry_stream_words,
        ranges.geometry_stream_capacity,
        None,
        limits.max_geometry_stream_words,
        0,
        SHARED_GEOMETRY_ALIGNMENT_WORDS,
    );
    let biome = allocate_range_for_update(
        &mut arena.biome_len,
        &mut arena.free_biomes,
        ranges.biome_capacity,
        None,
        limits.max_biome_words,
        0,
    );
    debug_assert_eq!(quad, Some((ranges.quad_start, ranges.quad_capacity)));
    debug_assert_eq!(
        geometry,
        Some((
            ranges.geometry_stream_start,
            ranges.geometry_stream_capacity
        ))
    );
    debug_assert_eq!(biome, Some((ranges.biome_start, ranges.biome_capacity)));
    debug_assert_eq!(
        (arena.quad_len, arena.geometry_stream_len, arena.biome_len),
        (
            ranges.quad_len,
            ranges.geometry_stream_len,
            ranges.biome_len
        )
    );
}

pub(in crate::chunk) fn checked_geometry_range(start: u32, count: u32) -> Option<Range<u32>> {
    if count == 0 {
        return None;
    }
    start.checked_add(count).map(|end| start..end)
}

#[cfg(test)]
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
    pub(in crate::chunk) free_quads: FreeRanges,
    pub(in crate::chunk) geometry_stream_len: usize,
    pub(in crate::chunk) free_geometry_stream_words: FreeRanges,
    pub(in crate::chunk) biome_len: usize,
    pub(in crate::chunk) free_biomes: FreeRanges,
}

#[cfg(test)]
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
    let mut free_quads = current_free_quads.iter().cloned().collect();
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
    let mut free_geometry_stream_words =
        current_free_geometry_stream_words.iter().cloned().collect();
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

    let mut free_biomes = current_free_biomes.iter().cloned().collect();
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
    free: &mut FreeRanges,
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
    free: &mut FreeRanges,
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
    free: &mut FreeRanges,
    required: u32,
    max_items: usize,
    alignment: u32,
) -> Option<u32> {
    if let Some(start) = free.take(required, alignment) {
        return Some(start);
    }

    let current = u32::try_from(*len).ok()?;
    let start = checked_align_up(current, alignment)?;
    let end = start.checked_add(required)?;
    if end as usize > max_items {
        return None;
    }
    free.insert(current..start);
    *len = end as usize;
    Some(start)
}

pub(in crate::chunk) fn allocate_quad_range(
    len: &mut usize,
    free: &mut FreeRanges,
    required: u32,
    max_items: usize,
) -> Option<u32> {
    if let Some(start) = free.take(required, 1) {
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
    free: &mut FreeRanges,
    freed: Range<u32>,
) {
    free.insert(freed);
    free.trim_tail(len);
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

/// Biome ranges that more than one generation holds, keyed by range start. A generation whose
/// record is unchanged keeps its predecessor's range, and either may still be drawn until its own
/// retirement releases, so the range is reusable only once its last holder has released it.
#[derive(Default)]
pub(in crate::chunk) struct SharedBiomeRanges(HashMap<u32, u32>);

impl SharedBiomeRanges {
    /// Records one more generation holding the range starting at `start`.
    pub(in crate::chunk) fn share(&mut self, start: u32) {
        *self.0.entry(start).or_insert(0) += 1;
    }

    /// Drops one holder of the range starting at `start`; true when it was the last.
    pub(in crate::chunk) fn release(&mut self, start: u32) -> bool {
        let Some(extra) = self.0.get_mut(&start) else {
            return true;
        };
        *extra -= 1;
        if *extra == 0 {
            self.0.remove(&start);
        }
        false
    }
}

pub(in crate::chunk) fn release_completed_transparent_retirements(
    arena: &mut ChunkGpuArena,
    completed_epoch: u64,
) {
    #[cfg(feature = "tracy")]
    let _zone = bevy::log::info_span!(
        "chunk.resource_retirement",
        pending = arena.retired_allocations.len(),
        completed_epoch,
    )
    .entered();
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
        if let Some((range, capacity)) = retirement.biome
            && arena.shared_biome_ranges.release(range.start)
        {
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
