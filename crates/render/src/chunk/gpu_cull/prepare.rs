//! Per-frame preparation: record sync, uploads, the depth pyramid and the cull uniforms.

use crate::queue_writes::ProfiledQueue as _;
use render_scene::{ChunkBiomeTints, ChunkRenderInstance, RuntimeStageProfiler};

use bevy::{
    camera::{MainPassResolutionOverride, primitives::Frustum},
    render::{
        Extract, render_resource::TextureViewId, sync_world::RenderEntity,
        view::ViewDepthStencilTexture,
    },
};

use crate::chunk::*;

use super::{
    GpuCullFrame, GpuCullSubmission,
    kernels::{CullKernels, CullStorage, HizPyramid, PyramidBindings},
    model::{
        CullCamera, CullPhase, CullRecord, CullStream, CullViewInput, CullViewUniform, STREAM_COUNT,
    },
    slots::{CullSlots, cull_record},
};

const MIN_CAPACITY: u32 = 1024;

/// Render entities whose main-world chunk is not inherited-visible (the cave culler hides them).
#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkHiddenEntities {
    pub(super) hidden: HashSet<Entity>,
    pub(super) changed: Vec<Entity>,
    /// Moves whenever `hidden` changes; shader-pack draw lists rebuild only then.
    #[cfg_attr(not(feature = "shaderpacks"), allow(dead_code))]
    revision: u64,
}

#[cfg_attr(
    not(feature = "shaderpacks"),
    expect(dead_code, reason = "read by shader-pack draws")
)]
impl ChunkHiddenEntities {
    /// Whether the cave culler hides render entity `entity`.
    pub(in crate::chunk) fn contains(&self, entity: Entity) -> bool {
        self.hidden.contains(&entity)
    }

    /// Moves whenever the hidden set changes.
    pub(in crate::chunk) fn revision(&self) -> u64 {
        self.revision
    }
}

pub(super) struct PreparedPyramid {
    pub(super) pyramid: HizPyramid,
    pub(super) depth: TextureViewId,
    pub(super) bindings: PyramidBindings,
}
pub(super) struct PreparedCullDraws<'a> {
    pub(super) args: &'a Buffer,
    pub(super) counts: &'a Buffer,
    /// The vertex buffer every culled pipeline reads its draw's offsets from.
    pub(super) offsets: &'a Buffer,
    pub(super) capacity: u32,
    /// Per-stream draw ceilings from [`CullSlots::draw_bounds`].
    pub(super) draw_bounds: [u32; STREAM_COUNT],
    pub(super) submission: GpuCullSubmission,
}

/// The record table plus the GPU state of the culled view.
#[derive(Resource)]
pub(in crate::chunk) struct GpuCull {
    pub(super) kernels: CullKernels,
    pub(super) submission: GpuCullSubmission,
    storage: Option<CullStorage>,
    args: Option<Buffer>,
    draw_counts: Option<Buffer>,
    draw_offsets: Option<Buffer>,
    pub(super) table: CullSlots,
    pub(super) pyramid: Option<PreparedPyramid>,
    pub(super) bind_groups: Option<[wgpu::BindGroup; 2]>,
    bound_pyramid: Option<TextureViewId>,
    pub(super) prepared_view: Option<Entity>,
}

impl GpuCull {
    pub(super) fn new(device: &RenderDevice, submission: GpuCullSubmission) -> Self {
        Self {
            kernels: CullKernels::new(device.wgpu_device()),
            submission,
            storage: None,
            args: None,
            draw_counts: None,
            draw_offsets: None,
            table: CullSlots::default(),
            pyramid: None,
            bind_groups: None,
            bound_pyramid: None,
            prepared_view: None,
        }
    }

    /// Replaces storage and rewrites every record into it.
    fn install_storage(&mut self, storage: CullStorage) {
        self.args = Some(Buffer::from(storage.args.clone()));
        self.draw_counts = Some(Buffer::from(storage.draw_counts.clone()));
        self.draw_offsets = Some(Buffer::from(storage.draw_offsets.clone()));
        self.storage = Some(storage);
        self.bind_groups = None;
        self.table.mark_all_dirty();
    }

