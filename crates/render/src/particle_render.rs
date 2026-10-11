//! GPU particle atlas and instance uploads, with depth-writing, blended and additive draws.

use std::{ops::Range, sync::Arc};

#[cfg(test)]
use bevy::prelude::Mut;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    prelude::{
        App, BevyError, Commands, Deref, DerefMut, Entity, FromWorld, GlobalTransform, Handle,
        IntoScheduleConfigs, Msaa, Plugin, Projection, Query, Res, ResMut, Resource, Result,
        Shader, Vec3, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferSize, BufferUsages, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, Origin3d, PipelineCache,
            RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use particles::{
    ATLAS_SIDE, AtlasPatch, DrawLists, ParticleInstance, ParticleSystem, ParticleView,
    ParticleWorld,
};

const PARTICLE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b2f9c3e-4d1a-4e8b-9a57-3c0f1d7a2b64");
const INSTANCE_BYTES: u64 = std::mem::size_of::<ParticleInstance>() as u64;
const MIN_CAPACITY: usize = 256;

#[cfg(all(test, target_os = "macos"))]
#[path = "particle_render/upload_tests.rs"]
mod upload_tests;

#[cfg(test)]
#[path = "particle_render/tests.rs"]
pub(crate) mod tests;

/// The particle simulation as a Bevy resource.
#[derive(Resource, Default, Deref, DerefMut)]
pub struct ParticleSimulation(pub ParticleSystem);

/// The frame's particle draw data, extracted to the render world each frame.
#[derive(Resource, ExtractResource, Clone, Default)]
pub struct ParticleGpuFrame {
    base: Option<Arc<[u8]>>,
    patch_seq: u64,
    patches: Arc<[AtlasPatch]>,
    opaque: Arc<[ParticleInstance]>,
    blend: Arc<[ParticleInstance]>,
    add: Arc<[ParticleInstance]>,
    blend_centroid: [f32; 3],
    add_centroid: [f32; 3],
}

impl ParticleGpuFrame {
    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.opaque.len() + self.blend.len() + self.add.len()
    }

    fn set_lists(&mut self, lists: DrawLists, camera: [f32; 3]) {
        self.blend_centroid = draw_centroid(&lists.blend, camera);
        self.add_centroid = draw_centroid(&lists.add, camera);
        self.blend = lists.blend.into();
        self.add = lists.add.into();
        self.opaque = lists.opaque.into();
    }
}

/// Returns a draw list's centroid, falling back to the camera for an empty list.
fn draw_centroid(instances: &[ParticleInstance], camera: [f32; 3]) -> [f32; 3] {
    if instances.is_empty() {
        return camera;
    }
    let mut sum = [0.0; 3];
    for instance in instances {
        for (axis, total) in sum.iter_mut().enumerate() {
            *total += instance.center_light[axis];
        }
    }
    sum.map(|total| total / instances.len() as f32)
}

/// Camera frustum basis from a Bevy camera transform and projection.
#[must_use]
pub fn particle_view(transform: &GlobalTransform, projection: &Projection) -> ParticleView {
    let (right, up, forward) = (transform.right(), transform.up(), transform.forward());
    let half_diagonal = match projection {
        Projection::Perspective(perspective) => {
            let half_height = (perspective.fov * 0.5).tan();
            let half_width = half_height * perspective.aspect_ratio;
            half_width.hypot(half_height).atan()
        }
        _ => std::f32::consts::FRAC_PI_2,
    };
    ParticleView {
        position: transform.translation().to_array(),
        right: right.to_array(),
        up: up.to_array(),
        forward: forward.to_array(),
        half_diagonal,
    }
}

