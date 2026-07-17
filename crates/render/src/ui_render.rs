use std::{mem::size_of, sync::Arc};

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::Transparent3d,
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor,
            BufferInitDescriptor, BufferSize, BufferUsages, Canonical, ColorTargetState,
            ColorWrites, Extent3d, FilterMode, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            Specializer, SpecializerKey, Texture, TextureDataOrder, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexAttribute, VertexFormat,
            VertexState, VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget},
    },
};
use bytemuck::{Pod, Zeroable};

use crate::ui::{
    MAX_UI_INDICES, MAX_UI_VERTICES, UiRenderBatch, UiRenderInput, UiRenderReject,
    UiRenderRejectReason, UiRenderScene, UiRenderStats, UiRenderVertex, UiScissor,
};

const UI_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7cfb904c-c8cf-4dd2-9214-7d208ce454e7");

#[derive(Debug, Clone, Copy, Default)]
pub struct UiRenderPlugin;

impl Plugin for UiRenderPlugin {
    fn build(&self, app: &mut App) {
        install_ui_render(app);
    }

    fn finish(&self, app: &mut App) {
        install_ui_render(app);
    }
}

#[derive(Resource)]
struct UiRenderInstalled;

fn install_ui_render(app: &mut App) {
    app.init_resource::<UiRenderScene>()
        .init_resource::<UiRenderStats>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<UiRenderInstalled>() {
        return;
    }
    let stats = app.world().resource::<UiRenderStats>().clone();
    app.add_plugins(ExtractResourcePlugin::<UiRenderScene>::default());
    load_internal_asset!(app, UI_SHADER_HANDLE, "ui.wgsl", Shader::from_wgsl);
    app.sub_app_mut(RenderApp)
        .insert_resource(UiRenderInstalled)
        .init_resource::<UiPipeline>()
        .insert_resource(stats)
        .add_render_command::<Transparent3d, DrawUiCommands>()
        .add_systems(RenderStartup, init_ui_gpu)
        .add_systems(
            Render,
            (
                prepare_ui_resources.in_set(RenderSystems::PrepareResources),
                prepare_ui_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_ui_overlay.in_set(RenderSystems::Queue),
            ),
        );
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct UiViewportUniform {
    viewport_size: [f32; 2],
    _padding: [f32; 2],
}

#[derive(Resource)]
pub(crate) struct UiGpu {
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_capacity: usize,
    index_capacity: usize,
    vertex_arena_id: u64,
    index_arena_id: u64,
    viewport_buffer: Buffer,
    viewport_size: [u32; 2],
    texture: Option<Texture>,
    texture_view: Option<TextureView>,
    texture_identity: Option<[u8; 32]>,
    texture_bytes: usize,
    sampler: Sampler,
    bind_group: Option<BindGroup>,
    batches: Arc<[UiRenderBatch]>,
    accepted_revision: Option<u64>,
}

fn init_ui_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let viewport_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("shared UI viewport uniform"),
        contents: bytemuck::bytes_of(&UiViewportUniform {
            viewport_size: [1.0, 1.0],
            _padding: [0.0; 2],
        }),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("shared nearest UI texture sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: FilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(UiGpu {
        vertex_buffer: None,
        index_buffer: None,
        vertex_capacity: 0,
        index_capacity: 0,
        vertex_arena_id: 0,
        index_arena_id: 0,
        viewport_buffer,
        viewport_size: [1, 1],
        texture: None,
        texture_view: None,
        texture_identity: None,
        texture_bytes: 0,
        sampler,
        bind_group: None,
        batches: Arc::from([]),
        accepted_revision: None,
    });
}