    /// Resets derived CPU records and temporal GPU visibility while retaining storage.
    pub(super) fn invalidate_publication(&mut self, device: &RenderDevice, queue: &RenderQueue) {
        self.table = default();
        self.bind_groups = None;
        self.prepared_view = None;
        if let Some(storage) = &self.storage {
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.clear_buffer(&storage.history, 0, None);
            queue.submit([encoder.finish()]);
        }
    }

    /// Whether every terrain slot fits this device's cull bindings before queueing draws.
    pub(super) fn supports_slots(device: &RenderDevice, slots: u32) -> bool {
        binding_capacity(
            &device.limits(),
            slots.max(MIN_CAPACITY),
            cull_storage_largest_buffer,
        )
        .is_some()
    }

    /// Live CPU record slots, published together after storage growth succeeds.
    pub(super) fn slot_count(&self) -> u32 {
        self.table.slot_count()
    }

    /// Prepared indirect buffers and their live-slot/capacity bounds for `view`.
    pub(super) fn prepared_draws(&self, view: Entity) -> Option<PreparedCullDraws<'_>> {
        (self.prepared_view == Some(view)).then_some(())?;
        Some(PreparedCullDraws {
            args: self.args.as_ref()?,
            counts: self.draw_counts.as_ref()?,
            offsets: self.draw_offsets.as_ref()?,
            capacity: self.storage.as_ref()?.capacity,
            draw_bounds: self.table.draw_bounds(),
            submission: self.submission,
        })
    }

    /// Samples rejection feedback without changing the visibility used for drawing.
    pub(super) fn encode_feedback(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        feedback: &super::feedback::GpuCullFeedback,
    ) {
        if let Some(storage) = &self.storage {
            feedback.encode(encoder, &storage.record_draws);
        }
    }

    pub(super) fn clear_fixed_args(&self, encoder: &mut wgpu::CommandEncoder) {
        if self.submission == GpuCullSubmission::Fixed
            && let Some(args) = &self.args
        {
            encoder.clear_buffer(args, 0, None);
        }
    }
}

/// Chunks whose inherited visibility changed since the last extract.
type VisibilityChanged = (With<ChunkRenderInstance>, Changed<InheritedVisibility>);

pub(super) fn extract_hidden_chunks(
    mut hidden: ResMut<ChunkHiddenEntities>,
    chunks: Extract<Query<(RenderEntity, &InheritedVisibility), VisibilityChanged>>,
) {
    for (entity, visibility) in &chunks {
        let changed = if visibility.get() {
            hidden.hidden.remove(&entity)
        } else {
            hidden.hidden.insert(entity)
        };
        if changed {
            hidden.changed.push(entity);
            hidden.revision += 1;
        }
    }
}