/// Ticks the simulation and refreshes the extracted draw data; call once per frame.
pub fn update_particle_frame(
    system: &mut ParticleSystem,
    frame: &mut ParticleGpuFrame,
    dt: f32,
    view: &ParticleView,
    world: &dyn ParticleWorld,
) {
    system.set_camera(view.position);
    system.tick(dt, world);
    let lists = system.build_draw(view, world);
    frame.set_lists(lists, view.position);
    let base = system.atlas_base();
    if frame
        .base
        .as_ref()
        .is_none_or(|old| !Arc::ptr_eq(old, &base))
    {
        frame.base = Some(base);
        frame.patch_seq = 0;
        frame.patches = Arc::default();
    }
    let seq = system.atlas().patch_seq();
    if seq != frame.patch_seq {
        frame.patches = system.atlas().patches().into();
        frame.patch_seq = seq;
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ParticleRenderPlugin;

impl Plugin for ParticleRenderPlugin {
    fn build(&self, app: &mut App) {
        crate::upload_staging::install(app);
        crate::lighting::install(app);
        app.init_resource::<ParticleSimulation>()
            .init_resource::<ParticleGpuFrame>()
            .add_plugins(ExtractResourcePlugin::<ParticleGpuFrame>::default());
        load_internal_asset!(
            app,
            PARTICLE_SHADER_HANDLE,
            "particles.wgsl",
            crate::shader_safety::from_wgsl
        );
        crate::pipeline_warmup::register::<ParticlePipeline>(app);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        crate::transparent_phase::install(render_app);
        render_app
            .init_resource::<ParticlePipeline>()
            .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Opaque as u8 }>>()
            .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Blend as u8 }>>()
            .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Add as u8 }>>()
            .add_systems(RenderStartup, init_particle_gpu)
            .add_systems(
                Render,
                (
                    prepare_particle_resources.in_set(RenderSystems::PrepareResources),
                    prepare_particle_bind_group.in_set(RenderSystems::PrepareBindGroups),
                    queue_particles
                        .run_if(crate::panorama::world_passes_enabled)
                        .in_set(RenderSystems::Queue),
                ),
            );
    }
}

#[derive(Resource)]
struct ParticleGpu {
    sampler: Sampler,
    texture_view: Option<TextureView>,
    texture: Option<Texture>,
    base: Option<Arc<[u8]>>,
    uploaded_seq: u64,
    buffer: Option<Buffer>,
    capacity: usize,
    opaque_range: Range<u32>,
    blend_range: Range<u32>,
    add_range: Range<u32>,
    blend_centroid: [f32; 3],
    add_centroid: [f32; 3],
    bind_group: Option<BindGroup>,
    bound: (Option<BufferId>, Option<BufferId>, u64),
}

fn init_particle_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("particle atlas sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(ParticleGpu {
        sampler,
        texture_view: None,
        texture: None,
        base: None,
        uploaded_seq: 0,
        buffer: None,
        capacity: 0,
        opaque_range: 0..0,
        blend_range: 0..0,
        add_range: 0..0,
        blend_centroid: [0.0; 3],
        add_centroid: [0.0; 3],
        bind_group: None,
        bound: (None, None, 0),
    });
}

