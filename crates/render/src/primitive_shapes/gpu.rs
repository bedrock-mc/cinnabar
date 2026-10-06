//! GPU ownership and dirty uploads; growth preserves old slots with a GPU copy.
use super::{PrimitiveShapesScene, mesh};
use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
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

pub(super) struct Slots {
    pub buffer: Buffer,
    pub capacity: usize,
}

impl Slots {
    /// Allocates a nonempty storage binding; contents are initialized by the first dirty write.
    fn new(device: &RenderDevice, bytes: u64, capacity: usize) -> Self {
        Self {
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some("primitive shape slots"),
                size: bytes * capacity as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            capacity,
        }
    }

    /// Preserves unchanged slots on the GPU instead of reuploading the entire arena.
    fn grow(
        &mut self,
        needed: usize,
        bytes: u64,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        if needed <= self.capacity {
            return false;
        }
        let next = Self::new(device, bytes, needed.next_power_of_two());
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("grow primitive shape slots"),
        });
        encoder.copy_buffer_to_buffer(
            &self.buffer,
            0,
            &next.buffer,
            0,
            self.capacity as u64 * bytes,
        );
        queue.submit([encoder.finish()]);
        *self = next;
        true
    }
}

pub(super) struct Batch {
    pub key: PrimitiveMeshKey,
    pub slots: Slots,
    pub mesh: Buffer,
    pub vertices: u32,
    pub instances: u32,
    pub bind_group: Option<BindGroup>,
}

/// Cumulative deterministic work, independent of hardware timing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ShapeWork {
    pub uploads: u64,
    pub bytes: u64,
    pub mesh_rebuilds: u64,
}

/// Retained geometry and the existing nametag atlas format share one binding layout.
#[derive(Resource)]
pub(super) struct ShapeGpu {
    pub work: ShapeWork,
    pub batches: Vec<Batch>,
    source: Option<Arc<std::sync::Mutex<render_model::primitive_shapes::PrimitiveShapeStore>>>,
    pub actors: Slots,
    pub text: Slots,
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
    commands.insert_resource(ShapeGpu {
        work: ShapeWork::default(),
        batches: Vec::new(),
        source: None,
        actors: Slots::new(&device, ACTOR_BYTES, 1),
        text: Slots::new(&device, TEXT_BYTES, 1),
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
    });
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
        gpu.batches.push(Batch {
            key: batch.key,
            slots: Slots::new(&device, INSTANCE_BYTES, 1),
            vertices: vertices.len() as u32,
            mesh,
            instances: 0,
            bind_group: None,
        });
    }
    for (batch, source) in gpu.batches.iter_mut().zip(&store.batches) {
        batch.instances = source.instances.values.len() as u32;
        if batch.slots.grow(
            source.instances.values.len(),
            INSTANCE_BYTES,
            &device,
            &queue,
        ) {
            batch.bind_group = None;
        }
    }
    store.drain_uploads(|index, _key, start, instances| {
        work.uploads += 1;
        work.bytes += std::mem::size_of_val(instances) as u64;
        queue.write_buffer(
            &gpu.batches[index].slots.buffer,
            u64::from(start) * INSTANCE_BYTES,
            bytemuck::cast_slice(instances),
        );
    });
    let actor_grew = gpu
        .actors
        .grow(store.actors.values.len(), ACTOR_BYTES, &device, &queue);
    let text_grew = gpu
        .text
        .grow(store.text_records.values.len(), TEXT_BYTES, &device, &queue);
    if actor_grew || text_grew {
        for batch in &mut gpu.batches {
            batch.bind_group = None;
        }
    }
    store.actors.drain(|start, actors| {
        work.uploads += 1;
        work.bytes += std::mem::size_of_val(actors) as u64;
        queue.write_buffer(
            &gpu.actors.buffer,
            u64::from(start) * ACTOR_BYTES,
            bytemuck::cast_slice(actors),
        )
    });
    store.text_records.drain(|start, records| {
        work.uploads += 1;
        work.bytes += std::mem::size_of_val(records) as u64;
        queue.write_buffer(
            &gpu.text.buffer,
            u64::from(start) * TEXT_BYTES,
            bytemuck::cast_slice(records),
        )
    });
    gpu.text_count = store.text_records.values.len() as u32;
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
}

/// Text records address the single text-shape pool regardless of atlas placement.
pub(super) fn is_text(key: PrimitiveMeshKey) -> bool {
    key.kind == PrimitiveShapeKind::Text
}
