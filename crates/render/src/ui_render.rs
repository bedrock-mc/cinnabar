use std::{
    mem::size_of,
    sync::{Arc, Weak},
};

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
    ecs::system::SystemChangeTick,
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_resource::{
            AddressMode, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingType,
            BlendComponent, BlendFactor, BlendOperation, BlendState, Buffer, BufferBindingType,
            BufferDescriptor, BufferInitDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, FilterMode, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            Specializer, SpecializerKey, TextureFormat, TextureSampleType, TextureViewDimension,
            Variants, VertexAttribute, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget},
    },
};
#[path = "ui_render/textures.rs"]
mod textures;
pub(crate) use textures::DeviceObservation;
use textures::{UiGpuTextures, prepare_ui_bind_group};
#[path = "ui_render/batches.rs"]
mod batches;
#[path = "ui_render/composite.rs"]
pub(crate) mod composite;
#[path = "ui_render/glint.rs"]
mod glint;
#[path = "ui_render/model_depth.rs"]
mod model_depth;
#[path = "ui_render/overlay.rs"]
pub(crate) mod overlay;
#[path = "ui_render/pipeline.rs"]
mod pipeline;
#[path = "ui_render/shader.rs"]
pub(crate) mod shader;
#[path = "ui_render/uploads.rs"]
mod uploads;
use batches::resolved_batches;
pub use glint::UiGlintSettings;
use overlay::queue_ui_overlay;
pub(crate) use overlay::{UiHandCoverage, UiOverlayLabel, UiWorldLabel, install_overlay_graph};
use pipeline::UiPipelineKey;
use shader::UiViewportUniform;

use render_model::{
    MAX_UI_INDICES, MAX_UI_VERTICES, UI_BLEND_INVERT, UiRenderBatch, UiRenderInput,
    UiRenderRejectReason, UiRenderScene, UiRenderStats, UiRenderVertex,
};
#[cfg(test)]
use render_model::{UiRenderReject, UiScissor};

/// Main-world holder of the published [`UiRenderScene`], cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Debug, Default, Deref, DerefMut)]
pub struct UiRenderSceneResource(pub UiRenderScene);