pub(super) type CullViewComponents = (
    &'static ExtractedView,
    &'static Frustum,
    Option<&'static ViewDepthStencilTexture>,
    &'static Msaa,
    Option<&'static MainPassResolutionOverride>,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_gpu_cull(
    mut cull: ResMut<GpuCull>,
    mut hidden: ResMut<ChunkHiddenEntities>,
    frame: Res<GpuCullFrame>,
    mut feedback: ResMut<super::feedback::GpuCullFeedback>,
    changed: Query<
        (Entity, &GpuChunkAllocation, Option<&ChunkRenderInstance>),
        Changed<GpuChunkAllocation>,
    >,
    mut removed: RemovedComponents<GpuChunkAllocation>,
    mut removed_instances: RemovedComponents<ChunkRenderInstance>,
    biome_tints: Res<ChunkBiomeTints>,
    views: Query<CullViewComponents>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    profiler: Option<Res<RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(RuntimeStage::IndirectPreparation));
    let cull = &mut *cull;
    let hidden = &mut *hidden;
    for entity in removed.read() {
        cull.table.remove(entity);
    }
    for entity in removed_instances.read() {
        hidden.hidden.remove(&entity);
    }
    cull.table
        .set_tint(biome_tints.table_identity(), &hidden.hidden);
    for (entity, allocation, instance) in &changed {
        let record = cull_record(allocation, instance);
        let slot = allocation.metadata_index;
        cull.table.update(
            entity,
            slot,
            allocation.tint_identity,
            record,
            &hidden.hidden,
        );
    }
    for entity in hidden.changed.drain(..) {
        cull.table.refresh_entity(entity, &hidden.hidden);
    }
    cull.table.trim();
    if upload_records(cull, &device, &queue).is_err() {
        return;
    }

    let Some(view) = frame.view else {
        return;
    };
    let Ok((extracted, frustum, depth, msaa, resolution_override)) = views.get(view.entity) else {
        return;
    };
    let depth = sampleable_depth(depth, resolution_override);
    if cull.storage.is_none() {
        return;
    }
    prepare_pyramid(&mut cull.pyramid, &cull.kernels, &device, depth, *msaa);
    let storage = cull.storage.as_ref().expect("checked above");
    let hiz_mips = cull
        .pyramid
        .as_ref()
        .map_or(0, |prepared| prepared.pyramid.mip_count());
    let depth_size = cull
        .pyramid
        .as_ref()
        .map_or([1, 1], |prepared| prepared.pyramid.depth_size);
    let input = cull_view_input(extracted, frustum, depth_size, hiz_mips);
    for phase in CullPhase::ALL {
        let uniform = CullViewUniform::new(&input, phase, cull.slot_count(), storage.capacity);
        if crate::gpu_allocation::terrain_write(
            &device,
            "terrain cull uniform upload",
            std::mem::size_of_val(&uniform) as u64,
            || storage.write_uniform(&queue, phase, &uniform),
        )
        .is_err()
        {
            return;
        }
    }
    let pyramid_id = cull.pyramid.as_ref().map(|prepared| prepared.depth);
    if cull.bind_groups.is_none() || cull.bound_pyramid != pyramid_id {
        cull.bind_groups = Some(cull.kernels.bind_groups(
            device.wgpu_device(),
            storage,
            cull.pyramid.as_ref().map(|prepared| &prepared.pyramid),
        ));
        cull.bound_pyramid = pyramid_id;
    }
    cull.prepared_view = Some(view.entity);
    feedback.prepare(
        &device,
        view.entity,
        cull.slot_count(),
        &storage.record_draws,
    );
}

/// Grows storage when the slot watermark passes it, then writes dirty records and bits.
/// Failed growth leaves storage untouched for the shared recovery path.
fn upload_records(
    cull: &mut GpuCull,
    device: &RenderDevice,
    queue: &RenderQueue,
) -> Result<(), crate::gpu_allocation::TerrainWriteError> {
    let slots = cull.table.slot_count();
    if cull
        .storage
        .as_ref()
        .is_none_or(|storage| storage.capacity < slots)
    {
        match grow_storage(device, slots) {
            Some(storage) => cull.install_storage(storage),
            None => return Err(crate::gpu_allocation::TerrainWriteError),
        }
    }
    let Some(storage) = cull.storage.as_ref() else {
        return Err(crate::gpu_allocation::TerrainWriteError);
    };
    write_dirty_records(&mut cull.table, device, queue, &storage.records)?;
    if cull.table.take_enabled_dirty() {
        let enabled = cull.table.enabled();
        let words = enabled.len().min((storage.capacity as usize).div_ceil(32));
        if words != 0 {
            #[cfg(feature = "tracy")]
            let _span = bevy::log::info_span!(
                "terrain.cull_enabled_write",
                bytes = words * std::mem::size_of::<u32>()
            )
            .entered();
            crate::gpu_allocation::terrain_write(
                device,
                "terrain cull enabled upload",
                words as u64 * 4,
                || {
                    queue.profiled_write_buffer(
                        &storage.enabled,
                        0,
                        bytemuck::cast_slice(&enabled[..words]),
                    )
                },
            )?;
        }
    }
    Ok(())
}

/// Storage for `slots`, a power of two where one storage binding allows; `None` when no
/// capacity holding them fits the device's binding limit or its memory.
fn grow_storage(device: &RenderDevice, slots: u32) -> Option<CullStorage> {
    let capacity = binding_capacity(
        &device.limits(),
        slots.max(MIN_CAPACITY),
        cull_storage_largest_buffer,
    )?;
    crate::gpu_allocation::terrain_checked(
        device,
        "terrain cull storage",
        cull_storage_largest_buffer(capacity),
        || CullStorage::new(device.wgpu_device(), capacity, cfg!(test)),
    )
}