fn prepare_particle_resources(
    frame: Res<ParticleGpuFrame>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<ParticleGpu>,
    staging: Option<Res<crate::upload_staging::BufferUploadStaging>>,
) {
    if let Some(base) = &frame.base
        && gpu.base.as_ref().is_none_or(|old| !Arc::ptr_eq(old, base))
        && base.len() == (ATLAS_SIDE * ATLAS_SIDE * 4) as usize
    {
        let texture = render_device.create_texture(&TextureDescriptor {
            label: Some("particle atlas"),
            size: Extent3d {
                width: ATLAS_SIDE,
                height: ATLAS_SIDE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_rect(
            &render_queue,
            &texture,
            [0, 0],
            [ATLAS_SIDE, ATLAS_SIDE],
            base,
        );
        gpu.texture_view = Some(texture.create_view(&TextureViewDescriptor::default()));
        gpu.texture = Some(texture);
        gpu.base = Some(Arc::clone(base));
        gpu.uploaded_seq = 0;
        gpu.bind_group = None;
    }
    if let Some(texture) = &gpu.texture
        && frame.patch_seq != gpu.uploaded_seq
    {
        for patch in frame.patches.iter().filter(|p| p.seq > gpu.uploaded_seq) {
            write_rect(
                &render_queue,
                texture,
                [patch.x, patch.y],
                [patch.width, patch.height],
                &patch.rgba8,
            );
        }
        let seq = frame.patch_seq;
        gpu.uploaded_seq = seq;
    }

    let (opaque, blend, add) = (frame.opaque.len(), frame.blend.len(), frame.add.len());
    gpu.blend_centroid = frame.blend_centroid;
    gpu.add_centroid = frame.add_centroid;
    gpu.opaque_range = 0..opaque as u32;
    gpu.blend_range = opaque as u32..(opaque + blend) as u32;
    gpu.add_range = (opaque + blend) as u32..(opaque + blend + add) as u32;
    let total = opaque + blend + add;
    if total == 0 {
        return;
    }
    if gpu.capacity < total || gpu.buffer.is_none() {
        let capacity = total.next_power_of_two().max(MIN_CAPACITY);
        gpu.buffer = Some(render_device.create_buffer(&BufferDescriptor {
            label: Some("particle instances"),
            size: capacity as u64 * INSTANCE_BYTES,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
        gpu.capacity = capacity;
        gpu.bind_group = None;
    }
    if let Some(buffer) = &gpu.buffer {
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "particles.instance_write",
            instances = total,
            bytes = total as u64 * INSTANCE_BYTES
        )
        .entered();
        crate::upload_staging::write_batch(
            staging.as_deref(),
            &render_device,
            &render_queue,
            &[
                (buffer, 0, bytemuck::cast_slice(&frame.opaque[..])),
                (
                    buffer,
                    opaque as u64 * INSTANCE_BYTES,
                    bytemuck::cast_slice(&frame.blend[..]),
                ),
                (
                    buffer,
                    (opaque + blend) as u64 * INSTANCE_BYTES,
                    bytemuck::cast_slice(&frame.add[..]),
                ),
            ],
        );
    }
}

fn write_rect(
    queue: &RenderQueue,
    texture: &Texture,
    origin: [u32; 2],
    size: [u32; 2],
    rgba: &[u8],
) {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!("particles.texture_write", bytes = rgba.len()).entered();
    queue.write_texture(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Origin3d {
                x: origin[0],
                y: origin[1],
                z: 0,
            },
            aspect: Default::default(),
        },
        rgba,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: Some(size[1]),
        },
        Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
    );
}

struct ParticleSpecializer;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum ParticleMode {
    Opaque,
    Blend,
    Add,
}

#[derive(Resource)]
struct ParticlePipeline {
    variants: Variants<RenderPipeline, ParticleSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ParticlePipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "particle bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
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
                        min_binding_size: BufferSize::new(INSTANCE_BYTES),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("particle pipeline".into()),
            layout: vec![bind_group_layout.clone(), crate::lighting::layout()],
            vertex: VertexState {
                shader: PARTICLE_SHADER_HANDLE,
                entry_point: Some("particle_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: PARTICLE_SHADER_HANDLE,
                entry_point: Some("particle_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: crate::SCENE_COLOR_FORMAT,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::COLOR,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(ParticleSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct ParticlePipelineKey {
    msaa: Msaa,
    hdr: bool,
    material: ParticleMode,
}

impl Specializer<RenderPipeline> for ParticleSpecializer {
    type Key = ParticlePipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        descriptor
            .depth_stencil
            .as_mut()
            .unwrap()
            .depth_write_enabled = Some(key.material == ParticleMode::Opaque);
        let target = descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap();
        target.format = if key.hdr {
            crate::SCENE_HDR_FORMAT
        } else {
            crate::SCENE_COLOR_FORMAT
        };
        target.blend = match key.material {
            ParticleMode::Opaque => None,
            ParticleMode::Add => Some(BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::SrcAlpha,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent {
                    src_factor: BlendFactor::Zero,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
            }),
            ParticleMode::Blend => Some(BlendState::ALPHA_BLENDING),
        };
        Ok(key)
    }
}

fn prepare_particle_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<ParticlePipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<ParticleGpu>,
) {
    let (Some(view_binding), Some(view_buffer)) = (
        view_uniforms.uniforms.binding(),
        view_uniforms.uniforms.buffer(),
    ) else {
        gpu.bind_group = None;
        return;
    };
    let (Some(buffer), Some(texture_view)) = (gpu.buffer.as_ref(), gpu.texture_view.as_ref())
    else {
        gpu.bind_group = None;
        return;
    };
    let key = (
        Some(view_buffer.id()),
        Some(buffer.id()),
        gpu.uploaded_seq.min(1),
    );
    if gpu.bind_group.is_some() && gpu.bound == key {
        return;
    }
    let bind_group = render_device.create_bind_group(
        "particle bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(texture_view),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
        ],
    );
    gpu.bind_group = Some(bind_group);
    gpu.bound = key;
}

fn queue_particles(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ParticlePipeline>,
    gpu: Res<ParticleGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    if gpu.opaque_range.is_empty() && gpu.blend_range.is_empty() && gpu.add_range.is_empty() {
        return;
    }
    let (opaque_draw, alpha_draw, add_draw) = {
        let functions = draw_functions.read();
        (
            functions.id::<DrawParticles<{ ParticleMode::Opaque as u8 }>>(),
            functions.id::<DrawParticles<{ ParticleMode::Blend as u8 }>>(),
            functions.id::<DrawParticles<{ ParticleMode::Add as u8 }>>(),
        )
    };
    for (view_entity, main_entity, view, extracted_camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for (material, draw_function, populated) in [
            (
                ParticleMode::Opaque,
                opaque_draw,
                !gpu.opaque_range.is_empty(),
            ),
            (ParticleMode::Blend, alpha_draw, !gpu.blend_range.is_empty()),
            (ParticleMode::Add, add_draw, !gpu.add_range.is_empty()),
        ] {
            if !populated {
                continue;
            }
            let Ok(pipeline_id) = pipeline.variants.specialize(
                &pipeline_cache,
                ParticlePipelineKey {
                    msaa: *msaa,
                    hdr: extracted_camera.hdr,
                    material,
                },
            ) else {
                continue;
            };
            let distance = match material {
                ParticleMode::Opaque => f32::NEG_INFINITY,
                ParticleMode::Blend => view
                    .rangefinder3d()
                    .distance(&Vec3::from_array(gpu.blend_centroid)),
                ParticleMode::Add => view
                    .rangefinder3d()
                    .distance(&Vec3::from_array(gpu.add_centroid)),
            };
            crate::transparent_phase::add(
                phase,
                Transparent3d {
                    sorting_info:
                        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                    entity: (view_entity, *main_entity),
                    pipeline: pipeline_id,
                    draw_function,
                    // Depth-writing sprites must precede translucent terrain and surface overlays.
                    distance,
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                },
            );
        }
    }
}

type DrawParticles<const MATERIAL: u8> = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuParticles as usize },
    (
        SetItemPipeline,
        SetParticleBindGroup<0>,
        crate::lighting::SetWorldLightmap,
        DrawParticleRange<MATERIAL>,
    ),
>;

struct SetParticleBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetParticleBindGroup<I> {
    type Param = SRes<ParticleGpu>;
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
        pass.set_bind_group(I, bind_group, &[view_offset.offset]);
        RenderCommandResult::Success
    }
}

struct DrawParticleRange<const MATERIAL: u8>;

impl<P: PhaseItem, const MATERIAL: u8> RenderCommand<P> for DrawParticleRange<MATERIAL> {
    type Param = SRes<ParticleGpu>;
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
        let range = if MATERIAL == ParticleMode::Add as u8 {
            gpu.add_range.clone()
        } else if MATERIAL == ParticleMode::Opaque as u8 {
            gpu.opaque_range.clone()
        } else {
            gpu.blend_range.clone()
        };
        if range.is_empty() {
            return RenderCommandResult::Skip;
        }
        pass.draw(0..6, range);
        RenderCommandResult::Success
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for ParticlePipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        for material in [ParticleMode::Opaque, ParticleMode::Blend, ParticleMode::Add] {
            ids.push(self.variants.specialize(
                cache,
                ParticlePipelineKey {
                    msaa: view.msaa,
                    hdr: view.hdr,
                    material,
                },
            )?);
        }
        Ok(())
    }
}