/// The [`UiRenderStats`] handle both worlds share.
#[derive(Resource, Clone, Debug, Default, Deref, DerefMut)]
pub struct UiRenderStatsResource(pub UiRenderStats);

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
    app.init_resource::<UiRenderSceneResource>()
        .init_resource::<UiGlintSettings>()
        .init_resource::<UiRenderStatsResource>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<UiRenderInstalled>() {
        install_overlay_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    crate::pipeline_warmup::register::<UiPipeline>(app);
    let stats = app.world().resource::<UiRenderStatsResource>().clone();
    app.add_plugins((
        ExtractResourcePlugin::<UiRenderSceneResource>::default(),
        ExtractResourcePlugin::<UiGlintSettings>::default(),
    ));
    load_internal_asset!(app, UI_SHADER_HANDLE, "ui.wgsl", shader::from_wgsl);
    load_internal_asset!(
        app,
        composite::UI_COMPOSITE_SHADER_HANDLE,
        "ui_composite.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(UiRenderInstalled)
        .init_resource::<UiPipeline>()
        .init_resource::<composite::UiCompositePipeline>()
        .insert_resource(stats)
        .init_resource::<UiHandCoverage>()
        .init_resource::<composite::UiLayerStore>()
        .init_resource::<model_depth::UiModelDepths>()
        .add_systems(RenderStartup, init_ui_gpu)
        .add_systems(
            Render,
            (
                prepare_ui_resources.in_set(RenderSystems::PrepareResources),
                composite::prepare_ui_layers.in_set(RenderSystems::PrepareResources),
                prepare_ui_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_ui_overlay.in_set(RenderSystems::Queue),
            ),
        );
    install_overlay_graph(app.sub_app_mut(RenderApp).world_mut());
}

#[derive(Resource)]
pub(crate) struct UiGpu {
    device: wgpu::Device,
    device_observation: DeviceObservation,
    vertex_buffer: Option<Buffer>,
    index_buffer: Option<Buffer>,
    vertex_capacity: usize,
    index_capacity: usize,
    vertex_arena_id: u64,
    index_arena_id: u64,
    viewport_buffer: Buffer,
    viewport_size: [u32; 2],
    started: std::time::Instant,
    textures: UiGpuTextures,
    sampler: Sampler,
    /// `bilinear` sprites sample through this instead.
    linear_sampler: Sampler,
    batches: Arc<[UiRenderBatch]>,
    accepted_revision: Option<u64>,
    /// The accepted revision draws glint, which animates without a new revision.
    animated: bool,
    // Admission watermark survives every draw rejection, even after payload drop.
    last_admitted_revision: Option<u64>,
    last_admitted_publication: Weak<UiRenderInput>,
    index_count: usize,
    uploads: uploads::BufferUploads,
    view_pipelines:
        std::collections::BTreeMap<Entity, (CachedRenderPipelineId, CachedRenderPipelineId)>,
    /// Each view's UI-layer composite pipelines.
    composite_pipelines: std::collections::BTreeMap<Entity, composite::CompositePipelines>,
    world_view_pipelines: std::collections::BTreeMap<
        (Entity, bool, bool),
        (CachedRenderPipelineId, CachedRenderPipelineId),
    >,
    model_view_pipelines: std::collections::BTreeMap<
        (Entity, bool, bool),
        (CachedRenderPipelineId, CachedRenderPipelineId),
    >,
}

fn init_ui_gpu(mut commands: Commands, render_device: Res<RenderDevice>, tick: SystemChangeTick) {
    let viewport_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("shared UI viewport uniform"),
        contents: bytemuck::bytes_of(&UiViewportUniform {
            viewport_size: [1.0, 1.0],
            time_seconds: 0.0,
            glint_strength: UiGlintSettings::default().strength,
        }),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let sampler_with = |label, filter| {
        render_device.create_sampler(&SamplerDescriptor {
            label: Some(label),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: filter,
            min_filter: filter,
            mipmap_filter: FilterMode::Nearest,
            ..default()
        })
    };
    let sampler = sampler_with("shared nearest UI texture sampler", FilterMode::Nearest);
    let linear_sampler = sampler_with("shared bilinear UI texture sampler", FilterMode::Linear);
    commands.insert_resource(UiGpu {
        device: render_device.wgpu_device().clone(),
        device_observation: DeviceObservation::new(tick.this_run()),
        vertex_buffer: None,
        index_buffer: None,
        vertex_capacity: 0,
        index_capacity: 0,
        vertex_arena_id: 0,
        index_arena_id: 0,
        viewport_buffer,
        viewport_size: [1, 1],
        started: std::time::Instant::now(),
        textures: UiGpuTextures::default(),
        sampler,
        linear_sampler,
        batches: Arc::from([]),
        accepted_revision: None,
        animated: false,
        last_admitted_revision: None,
        last_admitted_publication: Weak::new(),
        index_count: 0,
        uploads: uploads::BufferUploads::default(),
        view_pipelines: std::collections::BTreeMap::new(),
        composite_pipelines: std::collections::BTreeMap::new(),
        world_view_pipelines: std::collections::BTreeMap::new(),
        model_view_pipelines: std::collections::BTreeMap::new(),
    });
}

pub(crate) fn prepare_ui_resources(
    scene: Res<UiRenderSceneResource>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<UiGpu>,
    stats: Res<UiRenderStatsResource>,
    tick: SystemChangeTick,
    (coverage, glint): (Option<Res<UiHandCoverage>>, Option<Res<UiGlintSettings>>),
) {
    let same_device = &gpu.device == render_device.wgpu_device();
    let device_valid =
        gpu.device_observation
            .observe(render_device.last_changed(), tick.this_run(), same_device);
    if let Some(coverage) = coverage {
        coverage.clear();
    }
    let Some(input) = scene.input.as_ref() else {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        stats.update(|s| {
            s.accepted_revision = None;
            s.draw_calls = 0;
        });
        return;
    };
    if !device_valid {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        record_render_rejection(
            &stats,
            input.revision,
            UiRenderRejectReason::InvalidTextureExtent,
        );
        return;
    }
    // Written every frame: the glint animates without a new UI revision.
    let viewport = UiViewportUniform {
        viewport_size: [input.viewport_size[0] as f32, input.viewport_size[1] as f32],
        time_seconds: glint
            .as_deref()
            .copied()
            .unwrap_or_default()
            .animation_seconds(gpu.started.elapsed().as_secs_f32()),
        glint_strength: glint.as_deref().copied().unwrap_or_default().strength,
    };
    {
        #[cfg(feature = "tracy")]
        let _span =
            bevy::log::info_span!("ui.viewport_write", bytes = size_of::<UiViewportUniform>())
                .entered();
        render_queue.write_buffer(&gpu.viewport_buffer, 0, bytemuck::bytes_of(&viewport));
    }
    if let Some(previous) = gpu.last_admitted_revision {
        let reason = if input.revision < previous {
            Some(UiRenderRejectReason::StaleRevision {
                current: previous,
                rejected: input.revision,
            })
        } else if input.revision == previous
            && !gpu.last_admitted_publication.ptr_eq(&Arc::downgrade(input))
        {
            Some(UiRenderRejectReason::RevisionConflict {
                revision: input.revision,
            })
        } else {
            None
        };
        if let Some(reason) = reason {
            gpu.accepted_revision = None;
            gpu.batches = Arc::from([]);
            record_render_rejection(&stats, input.revision, reason);
            return;
        }
    }
    if gpu.accepted_revision == Some(input.revision) {
        if !gpu.textures.resident(&input.textures)
            || (!input.vertices.is_empty() && gpu.vertex_buffer.is_none())
            || (!input.indices.is_empty() && gpu.index_buffer.is_none())
        {
            gpu.accepted_revision = None;
            gpu.batches = Arc::from([]);
            record_render_rejection(
                &stats,
                input.revision,
                UiRenderRejectReason::InvalidTextureExtent,
            );
        }
        return;
    }
    if let Err(reason) = input.validate() {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        record_render_rejection(&stats, input.revision, reason);
        return;
    }
    if let Err(reason) = gpu
        .textures
        .prepare(&input.textures, &render_device, &render_queue)
    {
        gpu.accepted_revision = None;
        gpu.batches = Arc::from([]);
        record_render_rejection(&stats, input.revision, reason);
        return;
    }

    let fresh_vertices = gpu.vertex_capacity < input.vertices.len();
    let fresh_indices = gpu.index_capacity < input.indices.len();
    if fresh_vertices {
        let capacity = arena_capacity(input.vertices.len(), MAX_UI_VERTICES);
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.vertex_allocate",
            bytes = arena_bytes(capacity, size_of::<UiRenderVertex>())
        )
        .entered();
        gpu.vertex_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("shared bounded UI vertex arena"),
            size: arena_bytes(capacity, size_of::<UiRenderVertex>()),
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.vertex_capacity = capacity;
        gpu.vertex_arena_id = gpu.vertex_arena_id.saturating_add(1);
    }
    if fresh_indices {
        let capacity = arena_capacity(input.indices.len(), MAX_UI_INDICES);
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.index_allocate",
            bytes = arena_bytes(capacity, size_of::<u32>())
        )
        .entered();
        gpu.index_buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("shared bounded UI index arena"),
            size: arena_bytes(capacity, size_of::<u32>()),
            usage: BufferUsages::INDEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.index_capacity = capacity;
        gpu.index_arena_id = gpu.index_arena_id.saturating_add(1);
    }
    let upload = gpu.uploads.plan(input, fresh_vertices, fresh_indices);
    if let Some(buffer) = gpu.vertex_buffer.as_ref()
        && !upload.vertices.is_empty()
    {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.vertex_write",
            revision = input.revision,
            vertices = upload.vertices.len(),
            bytes = upload.vertices.len() * size_of::<UiRenderVertex>(),
        )
        .entered();
        render_queue.write_buffer(
            buffer,
            (upload.vertices.start * size_of::<UiRenderVertex>()) as u64,
            bytemuck::cast_slice(&input.vertices[upload.vertices.clone()]),
        );
    }
    if let Some(buffer) = gpu.index_buffer.as_ref()
        && !upload.indices.is_empty()
    {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "ui.index_write",
            revision = input.revision,
            indices = upload.indices.len(),
            bytes = upload.indices.len() * size_of::<u32>(),
        )
        .entered();
        render_queue.write_buffer(
            buffer,
            (upload.indices.start * size_of::<u32>()) as u64,
            bytemuck::cast_slice(&input.indices[upload.indices.clone()]),
        );
    }
    gpu.viewport_size = input.viewport_size;

    gpu.batches = Arc::clone(&input.batches);
    gpu.animated = input
        .vertices
        .iter()
        .any(|vertex| vertex.style_flags & render_model::UI_STYLE_GLINT != 0);
    gpu.index_count = input.indices.len();
    gpu.accepted_revision = Some(input.revision);
    gpu.last_admitted_revision = Some(input.revision);
    gpu.last_admitted_publication = Arc::downgrade(input);
    stats.update(|stats| {
        stats.accepted_revision = Some(input.revision);
        stats.uploaded_vertices = upload.vertices.len() as u32;
        stats.uploaded_indices = upload.indices.len() as u32;
        stats.draw_calls = input.batches.len() as u32;
        stats.vertex_arena_capacity = gpu.vertex_capacity as u32;
        stats.index_arena_capacity = gpu.index_capacity as u32;
        stats.per_node_gpu_allocations = 0;
        stats.retained_gpu_bytes =
            retained_gpu_bytes(gpu.vertex_capacity, gpu.index_capacity, gpu.textures.bytes);
        stats.rejected_revision = None;
        stats.rejected_reason = None;
    });
}