/// The largest cull storage buffer at `capacity`; it must fit one storage binding.
fn cull_storage_largest_buffer(capacity: u32) -> u64 {
    (u64::from(capacity) * std::mem::size_of::<CullRecord>() as u64)
        .max(super::model::args_words(capacity) * 4)
        .max(super::model::draw_offset_bytes(capacity))
}

/// `minimum` rounded up to a power of two, or `minimum` itself when only that fits one
/// storage binding; `None` when even `minimum` does not.
pub(super) fn binding_capacity(
    limits: &wgpu::Limits,
    minimum: u32,
    largest_buffer_bytes: impl Fn(u32) -> u64,
) -> Option<u32> {
    let binding = limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size);
    [minimum.checked_next_power_of_two(), Some(minimum)]
        .into_iter()
        .flatten()
        .find(|&capacity| largest_buffer_bytes(capacity) <= binding)
}

/// The view's depth when a pyramid can seed from it at full resolution.
pub(super) fn sampleable_depth<'a>(
    depth: Option<&'a ViewDepthStencilTexture>,
    resolution_override: Option<&MainPassResolutionOverride>,
) -> Option<&'a ViewDepthStencilTexture> {
    depth.filter(|depth| {
        resolution_override.is_none()
            && depth
                .texture()
                .usage()
                .contains(TextureUsages::TEXTURE_BINDING)
    })
}

/// Writes every dirty CPU record in contiguous runs after storage capacity is ensured.
pub(super) fn write_dirty_records(
    table: &mut CullSlots,
    device: &RenderDevice,
    queue: &RenderQueue,
    records: &wgpu::Buffer,
) -> Result<(), crate::gpu_allocation::TerrainWriteError> {
    let record_bytes = std::mem::size_of::<CullRecord>() as u64;
    let dirty = table.take_dirty();
    let source = table.records();
    for run in dirty.chunk_by(|left, right| left + 1 == *right) {
        let (first, last) = (run[0] as usize, run[run.len() - 1] as usize);
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "terrain.cull_records_write",
            first,
            records = run.len(),
            bytes = run.len() as u64 * record_bytes,
        )
        .entered();
        crate::gpu_allocation::terrain_write(
            device,
            "terrain cull records upload",
            run.len() as u64 * record_bytes,
            || {
                queue.profiled_write_buffer(
                    records,
                    first as u64 * record_bytes,
                    bytemuck::cast_slice(&source[first..=last]),
                )
            },
        )?;
    }
    Ok(())
}

/// Keeps `prepared` seeded from `depth`, reusing the pyramid while the target size holds.
pub(super) fn prepare_pyramid(
    prepared: &mut Option<PreparedPyramid>,
    kernels: &CullKernels,
    device: &RenderDevice,
    depth: Option<&ViewDepthStencilTexture>,
    msaa: Msaa,
) {
    let Some(depth) = depth else {
        *prepared = None;
        return;
    };
    let size = depth.texture().size();
    let depth_size = [size.width, size.height];
    let view = crate::scene_sampling::view_depth(depth);
    if prepared.as_ref().is_some_and(|prepared| {
        prepared.depth == view.id() && prepared.pyramid.depth_size == depth_size
    }) {
        return;
    }
    let pyramid = prepared
        .take()
        .map(|prepared| prepared.pyramid)
        .filter(|pyramid| pyramid.depth_size == depth_size)
        .unwrap_or_else(|| HizPyramid::new(device.wgpu_device(), depth_size));
    let bindings =
        kernels.pyramid_bindings(device.wgpu_device(), view, msaa.samples() > 1, &pyramid);
    *prepared = Some(PreparedPyramid {
        pyramid,
        depth: view.id(),
        bindings,
    });
}

