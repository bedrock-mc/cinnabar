//! GPU ownership and dirty uploads; growth preserves old slots with a GPU copy and splits
//! arenas at the device storage-binding limit.
use super::{PrimitiveShapesScene, mesh};
use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        settings::WgpuLimits,
    },
};
use render_api::primitive_shapes::PrimitiveShapeKind;
use render_model::{
    NAMETAG_ATLAS_SIDE, NametagAtlasRect,
    primitive_shapes::{PrimitiveActor, PrimitiveInstance, PrimitiveMeshKey, PrimitiveTextRecord},
};
use std::sync::Arc;

pub(super) const INSTANCE_BYTES: u64 = std::mem::size_of::<PrimitiveInstance>() as u64;
pub(super) const ACTOR_BYTES: u64 = std::mem::size_of::<PrimitiveActor>() as u64;
pub(super) const TEXT_BYTES: u64 = std::mem::size_of::<PrimitiveTextRecord>() as u64;

/// Stable slots split across buffers that each fit one storage binding; slot `s` lives in
/// chunk `s / chunk_slots`. Arenas indexed across draws are capped at one chunk.
pub(super) struct Arena {
    pub chunks: Vec<Buffer>,
    last_capacity: usize,
    bytes: u64,
    chunk_slots: usize,
    max_chunks: usize,
}

/// Allocates a storage buffer; contents are initialized by dirty writes.
fn slot_buffer(device: &RenderDevice, bytes: u64, slots: usize) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("primitive shape slots"),
        size: bytes * slots as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

impl Arena {
    /// Starts with one nonempty chunk so every bind group has a valid binding.
    fn new(device: &RenderDevice, bytes: u64, chunk_bytes: u64, max_chunks: usize) -> Self {
        Self {
            chunks: vec![slot_buffer(device, bytes, 1)],
            last_capacity: 1,
            bytes,
            chunk_slots: (chunk_bytes / bytes).max(1) as usize,
            max_chunks,
        }
    }

    fn capacity(&self) -> usize {
        (self.chunks.len() - 1) * self.chunk_slots + self.last_capacity
    }

    /// Live slots of `len` that fall in chunk `index`.
    pub fn chunk_len(&self, index: usize, len: usize) -> u32 {
        len.saturating_sub(index * self.chunk_slots)
            .min(self.chunk_slots) as u32
    }

    /// Grows the last chunk in powers of two, then appends chunks; old slots are kept by a GPU copy.
    fn grow(&mut self, needed: usize, device: &RenderDevice, queue: &RenderQueue) -> bool {
        let needed = needed.min(self.chunk_slots.saturating_mul(self.max_chunks));
        if needed <= self.capacity() {
            return false;
        }
        let base = (self.chunks.len() - 1) * self.chunk_slots;
        let target = (needed - base)
            .min(self.chunk_slots)
            .next_power_of_two()
            .min(self.chunk_slots);
        if target > self.last_capacity {
            let next = slot_buffer(device, self.bytes, target);
            let last = self.chunks.last_mut().expect("arena has a chunk");
            let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
                label: Some("grow primitive shape slots"),
            });
            encoder.copy_buffer_to_buffer(
                last,
                0,
                &next,
                0,
                self.last_capacity as u64 * self.bytes,
            );
            queue.submit([encoder.finish()]);
            *last = next;
            self.last_capacity = target;
        }
        while self.capacity() < needed {
            let slots = (needed - self.capacity())
                .next_power_of_two()
                .min(self.chunk_slots);
            self.chunks.push(slot_buffer(device, self.bytes, slots));
            self.last_capacity = slots;
        }
        true
    }

    /// Writes consecutive slots split at chunk boundaries; slots past a capped arena are counted.
    fn write<T: bytemuck::Pod>(
        &self,
        queue: &RenderQueue,
        start: u32,
        mut values: &[T],
        work: &mut ShapeWork,
    ) {
        let mut slot = start as usize;
        while !values.is_empty() {
            let Some(buffer) = self.chunks.get(slot / self.chunk_slots) else {
                work.skipped_slots += values.len() as u64;
                warn_once!("primitive shape slots exceed the device storage binding limit");
                return;
            };
            let local = slot % self.chunk_slots;
            let count = values.len().min(self.chunk_slots - local);
            let bytes = bytemuck::cast_slice(&values[..count]);
            queue.write_buffer(buffer, local as u64 * self.bytes, bytes);
            work.uploads += 1;
            work.bytes += bytes.len() as u64;
            slot += count;
            values = &values[count..];
        }
    }
}

pub(super) struct Batch {
    pub key: PrimitiveMeshKey,
    pub slots: Arena,
    pub mesh: Buffer,
    pub vertices: u32,
    pub instances: u32,
    pub bind_groups: Vec<BindGroup>,
}

/// Cumulative deterministic work, independent of hardware timing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ShapeWork {
    pub uploads: u64,
    pub bytes: u64,
    pub mesh_rebuilds: u64,
    pub skipped_slots: u64,
}

/// Retained geometry and the existing nametag atlas format share one binding layout.
#[derive(Resource)]
pub(super) struct ShapeGpu {
    pub work: ShapeWork,
    pub batches: Vec<Batch>,
    source: Option<Arc<std::sync::Mutex<render_model::primitive_shapes::PrimitiveShapeStore>>>,
    pub actors: Arena,
    pub text: Arena,
    chunk_bytes: u64,
    pub text_count: u32,
    pub frame: Buffer,
    frame_value: [u32; 4],
    pub atlas: Texture,
    pub atlas_view: TextureView,
    pub sampler: Sampler,
    atlas_cells: Arc<[NametagAtlasRect]>,
    pub view_id: Option<BufferId>,
    pub global_id: Option<BufferId>,
}

