//! Mod world primitives: one premultiplied, depth-tested, non-writing draw per view, after
//! the world's own transparent geometry. Block highlights use a separate through-world draw.

use super::ModRenderScene;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferSize, BufferUsages, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TextureFormat, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use mod_render::geometry::ModVertex;
use std::sync::Arc;

const PRIMITIVE_SHADER: Handle<Shader> = uuid_handle!("8e1f4b6c-2d7a-4c93-b5e0-7a19c3d84f26");
const VERTEX_BYTES: u64 = std::mem::size_of::<ModVertex>() as u64;
/// After the world's own transparent geometry, before the camera overlay at `f32::MAX`.
pub(crate) const PRIMITIVE_DISTANCE: f32 = f32::MAX / 2.0;

pub(super) fn install(app: &mut App) {
    load_internal_asset!(
        app,
        PRIMITIVE_SHADER,
        "primitives.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .init_resource::<PrimitivePipeline>()
        .add_render_command::<Transparent3d, DrawPrimitiveCommands>()
        .add_render_command::<Transparent3d, DrawBlockHighlightCommands>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare.in_set(RenderSystems::PrepareResources),
                prepare_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct PrimitiveGpu {
    vertices: Buffer,
    capacity: u64,
    uploaded: Arc<[ModVertex]>,
    uploaded_marker: Arc<[ModVertex]>,
    uploaded_blocks: Arc<[ModVertex]>,
    block_count: u32,
    pub(crate) vertex_count: u32,
    frame: Buffer,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
}

fn vertex_buffer(device: &RenderDevice, vertices: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("mod primitive vertices"),
        size: vertices.max(6) * VERTEX_BYTES,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

pub(crate) fn init_gpu(mut commands: Commands, device: Res<RenderDevice>) {
    commands.insert_resource(PrimitiveGpu {
        vertices: vertex_buffer(&device, 6),
        capacity: 6,
        uploaded: Arc::from([]),
        uploaded_marker: Arc::from([]),
        uploaded_blocks: Arc::from([]),
        block_count: 0,
        vertex_count: 0,
        frame: device.create_buffer(&BufferDescriptor {
            label: Some("mod primitive frame"),
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        bind_group: None,
        view_buffer_id: None,
    });
}

fn prepare(
    scene: Option<Res<ModRenderScene>>,
    time: Res<Time>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<PrimitiveGpu>,
) {
    let Some(scene) = scene else { return };
    let needed = scene.vertex_count() as u64;
    let grew = needed > gpu.capacity;
    if grew {
        let capacity = needed.next_power_of_two().min(
            (mod_render::geometry::MAX_VERTICES
                + super::position_box::VERTICES
                + super::MAX_BLOCK_HIGHLIGHTS * super::block_highlights::VERTICES_PER_BLOCK)
                as u64,
        );
        gpu.vertices = vertex_buffer(&device, capacity);
        gpu.capacity = capacity;
        gpu.bind_group = None;
    }
    let previous_primitive_count = gpu.uploaded.len() + gpu.uploaded_marker.len();
    let [guest_changed, marker_changed] = upload_changes(
        &gpu.uploaded,
        &gpu.uploaded_marker,
        &scene.vertices,
        &scene.marker_vertices,
        grew,
    );
    if guest_changed && !scene.vertices.is_empty() {
        queue.write_buffer(&gpu.vertices, 0, bytemuck::cast_slice(&scene.vertices));
    }
    if marker_changed && !scene.marker_vertices.is_empty() {
        queue.write_buffer(
            &gpu.vertices,
            scene.vertices.len() as u64 * VERTEX_BYTES,
            bytemuck::cast_slice(&scene.marker_vertices),
        );
    }
    if guest_changed {
        gpu.uploaded = Arc::clone(&scene.vertices);
    }
    if marker_changed {
        gpu.uploaded_marker = Arc::clone(&scene.marker_vertices);
    }
    let primitive_count = scene.vertices.len() + scene.marker_vertices.len();
    let blocks_changed = block_upload_changed(
        &gpu.uploaded_blocks,
        &scene.block_vertices,
        previous_primitive_count,
        primitive_count,
        grew,
    );
    if blocks_changed && !scene.block_vertices.is_empty() {
        queue.write_buffer(
            &gpu.vertices,
            primitive_count as u64 * VERTEX_BYTES,
            bytemuck::cast_slice(&scene.block_vertices),
        );
    }
    if blocks_changed {
        gpu.uploaded_blocks = Arc::clone(&scene.block_vertices);
    }
    gpu.vertex_count = primitive_count as u32;
    gpu.block_count = scene.block_vertices.len() as u32;
    if needed > 0 {
        let frame = [time.elapsed_secs_wrapped(), time.delta_secs(), 0.0, 0.0];
        queue.write_buffer(&gpu.frame, 0, bytemuck::cast_slice(&frame));
    }
}

fn upload_changes(
    previous_guest: &Arc<[ModVertex]>,
    previous_marker: &Arc<[ModVertex]>,
    guest: &Arc<[ModVertex]>,
    marker: &Arc<[ModVertex]>,
    grew: bool,
) -> [bool; 2] {
    [
        grew || !Arc::ptr_eq(previous_guest, guest),
        grew || previous_guest.len() != guest.len() || !Arc::ptr_eq(previous_marker, marker),
    ]
}

fn block_upload_changed(
    previous: &Arc<[ModVertex]>,
    next: &Arc<[ModVertex]>,
    previous_offset: usize,
    next_offset: usize,
    grew: bool,
) -> bool {
    grew || previous_offset != next_offset || !Arc::ptr_eq(previous, next)
}

#[cfg(test)]
mod upload_tests {
    use super::*;

    #[test]
    fn unchanged_blocks_skip_upload_but_prefix_resize_moves_their_suffix() {
        let vertices: Arc<[ModVertex]> = vec![ModVertex::default(); 36].into();
        assert!(!block_upload_changed(&vertices, &vertices, 108, 108, false));
        assert!(block_upload_changed(&vertices, &vertices, 108, 114, false));
        assert!(block_upload_changed(&vertices, &vertices, 108, 108, true));
        let moved: Arc<[ModVertex]> = vec![ModVertex::default(); 36].into();
        assert!(block_upload_changed(&vertices, &moved, 108, 108, false));
    }

    #[test]
    fn marker_motion_uploads_only_its_suffix_and_guest_resize_moves_that_suffix() {
        let guest: Arc<[ModVertex]> = vec![ModVertex::default(); 6].into();
        let marker: Arc<[ModVertex]> = vec![ModVertex::default(); 108].into();
        let moved: Arc<[ModVertex]> = vec![ModVertex::default(); 108].into();
        assert_eq!(
            upload_changes(&guest, &marker, &guest, &marker, false),
            [false, false]
        );
        assert_eq!(
            upload_changes(&guest, &marker, &guest, &moved, false),
            [false, true]
        );
        let resized: Arc<[ModVertex]> = vec![ModVertex::default(); 12].into();
        assert_eq!(
            upload_changes(&guest, &marker, &resized, &marker, false),
            [true, true]
        );
        assert_eq!(
            upload_changes(&guest, &marker, &guest, &marker, true),
            [true, true]
        );
    }
}

struct PrimitiveSpecializer;

#[derive(Resource)]
pub(crate) struct PrimitivePipeline {
    variants: Variants<RenderPipeline, PrimitiveSpecializer>,
    pub(crate) layout: BindGroupLayoutDescriptor,
}

impl FromWorld for PrimitivePipeline {
    fn from_world(_world: &mut World) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "mod primitive layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(VERTEX_BYTES),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(16),
                    },
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("mod primitive pipeline".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: PRIMITIVE_SHADER,
                entry_point: Some("mod_primitive_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: PRIMITIVE_SHADER,
                entry_point: Some("mod_primitive_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::GreaterEqual,
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(PrimitiveSpecializer, descriptor),
            layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(crate) struct PrimitiveKey {
    pub(crate) msaa: Msaa,
    pub(crate) hdr: bool,
    pub(crate) through_world: bool,
}

impl Specializer<RenderPipeline> for PrimitiveSpecializer {
    type Key = PrimitiveKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        descriptor.label = Some(if key.through_world {
            "block highlights pipeline".into()
        } else {
            "mod primitive pipeline".into()
        });
        descriptor.depth_stencil.as_mut().unwrap().depth_compare = if key.through_world {
            CompareFunction::Always
        } else {
            CompareFunction::GreaterEqual
        };
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        Ok(key)
    }
}

impl PrimitivePipeline {
    pub(crate) fn specialize(
        &mut self,
        cache: &PipelineCache,
        key: PrimitiveKey,
    ) -> Result<bevy::render::render_resource::CachedRenderPipelineId, BevyError> {
        self.variants.specialize(cache, key)
    }
}

fn prepare_bind_group(
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    pipeline: Res<PrimitivePipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<PrimitiveGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.bind_group.is_some() && gpu.view_buffer_id == Some(view_buffer.id()) {
        return;
    }
    gpu.bind_group = Some(device.create_bind_group(
        "mod primitive bind group",
        &cache.get_bind_group_layout(&pipeline.layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: gpu.vertices.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: gpu.frame.as_entire_binding(),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
}

pub(crate) fn queue(
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<PrimitivePipeline>,
    scene: Option<Res<ModRenderScene>>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
) {
    // Queue precedes upload, so this frame's scene decides; the draw reads the upload.
    let Some(scene) = scene else { return };
    let regular = scene.vertices.len() + scene.marker_vertices.len() > 0;
    let highlights = !scene.block_vertices.is_empty();
    let draw_functions = draw_functions.read();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for through_world in [false, true] {
            if !(if through_world { highlights } else { regular }) {
                continue;
            }
            let Ok(pipeline_id) = pipeline.specialize(
                &cache,
                PrimitiveKey {
                    msaa: *msaa,
                    hdr: view.hdr,
                    through_world,
                },
            ) else {
                continue;
            };
            phase.add(Transparent3d {
                entity: (view_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function: if through_world {
                    draw_functions.id::<DrawBlockHighlightCommands>()
                } else {
                    draw_functions.id::<DrawPrimitiveCommands>()
                },
                distance: PRIMITIVE_DISTANCE
                    + if through_world {
                        PRIMITIVE_DISTANCE * 0.1
                    } else {
                        0.0
                    },
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
        }
    }
}

pub(crate) type DrawPrimitiveCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuModPrimitives as usize },
    (SetItemPipeline, SetPrimitiveBindGroup, DrawPrimitives),
>;

pub(crate) type DrawBlockHighlightCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuModPrimitives as usize },
    (SetItemPipeline, SetPrimitiveBindGroup, DrawBlockHighlights),
>;

pub(crate) struct SetPrimitiveBindGroup;

impl<P: PhaseItem> RenderCommand<P> for SetPrimitiveBindGroup {
    type Param = SRes<PrimitiveGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = &gpu.into_inner().bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        RenderCommandResult::Success
    }
}

pub(crate) struct DrawPrimitives;

impl<P: PhaseItem> RenderCommand<P> for DrawPrimitives {
    type Param = SRes<PrimitiveGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let count = gpu.into_inner().vertex_count;
        if count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.draw(0..count, 0..1);
        RenderCommandResult::Success
    }
}

pub(crate) struct DrawBlockHighlights;

impl<P: PhaseItem> RenderCommand<P> for DrawBlockHighlights {
    type Param = SRes<PrimitiveGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        if gpu.block_count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.draw(gpu.vertex_count..gpu.vertex_count + gpu.block_count, 0..1);
        RenderCommandResult::Success
    }
}