pub(super) fn cull_view_input(
    view: &ExtractedView,
    frustum: &Frustum,
    depth_size: [u32; 2],
    hiz_mips: u32,
) -> CullViewInput {
    let world_from_view = view.world_from_view.to_matrix().as_dmat4();
    let clip_from_world = view
        .clip_from_world
        .map(|matrix| matrix.as_dmat4())
        .unwrap_or_else(|| view.clip_from_view.as_dmat4() * world_from_view.inverse());
    let eye = pipeline::solid::solid_cull_camera(view, false);
    CullViewInput {
        planes: std::array::from_fn(|index| frustum.half_spaces[index].normal_d().to_array()),
        clip_from_world: clip_from_world.to_cols_array_2d(),
        camera: CullCamera::new(eye),
        viewport: view.viewport.as_vec4().to_array(),
        depth_size,
        hiz_mips,
        index_counts: CullStream::ALL.map(|stream| match stream {
            CullStream::Model => MODEL_INDEX_COUNT,
            _ => STATIC_QUAD_INDICES.len() as u32,
        }),
    }
}

#[cfg(test)]
mod storage_tests {
    use super::*;

    impl GpuCull {
        /// Reads the records and enabled bits that this publication's compute cull consumes.
        pub(in crate::chunk::gpu_cull) fn publication_snapshot(
            &self,
            device: &RenderDevice,
            queue: &RenderQueue,
        ) -> Vec<Vec<u8>> {
            let Some(storage) = &self.storage else {
                return Vec::new();
            };
            let slots = u64::from(self.slot_count());
            let read = crate::render_test_support::read_buffer;
            vec![
                read(
                    device,
                    queue,
                    &storage.records,
                    0,
                    slots * std::mem::size_of::<CullRecord>() as u64,
                ),
                read(device, queue, &storage.enabled, 0, slots.div_ceil(32) * 4),
            ]
        }
    }

    /// Capacity rounds up to a power of two only while every buffer fits one binding.
    #[test]
    fn capacity_stays_within_one_storage_binding() {
        let limits = |binding: u64| wgpu::Limits {
            max_storage_buffer_binding_size: binding,
            max_buffer_size: binding,
            ..wgpu::Limits::default()
        };
        let bytes = |capacity: u32| u64::from(capacity) * 10;
        assert_eq!(binding_capacity(&limits(10_240), 1000, bytes), Some(1024));
        assert_eq!(binding_capacity(&limits(10_000), 1000, bytes), Some(1000));
        assert_eq!(binding_capacity(&limits(9_990), 1000, bytes), None);
    }

    /// Storage that cannot grow writes nothing and retries.
    #[test]
    fn storage_without_memory_waits_until_it_grows() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let mut cull = GpuCull::new(&device, GpuCullSubmission::Fixed);
        {
            let _failure =
                crate::gpu_allocation::inject(|label, _| label == "terrain cull storage");
            assert!(upload_records(&mut cull, &device, &queue).is_err());
        }
        assert!(cull.storage.is_none());
        assert!(upload_records(&mut cull, &device, &queue).is_ok());
        assert!(cull.storage.is_some());
    }

    /// Failed growth keeps old storage without publishing a prefix of the CPU table.
    #[test]
    fn failed_growth_keeps_storage_without_publishing_partial_records() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let (device, queue) = (RenderDevice::from(device), RenderQueue::new(queue));
        let mut cull = GpuCull::new(&device, GpuCullSubmission::Fixed);
        upload_records(&mut cull, &device, &queue).unwrap();
        let capacity = cull.storage.as_ref().expect("initial storage").capacity;
        cull.table.update(
            Entity::PLACEHOLDER,
            capacity + 10,
            ChunkBiomeTintIdentity::default(),
            CullRecord::default(),
            &HashSet::new(),
        );
        {
            let _failure =
                crate::gpu_allocation::inject(|label, _| label == "terrain cull storage");
            assert!(upload_records(&mut cull, &device, &queue).is_err());
        }
        let held = cull.storage.as_ref().map(|storage| storage.capacity);
        assert_eq!(held, Some(capacity));
        assert_eq!(cull.slot_count(), capacity + 11);
        assert!(upload_records(&mut cull, &device, &queue).is_ok());
        assert_eq!(cull.slot_count(), capacity + 11);
    }
}
