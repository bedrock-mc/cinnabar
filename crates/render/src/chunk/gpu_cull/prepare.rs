//! Per-frame preparation: record sync, uploads, the depth pyramid and the cull uniforms.

use bevy::{
    camera::{MainPassResolutionOverride, primitives::Frustum},
    render::{
        Extract, render_resource::TextureViewId, sync_world::RenderEntity, view::ViewDepthTexture,
    },
};

use crate::chunk::*;

use super::{
    GpuCullFrame,
    kernels::{CullKernels, CullStorage, HizPyramid, PyramidBindings},
    model::{CullCamera, CullPhase, CullRecord, CullStream, CullViewInput, CullViewUniform},
    slots::{CullSlots, cull_record},
};

const MIN_CAPACITY: u32 = 1024;

/// Render entities whose main-world chunk is not inherited-visible (the cave culler hides them).
#[derive(Resource, Default)]
pub(in crate::chunk) struct ChunkHiddenEntities {
    pub(super) hidden: HashSet<Entity>,
    pub(super) changed: Vec<Entity>,
}

pub(super) struct PreparedPyramid {
    pub(super) pyramid: HizPyramid,
    pub(super) depth: TextureViewId,
    pub(super) bindings: PyramidBindings,
}

/// The record table plus the GPU state of the culled view.
#[derive(Resource)]
pub(in crate::chunk) struct GpuCull {
    pub(super) kernels: CullKernels,
    storage: Option<CullStorage>,
    args: Option<Buffer>,
    draw_counts: Option<Buffer>,
    pub(super) table: CullSlots,
    pub(super) pyramid: Option<PreparedPyramid>,
    pub(super) bind_groups: Option<[wgpu::BindGroup; 2]>,
    bound_pyramid: Option<TextureViewId>,
    pub(super) prepared_view: Option<Entity>,
}

impl GpuCull {
    pub(super) fn new(device: &RenderDevice) -> Self {
        Self {
            kernels: CullKernels::new(device.wgpu_device()),
            storage: None,
            args: None,
            draw_counts: None,
            table: CullSlots::default(),
            pyramid: None,
            bind_groups: None,
            bound_pyramid: None,
            prepared_view: None,
        }
    }

    pub(super) fn slot_count(&self) -> u32 {
        self.table.slot_count()
    }

    /// Args, counts and slot capacity when `view` was prepared this frame.
    pub(in crate::chunk) fn prepared_draws(&self, view: Entity) -> Option<(&Buffer, &Buffer, u32)> {
        (self.prepared_view == Some(view)).then_some(())?;
        Some((
            self.args.as_ref()?,
            self.draw_counts.as_ref()?,
            self.storage.as_ref()?.capacity,
        ))
    }
}

type HiddenChunkComponents = (RenderEntity, &'static InheritedVisibility);
type HiddenChunkFilter = (With<ChunkRenderInstance>, Changed<InheritedVisibility>);

pub(super) fn extract_hidden_chunks(
    mut hidden: ResMut<ChunkHiddenEntities>,
    chunks: Extract<Query<HiddenChunkComponents, HiddenChunkFilter>>,
) {
    for (entity, visibility) in &chunks {
        let changed = if visibility.get() {
            hidden.hidden.remove(&entity)
        } else {
            hidden.hidden.insert(entity)
        };
        if changed {
            hidden.changed.push(entity);
        }
    }
}

pub(super) type CullViewComponents = (
    &'static ExtractedView,
    &'static Frustum,
    Option<&'static ViewDepthTexture>,
    &'static Msaa,
    Option<&'static MainPassResolutionOverride>,
);

#[allow(clippy::too_many_arguments)]
pub(super) fn prepare_gpu_cull(
    mut cull: ResMut<GpuCull>,
    mut hidden: ResMut<ChunkHiddenEntities>,
    frame: Res<GpuCullFrame>,
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
    upload_records(cull, &device, &queue);

    let Some(view) = frame.view else {
        return;
    };
    let Ok((extracted, frustum, depth, msaa, resolution_override)) = views.get(view.entity) else {
        return;
    };
    let depth = sampleable_depth(depth, resolution_override);
    prepare_pyramid(&mut cull.pyramid, &cull.kernels, &device, depth, *msaa);
    let storage = cull.storage.as_ref().expect("records were uploaded");
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
        storage.write_uniform(&queue, phase, &uniform);
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
}

/// Grows storage when the slot watermark passes it, then writes dirty records and bits.
fn upload_records(cull: &mut GpuCull, device: &RenderDevice, queue: &RenderQueue) {
    let slots = cull.slot_count();
    if cull
        .storage
        .as_ref()
        .is_none_or(|storage| storage.capacity < slots)
    {
        let capacity = slots.max(MIN_CAPACITY).next_power_of_two();
        let storage = CullStorage::new(device.wgpu_device(), capacity, false);
        cull.args = Some(Buffer::from(storage.args.clone()));
        cull.draw_counts = Some(Buffer::from(storage.draw_counts.clone()));
        cull.storage = Some(storage);
        cull.bind_groups = None;
        cull.table.mark_all_dirty();
    }
    let storage = cull.storage.as_ref().expect("storage was just ensured");
    write_dirty_records(&mut cull.table, queue, &storage.records);
    if cull.table.take_enabled_dirty() {
        let enabled = cull.table.enabled();
        let words = enabled.len().min((storage.capacity as usize).div_ceil(32));
        if words != 0 {
            queue.write_buffer(&storage.enabled, 0, bytemuck::cast_slice(&enabled[..words]));
        }
    }
}

/// The view's depth when a pyramid can seed from it at full resolution.
pub(super) fn sampleable_depth<'a>(
    depth: Option<&'a ViewDepthTexture>,
    resolution_override: Option<&MainPassResolutionOverride>,
) -> Option<&'a ViewDepthTexture> {
    depth.filter(|depth| {
        resolution_override.is_none()
            && depth
                .texture
                .usage()
                .contains(TextureUsages::TEXTURE_BINDING)
    })
}

/// Writes the table's dirty records into `records` in contiguous runs.
pub(super) fn write_dirty_records(
    table: &mut CullSlots,
    queue: &RenderQueue,
    records: &wgpu::Buffer,
) {
    let record_bytes = std::mem::size_of::<CullRecord>() as u64;
    let dirty = table.take_dirty();
    let source = table.records();
    for run in dirty.chunk_by(|left, right| left + 1 == *right) {
        let (first, last) = (run[0] as usize, run[run.len() - 1] as usize);
        queue.write_buffer(
            records,
            first as u64 * record_bytes,
            bytemuck::cast_slice(&source[first..=last]),
        );
    }
}

/// Keeps `prepared` seeded from `depth`, reusing the pyramid while the target size holds.
pub(super) fn prepare_pyramid(
    prepared: &mut Option<PreparedPyramid>,
    kernels: &CullKernels,
    device: &RenderDevice,
    depth: Option<&ViewDepthTexture>,
    msaa: Msaa,
) {
    let Some(depth) = depth else {
        *prepared = None;
        return;
    };
    let size = depth.texture.size();
    let depth_size = [size.width, size.height];
    let view = depth.view();
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