fn record_render_rejection(stats: &UiRenderStats, revision: u64, reason: UiRenderRejectReason) {
    static REJECTIONS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let count = REJECTIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    if count.is_power_of_two() {
        bevy::log::warn!(count, revision, ?reason, "UI frame rejected");
    }
    stats.update(|stats| {
        stats.accepted_revision = None;
        stats.draw_calls = 0;
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
                // The fragment stage reads the glint clock.
                visibility: ShaderStages::VERTEX_FRAGMENT,
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
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            },
        ],
    )
}

/// The premultiplied-alpha blend state shared by every UI quad except the
/// crosshair.
pub(crate) fn ui_alpha_blend_state() -> BlendState {
    let blend = BlendComponent {
        src_factor: BlendFactor::One,
        dst_factor: BlendFactor::OneMinusSrcAlpha,
        operation: BlendOperation::Add,
    };
    BlendState {
        color: blend,
        alpha: blend,
    }
}

/// The classic crosshair invert: color = src*(1-dst) + dst*(1-src), so the
/// white cross reads against any background; alpha passes the source through.
pub(crate) fn ui_invert_blend_state() -> BlendState {
    BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::OneMinusDst,
            dst_factor: BlendFactor::OneMinusSrc,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent {
            src_factor: BlendFactor::One,
            dst_factor: BlendFactor::Zero,
            operation: BlendOperation::Add,
        },
    }
}

