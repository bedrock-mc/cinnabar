//! GPU particle atlas and instance uploads, with depth-writing, blended and additive draws.

use crate::queue_writes::ProfiledQueue as _;
use bevy::prelude::IntoScheduleConfigs;
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_resource::ShaderType;
use render_scene::{ParticleGpuFrame, ParticleSimulation};

use std::{ops::Range, sync::Arc};

#[cfg(test)]
use bevy::prelude::Mut;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    math::FloatOrd,
    platform::collections::HashMap,
    prelude::{
        App, BevyError, Commands, Entity, FromWorld, Handle, Msaa, Plugin, Query, Res, ResMut,
        Resource, Result, Shader, Vec3, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand, RenderCommandResult,
            SetItemPipeline, SortedPhaseItem, SortedRenderPhase, TrackedRenderPass,
            ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferSize, BufferUsages, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, Origin3d, PipelineCache,
            RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, Specializer, SpecializerKey, TexelCopyBufferLayout,
            TexelCopyTextureInfo, Texture, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, RetainedViewEntity, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use particles::ParticleInstance;

const PARTICLE_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b2f9c3e-4d1a-4e8b-9a57-3c0f1d7a2b64");
const INSTANCE_BYTES: u64 = std::mem::size_of::<ParticleInstance>() as u64;
const MIN_CAPACITY: usize = 256;
const MERGE_RETAINED_CAPACITY_FACTOR: usize = 4;

#[path = "particle_render/atlas.rs"]
mod atlas;

#[cfg(test)]
#[path = "particle_render/atlas_fault_tests.rs"]
mod atlas_fault_tests;

#[cfg(all(test, target_os = "macos"))]
#[path = "particle_render/upload_tests.rs"]
mod upload_tests;

#[cfg(test)]
#[path = "particle_render/tests.rs"]
pub(crate) mod tests;

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
            "particles.wesl",
            crate::shader_safety::from_wesl
        );
        crate::pipeline_warmup::register::<ParticlePipeline>(app);
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        crate::transparent_phase::install(render_app);
        render_app
            .init_resource::<ParticlePipeline>()
            .init_resource::<ParticlePhase>()
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
                        .run_if(render_scene::panorama::world_passes_enabled)
                        .in_set(RenderSystems::Queue),
                ),
            );
    }
}

#[derive(Resource)]
pub(crate) struct ParticleGpu {
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
    bind_group: Option<BindGroup>,
    bound: (Option<BufferId>, Option<BufferId>, u64),
    atlas: atlas::AtlasUpload,
}

#[cfg(feature = "shaderpacks")]
impl ParticleGpu {
    /// The uploaded particle atlas with its dynamic tiles, which shader-pack particle draws
    /// read through a gamma-byte view rather than a copy of their own.
    pub(crate) fn atlas_texture(&self) -> Option<&Texture> {
        self.texture.as_ref()
    }
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
        bind_group: None,
        bound: (None, None, 0),
        atlas: atlas::AtlasUpload::default(),
    });
}