pub(crate) fn prepare_ui_resources(
    scene: Res<UiRenderScene>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<UiGpu>,
    stats: Res<UiRenderStats>,
) {
    let Some(input) = scene.input.as_ref() else {
        return;
    };
    if gpu.accepted_revision == Some(input.revision) {
        return;
    }
    if let Err(reason) = input.validate() {
        record_render_rejection(&stats, input.revision, reason);
        return;
    }

    if gpu.vertex_capacity < input.vertices.len() {
        let capacity = arena_capacity(input.vertices.len(), MAX_UI_VERTICES);
        gpu.vertex_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("shared bounded UI vertex arena"),
            size: arena_bytes(capacity, size_of::<UiRenderVertex>()),
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.vertex_capacity = capacity;
        gpu.vertex_arena_id = gpu.vertex_arena_id.saturating_add(1);
    }
    if gpu.index_capacity < input.indices.len() {
        let capacity = arena_capacity(input.indices.len(), MAX_UI_INDICES);
        gpu.index_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("shared bounded UI index arena"),
            size: arena_bytes(capacity, size_of::<u32>()),
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.index_capacity = capacity;
        gpu.index_arena_id = gpu.index_arena_id.saturating_add(1);
    }
    if let Some(buffer) = gpu.vertex_buffer.as_ref()
        && !input.vertices.is_empty()
    {
        render_queue.write_buffer(buffer, 0, bytemuck::cast_slice(&input.vertices));
    }
    if let Some(buffer) = gpu.index_buffer.as_ref()
        && !input.indices.is_empty()
    {
        render_queue.write_buffer(buffer, 0, bytemuck::cast_slice(&input.indices));
    }
    let viewport = UiViewportUniform {
        viewport_size: [input.viewport_size[0] as f32, input.viewport_size[1] as f32],
        _padding: [0.0; 2],
    };
    render_queue.write_buffer(&gpu.viewport_buffer, 0, bytemuck::bytes_of(&viewport));
    gpu.viewport_size = input.viewport_size;

    if gpu.texture_identity != Some(input.textures.identity) {
        let texture = render_device.create_texture_with_data(
            &render_queue,
            &TextureDescriptor {
                label: Some("shared bounded UI texture array"),
                size: Extent3d {
                    width: input.textures.width,
                    height: input.textures.height,
                    depth_or_array_layers: input.textures.layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &input.textures.rgba8,
        );
        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("shared bounded UI texture array view"),
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
        gpu.texture = Some(texture);
        gpu.texture_view = Some(view);
        gpu.texture_identity = Some(input.textures.identity);
        gpu.texture_bytes = input.textures.rgba8.len();
        gpu.bind_group = None;
    }

    gpu.batches = Arc::clone(&input.batches);
    gpu.accepted_revision = Some(input.revision);
    stats.update(|stats| {
        stats.accepted_revision = Some(input.revision);
        stats.uploaded_vertices = input.vertices.len() as u32;
        stats.uploaded_indices = input.indices.len() as u32;
        stats.draw_calls = input.batches.len() as u32;
        stats.vertex_arena_capacity = gpu.vertex_capacity as u32;
        stats.index_arena_capacity = gpu.index_capacity as u32;
        stats.per_node_gpu_allocations = 0;
        stats.retained_gpu_bytes =
            retained_gpu_bytes(gpu.vertex_capacity, gpu.index_capacity, gpu.texture_bytes);
        stats.rejected_revision = None;
        stats.rejected_reason = None;
    });
}

fn record_render_rejection(stats: &UiRenderStats, revision: u64, reason: UiRenderRejectReason) {
    stats.update(|stats| {
        stats.rejected_revision = Some(revision);
        stats.rejected_reason = Some(reason);
        stats.rejection_count = stats.rejection_count.saturating_add(1);
    });
}

fn arena_capacity(required: usize, limit: usize) -> usize {
    if required == 0 {
        return 0;
    }
    required
        .checked_next_power_of_two()
        .unwrap_or(limit)
        .min(limit)
}

fn arena_bytes(capacity: usize, stride: usize) -> u64 {
    u64::try_from(capacity.saturating_mul(stride).max(4)).expect("bounded UI arena byte count")
}

fn retained_gpu_bytes(vertices: usize, indices: usize, texture_bytes: usize) -> u64 {
    let bytes = vertices
        .saturating_mul(size_of::<UiRenderVertex>())
        .saturating_add(indices.saturating_mul(size_of::<u32>()))
        .saturating_add(texture_bytes)
        .saturating_add(size_of::<UiViewportUniform>());
    bytes as u64
}

struct UiPipelineSpecializer;

#[derive(Resource)]
struct UiPipeline {
    variants: Variants<RenderPipeline, UiPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for UiPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = ui_bind_group_layout();
        let descriptor = ui_pipeline_descriptor(bind_group_layout.clone());
        Self {
            variants: Variants::new(UiPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

pub(crate) fn ui_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "shared UI bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<UiViewportUniform>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    )
}

pub(crate) fn ui_pipeline_descriptor(
    bind_group_layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
    let blend = BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    };
    RenderPipelineDescriptor {
        label: Some("shared retained UI overlay pipeline".into()),
        layout: vec![bind_group_layout],
        vertex: VertexState {
            shader: UI_SHADER_HANDLE,
            entry_point: Some("ui_vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: size_of::<UiRenderVertex>() as u64,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![
                    VertexAttribute {
                        format: VertexFormat::Float32x2,
                        offset: 0,
                        shader_location: 0,
                    },
                    VertexAttribute {
                        format: VertexFormat::Uint16x2,
                        offset: 8,
                        shader_location: 1,
                    },
                    VertexAttribute {
                        format: VertexFormat::Unorm8x4,
                        offset: 12,
                        shader_location: 2,
                    },
                    VertexAttribute {
                        format: VertexFormat::Uint32,
                        offset: 16,
                        shader_location: 3,
                    },
                ],
            }],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: UI_SHADER_HANDLE,
            entry_point: Some("ui_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: Some(BlendState {
                    color: blend,
                    alpha: blend,
                }),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: None,
        ..default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct UiPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for UiPipelineSpecializer {
    type Key = UiPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
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

fn prepare_ui_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<UiPipeline>,
    mut gpu: ResMut<UiGpu>,
) {
    if gpu.bind_group.is_some() {
        return;
    }
    let Some(texture_view) = gpu.texture_view.as_ref() else {
        return;
    };
    gpu.bind_group = Some(render_device.create_bind_group(
        "shared retained UI bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: gpu.viewport_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(texture_view),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
        ],
    ));
}

fn queue_ui_overlay(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<UiPipeline>,
    gpu: Res<UiGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
) {
    if gpu.batches.is_empty()
        || gpu.bind_group.is_none()
        || gpu.vertex_buffer.is_none()
        || gpu.index_buffer.is_none()
    {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawUiCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            UiPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };
        phase.add(Transparent3d {
            entity: (view_entity, *main_entity),
            pipeline: pipeline_id,
            draw_function,
            distance: f32::MAX,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: true,
        });
    }
}

type DrawUiCommands = (SetItemPipeline, SetUiBindGroup<0>, DrawUiBatches);

struct SetUiBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetUiBindGroup<I> {
    type Param = SRes<UiGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = &gpu.into_inner().bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(I, bind_group, &[]);
        RenderCommandResult::Success
    }
}