pub(crate) fn ui_pipeline_descriptor(
    bind_group_layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
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
                        format: VertexFormat::Float32x4,
                        offset: std::mem::offset_of!(UiRenderVertex, position) as u64,
                        shader_location: 0,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x2,
                        offset: std::mem::offset_of!(UiRenderVertex, uv) as u64,
                        shader_location: 1,
                    },
                    VertexAttribute {
                        format: VertexFormat::Unorm8x4,
                        offset: std::mem::offset_of!(UiRenderVertex, color) as u64,
                        shader_location: 2,
                    },
                    VertexAttribute {
                        format: VertexFormat::Uint32,
                        offset: std::mem::offset_of!(UiRenderVertex, style_flags) as u64,
                        shader_location: 3,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32,
                        offset: std::mem::offset_of!(UiRenderVertex, alpha_cutoff) as u64,
                        shader_location: 4,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32,
                        offset: std::mem::offset_of!(UiRenderVertex, model_light) as u64,
                        shader_location: 5,
                    },
                    VertexAttribute {
                        format: VertexFormat::Float32x4,
                        offset: std::mem::offset_of!(UiRenderVertex, overlay_color) as u64,
                        shader_location: 6,
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
                blend: Some(ui_alpha_blend_state()),
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: None,
        ..default()
    }
}

#[cfg(test)]
#[path = "ui_render/ordered_command_tests.rs"]
mod ordered_command_tests;

#[cfg(test)]
#[allow(dead_code)]
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

#[cfg(test)]
#[allow(dead_code)]
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

#[cfg(test)]
#[allow(dead_code)]
pub struct UiRenderHarness {
    scene: UiRenderScene,
    stats: UiRenderStats,
    vertex_capacity: usize,
    index_capacity: usize,
    vertex_arena_id: u64,
    index_arena_id: u64,
    prepared: Option<UiPreparedFrame>,
}

#[cfg(test)]
#[allow(dead_code)]
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
                input.textures.plan().bytes(),
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
    pub fn stats(&self) -> render_model::UiRenderStatsSnapshot {
        self.stats.snapshot()
    }
}

#[cfg(test)]
impl Default for UiRenderHarness {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "ui_render/retained_tests.rs"]
mod retained_tests;