pub(crate) fn prepare_particle_resources(
    frame: Res<ParticleGpuFrame>,
    uploads: crate::upload_staging::FrameUploads,
    mut gpu: ResMut<ParticleGpu>,
) {
    atlas::prepare(&frame, &uploads.device, &uploads.queue, &mut gpu);

    let (opaque, blend, add) = (frame.opaque.len(), frame.blend.len(), frame.add.len());
    gpu.opaque_range = 0..opaque as u32;
    gpu.blend_range = opaque as u32..(opaque + blend) as u32;
    gpu.add_range = (opaque + blend) as u32..(opaque + blend + add) as u32;
    let total = opaque + blend + add;
    if total == 0 {
        return;
    }
    if gpu.capacity < total || gpu.buffer.is_none() {
        let capacity = total.next_power_of_two().max(MIN_CAPACITY);
        gpu.buffer = Some(uploads.device.create_buffer(&BufferDescriptor {
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
        uploads.write_batch(&[
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
        ]);
    }
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
                    format: render_scene::SCENE_COLOR_FORMAT,
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
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
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

/// Queues compact translucent streams at their target-view depths from the extracted frame.
#[allow(
    clippy::too_many_arguments,
    reason = "a queue system's render-world inputs"
)]
fn queue_particles(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ParticlePipeline>,
    frame: Res<ParticleGpuFrame>,
    mut pending: ResMut<ParticlePhase>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
    pack: render_scene::shaderpack::ShaderPackDrawnEffects,
) {
    for draws in pending.0.values_mut() {
        draws.clear_frame();
    }
    let functions = draw_functions.read();
    let draws = [
        functions.id::<DrawParticles<{ ParticleMode::Opaque as u8 }>>(),
        functions.id::<DrawParticles<{ ParticleMode::Blend as u8 }>>(),
        functions.id::<DrawParticles<{ ParticleMode::Add as u8 }>>(),
    ];
    drop(functions);
    for (view_entity, main_entity, view, extracted_camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let mut offset = 0;
        for (material, instances, draw_function) in [
            (ParticleMode::Opaque, &frame.opaque, draws[0]),
            (ParticleMode::Blend, &frame.blend, draws[1]),
            (ParticleMode::Add, &frame.add, draws[2]),
        ] {
            let first = offset;
            offset += instances.len() as u32;
            if instances.is_empty()
                || pack.replaces(view_entity, |drawn| match material {
                    ParticleMode::Opaque => drawn.particles_opaque,
                    ParticleMode::Blend => drawn.particles_translucent,
                    ParticleMode::Add => drawn.particles_additive,
                })
            {
                continue;
            }
            let Ok(pipeline) = pipeline.variants.specialize(
                &pipeline_cache,
                ParticlePipelineKey {
                    msaa: *msaa,
                    hdr: extracted_camera.hdr,
                    material,
                },
            ) else {
                continue;
            };
            let item = |distance, extra_index| Transparent3d {
                sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                entity: (view_entity, *main_entity),
                pipeline,
                draw_function,
                distance,
                batch_range: 0..1,
                extra_index,
                indexed: false,
            };
            if material == ParticleMode::Opaque {
                // Depth-writing sprites must precede translucent terrain and surface overlays.
                crate::transparent_phase::add(
                    phase,
                    item(f32::NEG_INFINITY, PhaseItemExtraIndex::None),
                );
                continue;
            }
            let rangefinder = view.rangefinder3d();
            let streams = pending.0.entry(view.retained_view_entity).or_default();
            streams.push(
                phase.items.len(),
                item(0.0, PhaseItemExtraIndex::None),
                instances.iter().enumerate().map(|(index, instance)| {
                    let position = Vec3::from_slice(&instance.center_light[..3]);
                    (
                        FloatOrd(rangefinder.distance(&position)),
                        first + index as u32,
                    )
                }),
            );
        }
        if let Some(streams) = pending.0.get_mut(&view.retained_view_entity) {
            streams.streams.truncate(streams.active);
        }
    }
}

/// Per-view particle streams and merge storage retained while the view queues particles.
#[derive(Resource, Default)]
pub(crate) struct ParticlePhase(pub(crate) HashMap<RetainedViewEntity, ParticleViewDraws>);

impl ParticlePhase {
    /// Drops storage for absent or idle particle views, bounding the retained view table as well.
    pub(crate) fn retain_views(&mut self, phases: &ViewSortedRenderPhases<Transparent3d>) {
        self.0
            .retain(|view, draws| phases.contains_key(view) && draws.has_pending());
        if self.0.capacity() > self.0.len().saturating_mul(MERGE_RETAINED_CAPACITY_FACTOR) {
            self.0.shrink_to_fit();
        }
    }
}

/// Compact instance order and the draw state shared by one material list.
#[derive(Default)]
struct ParticleStream {
    insertion: usize,
    template: Option<Transparent3d>,
    instances: Vec<(FloatOrd, u32)>,
    next: usize,
}

#[derive(Default)]
pub(crate) struct ParticleViewDraws {
    streams: Vec<ParticleStream>,
    active: usize,
    order: ParticleMergeOrder,
}

/// Reusable source order and destination permutation; phase keys stay in their original map.
#[derive(Default)]
struct ParticleMergeOrder {
    source: Vec<usize>,
    destinations: Vec<usize>,
    emitted: usize,
}

impl ParticleViewDraws {
    /// Reports whether this view has fresh streams awaiting the shared phase merge.
    pub(crate) fn has_pending(&self) -> bool {
        self.active != 0
    }

    /// Expires authored draw state without discarding the view's reusable instance storage.
    fn clear_frame(&mut self) {
        self.active = 0;
        for stream in &mut self.streams {
            stream.template = None;
            stream.instances.clear();
            stream.next = 0;
        }
    }

    /// Refills a material slot and orders it by target-view depth with original index ties.
    fn push(
        &mut self,
        insertion: usize,
        template: Transparent3d,
        instances: impl Iterator<Item = (FloatOrd, u32)>,
    ) {
        if self.active == self.streams.len() {
            self.streams.push(ParticleStream::default());
        }
        let stream = &mut self.streams[self.active];
        stream.instances.clear();
        stream.instances.extend(instances);
        bound_merge_scratch(&mut stream.instances);
        if stream.instances.is_empty() {
            self.streams.truncate(self.active);
            return;
        }
        if !stream.instances.is_sorted_by_key(|instance| instance.0) {
            // GPU indices preserve stable material-list ties without allocating sort scratch.
            stream.instances.sort_unstable();
        }
        stream.insertion = insertion;
        stream.template = Some(template);
        stream.next = 0;
        self.active += 1;
    }

    /// Merges compatible instance runs and applies their order without rebuilding the phase map.
    pub(crate) fn merge(&mut self, phase: &mut SortedRenderPhase<Transparent3d>) {
        self.order.begin(phase);
        let mut next_key = u32::try_from(phase.items.len())
            .expect("transparent phase exceeds entity index capacity");
        let mut source_cursor = 0;
        let mut run: Option<(usize, Transparent3d)> = None;
        loop {
            // Stream order breaks equal-depth ties in the original material queue order.
            let next = self.streams[..self.active]
                .iter()
                .enumerate()
                .filter_map(|(index, stream)| {
                    stream
                        .instances
                        .get(stream.next)
                        .map(|&(depth, _)| (depth, stream.insertion, index))
                })
                .min();
            let Some((depth, insertion, stream_index)) = next else {
                break;
            };
            if let Some(&index) = self.order.source.get(source_cursor) {
                let (_, item) = phase.items.get_index(index).unwrap();
                if item.sort_key() < depth || (item.sort_key() == depth && index < insertion) {
                    self.order.flush_run(phase, &mut run, &mut next_key);
                    self.order.record_source(index);
                    source_cursor += 1;
                    continue;
                }
            }
            let stream = &mut self.streams[stream_index];
            let (_, index) = stream.instances[stream.next];
            stream.next += 1;
            if let Some((previous, item)) = &mut run
                && *previous == stream_index
                && let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } =
                    &mut item.extra_index
                && range.end == index
            {
                range.end = index + 1;
                continue;
            }
            self.order.flush_run(phase, &mut run, &mut next_key);
            let template = stream.template.as_ref().unwrap();
            run = Some((
                stream_index,
                Transparent3d {
                    sorting_info: template.sorting_info,
                    distance: depth.0,
                    entity: template.entity,
                    pipeline: template.pipeline,
                    draw_function: template.draw_function,
                    batch_range: 0..1,
                    extra_index: PhaseItemExtraIndex::IndirectParametersIndex {
                        range: index..index + 1,
                        batch_set_index: None,
                    },
                    indexed: template.indexed,
                },
            ));
        }
        self.order.flush_run(phase, &mut run, &mut next_key);
        while let Some(&index) = self.order.source.get(source_cursor) {
            self.order.record_source(index);
            source_cursor += 1;
        }
        self.order.finish(phase);
        self.clear_frame();
    }
}

impl ParticleMergeOrder {
    /// Sorts source indices with stable queue ties while leaving all keys reserved in the map.
    fn begin(&mut self, phase: &SortedRenderPhase<Transparent3d>) {
        self.source.clear();
        self.source.extend(0..phase.items.len());
        self.source.sort_unstable_by_key(|&index| {
            (phase.items.get_index(index).unwrap().1.sort_key(), index)
        });
        bound_merge_scratch(&mut self.source);
        self.destinations.clear();
        self.destinations.resize(phase.items.len(), 0);
        self.emitted = 0;
    }

    /// Assigns the next merged position to an existing source entry.
    fn record_source(&mut self, index: usize) {
        self.destinations[index] = self.emitted;
        self.emitted += 1;
    }

    /// Appends a completed run and records its destination without displacing any source key.
    fn flush_run(
        &mut self,
        phase: &mut SortedRenderPhase<Transparent3d>,
        run: &mut Option<(usize, Transparent3d)>,
        next_key: &mut u32,
    ) {
        if let Some((_, item)) = run.take() {
            crate::transparent_phase::add_with_cursor(phase, item, next_key);
            self.destinations.push(self.emitted);
            self.emitted += 1;
        }
    }

    /// Applies the permutation in place, then expires ordering data while retaining bounded space.
    fn finish(&mut self, phase: &mut SortedRenderPhase<Transparent3d>) {
        for index in 0..self.destinations.len() {
            while self.destinations[index] != index {
                let destination = self.destinations[index];
                phase.items.swap_indices(index, destination);
                self.destinations.swap(index, destination);
            }
        }
        bound_merge_scratch(&mut self.destinations);
        if phase.items.capacity()
            > phase
                .items
                .len()
                .saturating_mul(MERGE_RETAINED_CAPACITY_FACTOR)
        {
            phase.items.shrink_to_fit();
        }
        self.source.clear();
        self.destinations.clear();
        self.emitted = 0;
    }
}

/// Releases excess scratch after a workload shrinks, retaining at most four times its live length.
fn bound_merge_scratch<T>(scratch: &mut Vec<T>) {
    if scratch.capacity() > scratch.len().saturating_mul(MERGE_RETAINED_CAPACITY_FACTOR) {
        scratch.shrink_to_fit();
    }
}

type DrawParticles<const MATERIAL: u8> = crate::gpu_timing::GpuDrawSpan<
    { render_scene::RuntimeStage::GpuParticles as usize },
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
        item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let range = if MATERIAL == ParticleMode::Opaque as u8 {
            gpu.opaque_range.clone()
        } else {
            let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = item.extra_index()
            else {
                return RenderCommandResult::Skip;
            };
            let admitted = if MATERIAL == ParticleMode::Add as u8 {
                &gpu.add_range
            } else {
                &gpu.blend_range
            };
            if range.start < admitted.start || range.end > admitted.end {
                return RenderCommandResult::Skip;
            }
            range.clone()
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