/// Creates only small empty arenas; meshes and instance capacities grow on demand.
pub(super) fn init(mut commands: Commands, device: Res<RenderDevice>) {
    commands.insert_resource(ShapeGpu::new(&device, &device.limits()));
}

impl ShapeGpu {
    /// Sizes arena chunks to the smaller of the buffer and storage-binding limits.
    pub(super) fn new(device: &RenderDevice, limits: &WgpuLimits) -> Self {
        let chunk_bytes = limits
            .max_buffer_size
            .min(u64::from(limits.max_storage_buffer_binding_size));
        let atlas = device.create_texture(&TextureDescriptor {
            label: Some("primitive nametag atlas"),
            size: Extent3d {
                width: NAMETAG_ATLAS_SIDE,
                height: NAMETAG_ATLAS_SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let atlas_view = atlas.create_view(&TextureViewDescriptor::default());
        let sampler = device.create_sampler(&SamplerDescriptor {
            label: Some("primitive nametag sampler"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            ..default()
        });
        Self {
            work: ShapeWork::default(),
            batches: Vec::new(),
            source: None,
            actors: Arena::new(device, ACTOR_BYTES, chunk_bytes, 1),
            text: Arena::new(device, TEXT_BYTES, chunk_bytes, 1),
            chunk_bytes,
            text_count: 0,
            frame: device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("primitive visibility epoch"),
                contents: bytemuck::cast_slice(&[0_u32; 4]),
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            }),
            frame_value: [0; 4],
            atlas,
            atlas_view,
            sampler,
            atlas_cells: Arc::from([]),
            view_id: None,
            global_id: None,
        }
    }
}

/// Drains only dirty slots; an unchanged scene performs no buffer writes or mesh rebuilds.
pub(super) fn prepare(
    scene: Res<PrimitiveShapesScene>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<ShapeGpu>,
) {
    if gpu
        .source
        .as_ref()
        .is_none_or(|previous| !Arc::ptr_eq(previous, &scene.store))
    {
        gpu.batches.clear();
        gpu.text_count = 0;
        gpu.atlas_cells = Arc::from([]);
        gpu.source = Some(Arc::clone(&scene.store));
    }
    let mut store = scene
        .store
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let mut work = ShapeWork::default();
    let frame = [
        ((scene.clock / 3600.0).floor() * 3600.0).to_bits(),
        scene.dimension as u32,
        scene.render_distance.to_bits(),
        0,
    ];
    if gpu.frame_value != frame {
        queue.write_buffer(&gpu.frame, 0, bytemuck::cast_slice(&frame));
        gpu.frame_value = frame;
        work.uploads += 1;
        work.bytes += 16;
    }
    while gpu.batches.len() < store.batches.len() {
        let batch = &store.batches[gpu.batches.len()];
        let vertices = mesh::build(batch.key);
        work.mesh_rebuilds += 1;
        let mesh = device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("primitive shared unit mesh"),
            contents: bytemuck::cast_slice(if vertices.is_empty() {
                &[[0.0_f32; 4]][..]
            } else {
                &vertices
            }),
            usage: BufferUsages::VERTEX,
        });
        // Text records address text shapes across draws, so that pool stays in one binding.
        let max_chunks = if is_text(batch.key) { 1 } else { usize::MAX };
        let slots = Arena::new(&device, INSTANCE_BYTES, gpu.chunk_bytes, max_chunks);
        gpu.batches.push(Batch {
            key: batch.key,
            slots,
            vertices: vertices.len() as u32,
            mesh,
            instances: 0,
            bind_groups: Vec::new(),
        });
    }
    for (batch, source) in gpu.batches.iter_mut().zip(&store.batches) {
        batch.instances = source.instances.values.len() as u32;
        if batch
            .slots
            .grow(source.instances.values.len(), &device, &queue)
        {
            batch.bind_groups.clear();
        }
    }
    store.drain_uploads(|index, _key, start, instances| {
        gpu.batches[index]
            .slots
            .write(&queue, start, instances, &mut work);
    });
    let actor_grew = gpu.actors.grow(store.actors.values.len(), &device, &queue);
    let text_grew = gpu
        .text
        .grow(store.text_records.values.len(), &device, &queue);
    if actor_grew || text_grew {
        for batch in &mut gpu.batches {
            batch.bind_groups.clear();
        }
    }
    store
        .actors
        .drain(|start, actors| gpu.actors.write(&queue, start, actors, &mut work));
    store
        .text_records
        .drain(|start, records| gpu.text.write(&queue, start, records, &mut work));
    gpu.text_count = gpu.text.chunk_len(0, store.text_records.values.len());
    if !Arc::ptr_eq(&store.atlas, &gpu.atlas_cells) {
        for cell in NametagAtlasRect::updates(&store.atlas, &gpu.atlas_cells) {
            let [x, y, width, height] = cell.cell;
            work.uploads += 1;
            work.bytes += cell.rgba8.len() as u64;
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &gpu.atlas,
                    mip_level: 0,
                    origin: Origin3d { x, y, z: 0 },
                    aspect: TextureAspect::All,
                },
                &cell.rgba8,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        gpu.atlas_cells = Arc::clone(&store.atlas);
    }
    gpu.work.uploads += work.uploads;
    gpu.work.bytes += work.bytes;
    gpu.work.mesh_rebuilds += work.mesh_rebuilds;
    gpu.work.skipped_slots += work.skipped_slots;
}

/// Text records address the single text-shape pool regardless of atlas placement.
pub(super) fn is_text(key: PrimitiveMeshKey) -> bool {
    key.kind == PrimitiveShapeKind::Text
}