struct DrawUiBatches;

impl<P: PhaseItem> RenderCommand<P> for DrawUiBatches {
    type Param = SRes<UiGpu>;
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
        let (Some(vertices), Some(indices)) = (&gpu.vertex_buffer, &gpu.index_buffer) else {
            return RenderCommandResult::Skip;
        };
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
        for batch in gpu.batches.iter() {
            let scissor = batch.scissor;
            pass.set_scissor_rect(scissor.x, scissor.y, scissor.width, scissor.height);
            pass.draw_indexed(
                batch.first_index..batch.first_index + batch.index_count,
                0,
                batch.texture_page..batch.texture_page + 1,
            );
        }
        pass.set_scissor_rect(0, 0, gpu.viewport_size[0], gpu.viewport_size[1]);
        RenderCommandResult::Success
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UiPreparedFrame {
    pub revision: u64,
    pub pipeline_id: u64,
    pub bind_group_family_id: u64,
    pub vertex_arena_id: u64,
    pub index_arena_id: u64,
    pub per_node_gpu_allocations: u32,
    draw_order: Arc<[usize]>,
    scissors: Arc<[UiScissor]>,
}

impl UiPreparedFrame {
    #[must_use]
    pub fn draw_order(&self) -> &[usize] {
        &self.draw_order
    }

