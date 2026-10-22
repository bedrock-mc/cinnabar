use super::{clock::AnimationClock, materials, pipeline};
use crate::queue_writes::ProfiledQueue as _;
use bevy::core_pipeline::core_3d::Transparent3d;
use bevy::{
    prelude::*,
    render::{
        camera::ExtractedCamera,
        render_phase::{DrawFunctions, PhaseItemExtraIndex, ViewSortedRenderPhases},
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::ExtractedView,
    },
};
use mod_api::drawing::DrawingHandle;
use mod_render::retained::{
    Blend, Depth, Geometry,
    mesh::{self, GpuVertex},
};
use render_scene::{
    ModRenderScene,
    mod_scene::retained::{DrawingFrame, GpuInstance, resolve, visible},
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::Range,
    sync::Arc,
};

pub(super) struct Mesh {
    pub _geometry: Arc<Geometry>,
    pub buffer: Buffer,
    pub count: u32,
}
pub(super) struct Batch {
    pub geometry: DrawingHandle,
    pub instances: Range<u32>,
    pub depth: Depth,
    pub blend: Blend,
    pub material: Option<DrawingHandle>,
    pub key: pipeline::Key,
}
pub(super) struct ViewGpu {
    pub buffer: Buffer,
    pub uploaded: Vec<GpuInstance>,
    pub batches: Vec<Batch>,
    pub bindings: BTreeMap<DrawingHandle, BindGroup>,
    pub frame: Buffer,
    pub frame_uploaded: CameraFrame,
}
#[derive(Resource, Default)]
pub(super) struct Gpu {
    pub meshes: BTreeMap<DrawingHandle, Mesh>,
    pub views: HashMap<Entity, ViewGpu>,
    retired: Vec<Mesh>,
}

/// Fixed host camera/animation uniform shared by vertex and fragment stages.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct CameraFrame {
    pub clip: [[f32; 4]; 4],
    pub viewport: [f32; 4],
    pub animation: [f32; 4],
}

const LABEL: &str = "mod retained drawing";
const GEOMETRY_LABEL: &str = "mod retained geometry";

/// Allocates a nonempty storage buffer, or `None` when the device is out of memory.
fn buffer(device: &RenderDevice, label: &'static str, bytes: usize) -> Option<Buffer> {
    crate::gpu_allocation::buffer(
        device,
        &BufferDescriptor {
            label: Some(label),
            size: bytes.max(16) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        },
    )
}

/// Keeps only the instances a buffer of `capacity` instances holds, splitting the last batch.
fn truncate_to(instances: &mut Vec<GpuInstance>, batches: &mut Vec<Batch>, capacity: usize) {
    instances.truncate(capacity);
    let end = capacity as u32;
    batches.retain_mut(|batch| {
        batch.instances.end = batch.instances.end.min(end);
        batch.instances.start < batch.instances.end
    });
}