    #[must_use]
    pub fn scissors(&self) -> &[UiScissor] {
        &self.scissors
    }
}

pub struct UiRenderHarness {
    scene: UiRenderScene,
    stats: UiRenderStats,
    vertex_capacity: usize,
    index_capacity: usize,
    vertex_arena_id: u64,
    index_arena_id: u64,
    prepared: Option<UiPreparedFrame>,
}

impl UiRenderHarness {
    #[must_use]
    pub fn new() -> Self {
        Self {
            scene: UiRenderScene::default(),
            stats: UiRenderStats::default(),
            vertex_capacity: 0,
            index_capacity: 0,
            vertex_arena_id: 0,
            index_arena_id: 0,
            prepared: None,
        }
    }

    pub fn publish(&mut self, input: UiRenderInput) -> Result<(), UiRenderReject> {
        self.scene.publish(input, &self.stats)
    }

    pub fn prepare(&mut self) -> Result<UiPreparedFrame, UiRenderReject> {
        let Some(input) = self.scene.input.as_ref() else {
            return Err(UiRenderReject {
                revision: self.scene.revision,
                reason: UiRenderRejectReason::NoPublishedScene,
            });
        };
        if let Some(prepared) = &self.prepared
            && prepared.revision == input.revision
        {
            return Ok(prepared.clone());
        }
        if self.vertex_capacity < input.vertices.len() {
            self.vertex_capacity = arena_capacity(input.vertices.len(), MAX_UI_VERTICES);
            self.vertex_arena_id = self.vertex_arena_id.saturating_add(1);
        }
        if self.index_capacity < input.indices.len() {
            self.index_capacity = arena_capacity(input.indices.len(), MAX_UI_INDICES);
            self.index_arena_id = self.index_arena_id.saturating_add(1);
        }
        self.stats.update(|stats| {
            stats.accepted_revision = Some(input.revision);
            stats.uploaded_vertices = input.vertices.len() as u32;
            stats.uploaded_indices = input.indices.len() as u32;
            stats.draw_calls = input.batches.len() as u32;
            stats.vertex_arena_capacity = self.vertex_capacity as u32;
            stats.index_arena_capacity = self.index_capacity as u32;
            stats.per_node_gpu_allocations = 0;
            stats.retained_gpu_bytes = retained_gpu_bytes(
                self.vertex_capacity,
                self.index_capacity,
                input.textures.rgba8.len(),
            );
        });
        let prepared = UiPreparedFrame {
            revision: input.revision,
            pipeline_id: 1,
            bind_group_family_id: 1,
            vertex_arena_id: self.vertex_arena_id,
            index_arena_id: self.index_arena_id,
            per_node_gpu_allocations: 0,
            draw_order: (0..input.batches.len()).collect::<Vec<_>>().into(),
            scissors: input
                .batches
                .iter()
                .map(|batch| batch.scissor)
                .collect::<Vec<_>>()
                .into(),
        };
        self.prepared = Some(prepared.clone());
        Ok(prepared)
    }

    #[must_use]
    pub const fn scene(&self) -> &UiRenderScene {
        &self.scene
    }

    #[must_use]
    pub fn stats(&self) -> crate::ui::UiRenderStatsSnapshot {
        self.stats.snapshot()
    }
}

impl Default for UiRenderHarness {
    fn default() -> Self {
        Self::new()
    }
}