/// Uploads only newly admitted geometry, then updates small per-view instance tables.
#[allow(
    clippy::too_many_arguments,
    reason = "render queue owns view, phase, pipeline and resource admission together"
)]
pub(super) fn queue(
    scene: Res<ModRenderScene>,
    frame: Res<DrawingFrame>,
    panorama: Option<Res<render_scene::panorama::PanoramaScene>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<pipeline::Pipeline>,
    mut gpu: ResMut<Gpu>,
    mut materials: ResMut<materials::Materials>,
    clock: Res<AnimationClock>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &ExtractedCamera, &Msaa)>,
) {
    let resources: Vec<_> = scene
        .retained
        .iter()
        .filter(|r| {
            r.connection == frame.connection
                && r.dimension == frame.dimension
                && frame.connection != 0
                && r.geometry
                    .keys()
                    .chain(r.materials.keys())
                    .next()
                    .is_some_and(|handle| {
                        frame
                            .live_owners
                            .contains(&(handle.owner, handle.generation))
                    })
        })
        .collect();
    materials.retain(&resources);
    let mut compiles_left = mod_api::MAX_MATERIAL_PIPELINE_COMPILES_PER_FRAME;
    let removed: Vec<_> = gpu
        .meshes
        .keys()
        .copied()
        .filter(|handle| !resources.iter().any(|r| r.geometry.contains_key(handle)))
        .collect();
    for handle in removed {
        if let Some(mesh) = gpu.meshes.remove(&handle) {
            gpu.retired.push(mesh);
        }
    }
    gpu.views.retain(|entity, _| views.get(*entity).is_ok());
    if resources.is_empty() || panorama.is_some_and(|scene| !scene.game_visible()) {
        gpu.views.clear();
        return;
    }
    // Each missing geometry is attempted once per frame however many instances share it;
    // a failure retries next frame.
    let mut attempted = BTreeSet::new();
    for resources in &resources {
        for instance in resources.instances.values() {
            let handle = instance.geometry;
            if gpu.meshes.contains_key(&handle) || !attempted.insert(handle) {
                continue;
            }
            let Some(geometry) = resources.geometry.get(&handle) else {
                continue;
            };
            let vertices = mesh::build(geometry);
            let storage = if vertices.is_empty() {
                buffer(&device, GEOMETRY_LABEL, std::mem::size_of::<GpuVertex>())
            } else {
                crate::gpu_allocation::buffer_with_data(
                    &device,
                    &queue,
                    GEOMETRY_LABEL,
                    bytemuck::cast_slice(&vertices),
                    BufferUsages::STORAGE,
                )
            };
            let Some(storage) = storage else {
                continue;
            };
            gpu.meshes.insert(
                handle,
                Mesh {
                    _geometry: Arc::clone(geometry),
                    buffer: storage,
                    count: vertices.len() as u32,
                },
            );
        }
    }
    let draw = functions.read().id::<pipeline::Draw>();
    for (entity, main, view, extracted_camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let camera = view.world_from_view;
        let mut instances = Vec::new();
        let mut batches: Vec<Batch> = Vec::new();
        for resources in &resources {
            for instance in resources.instances.values() {
                let Some(resolved) = resolve(instance, &frame, &camera) else {
                    continue;
                };
                let Some(geometry) = resources.geometry.get(&instance.geometry) else {
                    continue;
                };
                if !visible(geometry, &resolved, &camera, view.clip_from_view) {
                    continue;
                }
                let slot = instances.len() as u32;
                instances.push(resolved);
                // Consecutive compatible instances batch without changing alpha order.
                if let Some(last) = batches.last_mut().filter(|last| {
                    last.geometry == instance.geometry
                        && last.depth == instance.depth
                        && last.blend == instance.blend
                        && last.material == instance.material
                }) {
                    last.instances.end = slot + 1;
                } else {
                    batches.push(Batch {
                        geometry: instance.geometry,
                        instances: slot..slot + 1,
                        depth: instance.depth,
                        blend: instance.blend,
                        material: instance.material,
                        key: pipeline::Key {
                            msaa: *msaa,
                            hdr: extracted_camera.hdr,
                            always: instance.depth == Depth::Always,
                            additive: instance.blend == Blend::Additive,
                        },
                    });
                }
            }
        }
        let prepared = gpu.views.entry(entity).or_insert_with(|| ViewGpu {
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some(LABEL),
                size: std::mem::size_of::<GpuInstance>().max(16) as u64,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            uploaded: Vec::new(),
            batches: Vec::new(),
            bindings: BTreeMap::new(),
            frame: device.create_buffer(&BufferDescriptor {
                label: Some("mod drawing camera"),
                size: std::mem::size_of::<CameraFrame>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            frame_uploaded: CameraFrame::default(),
        });
        let view_matrix = camera.to_matrix();
        let rotation = Mat4::from_cols(
            view_matrix.x_axis,
            view_matrix.y_axis,
            view_matrix.z_axis,
            Vec4::W,
        )
        .inverse();
        let camera_frame = CameraFrame {
            clip: (view.clip_from_view * rotation).to_cols_array_2d(),
            viewport: [
                view.viewport.z as f32,
                view.viewport.w as f32,
                view.clip_from_view.y_axis.y,
                0.,
            ],
            animation: [clock.phase, 0., 0., 0.],
        };
        if camera_frame != prepared.frame_uploaded {
            queue.profiled_write_buffer(&prepared.frame, 0, bytemuck::bytes_of(&camera_frame));
            prepared.frame_uploaded = camera_frame;
        }
        let bytes = instances.len() * std::mem::size_of::<GpuInstance>();
        if bytes as u64 > prepared.buffer.size() {
            if let Some(grown) = buffer(&device, LABEL, bytes.next_power_of_two()) {
                prepared.buffer = grown;
                prepared.bindings.clear();
            } else {
                // Out of memory draws the instances the current buffer holds.
                let capacity = prepared.buffer.size() as usize / std::mem::size_of::<GpuInstance>();
                truncate_to(&mut instances, &mut batches, capacity);
            }
        }
        if instances != prepared.uploaded {
            if !instances.is_empty() {
                queue.profiled_write_buffer(&prepared.buffer, 0, bytemuck::cast_slice(&instances));
            }
            prepared.uploaded = instances;
        }
        prepared.batches = batches;
        for (index, batch) in prepared.batches.iter().enumerate() {
            let key = batch.key;
            if let Some(handle) = batch.material {
                let Some(material) = resources.iter().find_map(|r| r.materials.get(&handle)) else {
                    continue;
                };
                if !materials.ensure(
                    handle,
                    material,
                    key,
                    &device,
                    &cache,
                    &pipeline.layout,
                    &mut compiles_left,
                ) {
                    continue;
                }
            }
            let Ok(pipeline) = pipeline.variants.specialize(&cache, key) else {
                continue;
            };
            crate::transparent_phase::add(
                phase,
                Transparent3d {
                    sorting_info:
                        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                    entity: (entity, *main),
                    pipeline,
                    draw_function: draw,
                    distance: super::super::primitives::PRIMITIVE_DISTANCE,
                    batch_range: index as u32..index as u32 + 1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                },
            );
        }
    }
}

/// Keeps removed geometry charged until the last submitted GPU work using it has completed.
pub(super) fn retire(mut gpu: ResMut<Gpu>, queue: Res<RenderQueue>) {
    if gpu.retired.is_empty() {
        return;
    }
    let retired = std::mem::take(&mut gpu.retired);
    crate::device_poll::on_frame_complete(&queue, move || drop(retired));
}
