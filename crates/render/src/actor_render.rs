use std::mem::size_of;
mod artwork;
use artwork::{GpuArtwork, draw_spans};

use crate::actor::{
    ActorDrawFrame, ActorDrawWitness, ActorGpuInstance, ActorPrepareWitness, ActorPresentationGate,
    ActorQueueWitness, ActorRenderFrame, ActorRigGeometrySpan, ActorRigVertex, ActorRuntimeWitness,
    ActorSubmitWitness, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE, gpu::ActorDrawTracker,
};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParam, SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem,
            RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass,
            ViewBinnedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache, PollType,
            RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDataOrder,
            TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
            TextureView, TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

const ACTOR_SHADER_HANDLE: Handle<Shader> = uuid_handle!("09d34708-6fd4-4c65-b27e-ce22f172cc73");
#[cfg(test)]
const ACTOR_SHADER_SOURCE: &str = include_str!("actor.wgsl");

#[derive(Debug, Clone, Copy, Default)]
pub struct ActorRenderPlugin;

impl Plugin for ActorRenderPlugin {
    fn build(&self, app: &mut App) {
        install_actor_render(app);
    }

    fn finish(&self, app: &mut App) {
        install_actor_render(app);
    }
}

#[derive(Resource)]
struct ActorRenderInstalled;

fn install_actor_render(app: &mut App) {
    app.init_resource::<ActorRenderFrame>()
        .init_resource::<ActorPresentationGate>()
        .init_resource::<ActorRuntimeWitness>();
    crate::lighting::install(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<ActorRenderInstalled>()
    {
        return;
    }
    let presentation_gate = app.world().resource::<ActorPresentationGate>().clone();
    let runtime_witness = app.world().resource::<ActorRuntimeWitness>().clone();
    app.add_plugins(ExtractResourcePlugin::<ActorRenderFrame>::default());
    load_internal_asset!(
        app,
        ACTOR_SHADER_HANDLE,
        "actor.wgsl",
        crate::shader_safety::from_actor_wgsl,
        crate::actor::ACTOR_GPU_INSTANCE_WORDS
    );
    crate::nametag_render::install_nametag_render(app);
    crate::install_opaque_phase_reset(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .insert_resource(ActorRenderInstalled)
        .insert_resource(presentation_gate)
        .insert_resource(runtime_witness)
        .init_resource::<ActorPipeline>()
        .init_resource::<ActorDrawTracker>()
        .add_render_command::<Opaque3d, DrawActorCommands>()
        .add_systems(RenderStartup, init_actor_gpu)
        .add_systems(
            Render,
            (
                prepare_actor_resources.in_set(RenderSystems::PrepareResources),
                prepare_actor_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_actors
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
                submit_actor_presented_frame
                    .in_set(RenderSystems::Render)
                    .after(bevy::render::renderer::render_system),
            ),
        );
}

#[derive(Resource)]
struct ActorGpu {
    artwork: GpuArtwork,
    player_material: Buffer,
    neutral_material: Buffer,
    color_mask_material: Buffer,
    multitexture_material: Buffer,
    spans: Vec<crate::actor::gpu::ActorDrawSpan>,
    artwork_identity: [u8; 32],
    artwork_current: bool,
    instance_buffer: Buffer,
    previous_bone_buffer: Buffer,
    current_bone_buffer: Buffer,
    geometry_vertices: crate::actor::gpu::SegmentedVertexBuffer,
    geometry_span_buffer: Option<Buffer>,
    instance_count: u32,
    maximum_vertex_count: u32,
    skin_texture: Option<Texture>,
    skin_view: Option<TextureView>,
    sampler: Sampler,
    bind_group: Option<BindGroup>,
    frame_generation: u64,
    geometry_revision: u64,
    skin_revision: u64,
    view_buffer_id: Option<BufferId>,
    manifest: std::sync::Arc<[crate::actor::ActorDrawManifestEntry]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActorSkinUploadPlan {
    layer_count: u32,
}

fn actor_skin_upload_plan(frame: &ActorRenderFrame) -> Option<ActorSkinUploadPlan> {
    if frame.rig.instances.is_empty()
        || !frame.skins_rgba8.len().is_multiple_of(STANDARD_SKIN_BYTES)
    {
        return None;
    }
    let layer_count = frame.skins_rgba8.len() / STANDARD_SKIN_BYTES;
    if layer_count > crate::actor::MAX_RENDERED_PLAYERS
        || frame
            .rig
            .instances
            .iter()
            .enumerate()
            .any(|(index, instance)| {
                frame.instance_pages.get(index).copied().unwrap_or(0) == 0
                    && instance.texture_layer as usize >= layer_count
            })
    {
        return None;
    }
    Some(ActorSkinUploadPlan {
        layer_count: u32::try_from(layer_count).ok()?,
    })
}

fn init_actor_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("nearest shared actor artwork sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: FilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(ActorGpu {
        artwork: GpuArtwork::default(),
        player_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("unchanged player material class"),
            contents: bytemuck::cast_slice(&[0u32; 4]),
            usage: BufferUsages::UNIFORM,
        }),
        neutral_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("neutral binary-alpha material class"),
            contents: bytemuck::cast_slice(&[1u32, 0, 0, 0]),
            usage: BufferUsages::UNIFORM,
        }),
        color_mask_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("native actor color-mask material"),
            // The shader consumes the second word as a Boolean, not a duplicated class ID.
            contents: bytemuck::cast_slice(&[0u32, 1, 0, 0]),
            usage: BufferUsages::UNIFORM,
        }),
        multitexture_material: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("native actor three-sampler material"),
            contents: bytemuck::cast_slice(&[0u32, 0, 1, 0]),
            usage: BufferUsages::UNIFORM,
        }),
        spans: Vec::new(),
        artwork_identity: [0; 32],
        artwork_current: false,
        instance_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor instance arena"),
            size: (crate::actor::MAX_ACTOR_RENDER_INSTANCES * size_of::<ActorGpuInstance>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        previous_bone_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor previous-bone arena"),
            size: (crate::actor::MAX_ACTOR_BONE_ARENA_BYTES / 2) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        current_bone_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor current-bone arena"),
            size: (crate::actor::MAX_ACTOR_BONE_ARENA_BYTES / 2) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        geometry_vertices: default(),
        geometry_span_buffer: None,
        instance_count: 0,
        maximum_vertex_count: 0,
        skin_texture: None,
        skin_view: None,
        sampler,
        bind_group: None,
        frame_generation: u64::MAX,
        geometry_revision: u64::MAX,
        skin_revision: u64::MAX,
        view_buffer_id: None,
        manifest: std::sync::Arc::from([]),
    });
}

fn prepare_actor_resources(
    frame: Res<ActorRenderFrame>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<ActorGpu>,
    witness: Res<ActorRuntimeWitness>,
    gate: Res<ActorPresentationGate>,
    tracker: Res<ActorDrawTracker>,
) {
    let rig = &frame.rig;
    let artwork_valid = gpu
        .artwork
        .prepare(&frame.artwork, &render_device, &render_queue);
    gpu.artwork_current = artwork_valid;
    if gpu.artwork_identity != frame.artwork.identity() {
        gate.clear();
        tracker.clear();
        gpu.artwork_identity = frame.artwork.identity();
        gpu.frame_generation = u64::MAX;
    }
    let skin_upload_plan = actor_skin_upload_plan(&frame);
    let structurally_valid = !rig.instances.is_empty()
        && rig.instances.len() <= crate::actor::MAX_ACTOR_RENDER_INSTANCES
        && rig.previous_bones.len() == rig.current_bones.len()
        && rig.previous_bones.len()
            <= crate::actor::MAX_ACTOR_RENDER_INSTANCES * crate::actor::MAX_RENDER_BONES_PER_ACTOR
        && rig.manifest.len() == rig.instances.len()
        && rig.maximum_vertex_count != 0
        && skin_upload_plan.is_some()
        && frame.instance_pages.len() == rig.instances.len();
    if gpu.geometry_revision != rig.geometry_revision {
        gate.clear();
        tracker.clear();
        gpu.frame_generation = u64::MAX;
        // A new skin model or item mesh uploads only its own vertices.
        gpu.geometry_vertices.sync(
            &render_device,
            &render_queue,
            "shared actor rig vertices",
            &rig.geometry_vertices,
        );
        gpu.geometry_span_buffer = (!rig.geometry_spans.is_empty()).then(|| {
            render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("shared actor rig geometry spans"),
                contents: bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(&rig.geometry_spans),
                usage: BufferUsages::STORAGE,
            })
        });
        gpu.geometry_revision = rig.geometry_revision;
        gpu.bind_group = None;
        gpu.artwork.invalidate_bindings();
    }
    if gpu.frame_generation != rig.frame_generation {
        let lifetime_changed = gpu.manifest.len() != rig.manifest.len()
            || gpu
                .manifest
                .iter()
                .zip(rig.manifest.iter())
                .any(|(old, new)| {
                    let old = old.identity;
                    let new = new.identity;
                    (
                        old.session_id,
                        old.dimension,
                        old.runtime_id,
                        old.spawn_revision,
                    ) != (
                        new.session_id,
                        new.dimension,
                        new.runtime_id,
                        new.spawn_revision,
                    )
                });
        if lifetime_changed {
            gate.clear();
            tracker.clear();
        }
        if structurally_valid {
            render_queue.write_buffer(
                &gpu.instance_buffer,
                0,
                bytemuck::cast_slice::<ActorGpuInstance, u8>(&rig.instances),
            );
            render_queue.write_buffer(
                &gpu.previous_bone_buffer,
                0,
                bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&rig.previous_bones),
            );
            render_queue.write_buffer(
                &gpu.current_bone_buffer,
                0,
                bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&rig.current_bones),
            );
            gpu.instance_count = rig.instances.len() as u32;
            gpu.maximum_vertex_count = rig.maximum_vertex_count;
            gpu.manifest = std::sync::Arc::clone(&rig.manifest);
            gpu.spans = draw_spans(&frame.instance_pages, &rig.instances, &rig.geometry_spans);
        } else {
            gpu.instance_count = 0;
            gpu.maximum_vertex_count = 0;
            gpu.manifest = std::sync::Arc::from([]);
            gpu.spans.clear();
            gate.clear();
            tracker.clear();
        }
        gpu.frame_generation = rig.frame_generation;
    }
    if gpu.skin_revision != frame.skin_revision
        || (structurally_valid && gpu.skin_texture.is_none())
    {
        gate.clear();
        tracker.clear();
        let Some(plan) = skin_upload_plan else {
            gpu.instance_count = 0;
            gpu.skin_revision = frame.skin_revision;
            gpu.bind_group = None;
            witness.observe_prepare(ActorPrepareWitness {
                input_instances: rig.instances.len(),
                input_manifest: rig.manifest.len(),
                skin_bytes: frame.skins_rgba8.len(),
                skin_plan: false,
                valid: structurally_valid,
                prepared_instances: gpu.instance_count,
                maximum_vertices: gpu.maximum_vertex_count,
            });
            return;
        };
        if gpu.skin_texture.is_none() {
            // wgpu zero-initialises the array; only packed layers are written below.
            let texture = render_device.create_texture(&TextureDescriptor {
                label: Some("bounded normalized server player skins"),
                size: Extent3d {
                    width: STANDARD_SKIN_SIDE as u32,
                    height: STANDARD_SKIN_SIDE as u32,
                    depth_or_array_layers: crate::actor::MAX_RENDERED_PLAYERS as u32,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&TextureViewDescriptor {
                label: Some("bounded normalized server player skin array"),
                dimension: Some(TextureViewDimension::D2Array),
                ..default()
            });
            gpu.skin_texture = Some(texture);
            gpu.skin_view = Some(view);
        }
        if plan.layer_count != 0 {
            render_queue.write_texture(
                TexelCopyTextureInfo {
                    texture: gpu
                        .skin_texture
                        .as_ref()
                        .expect("player allocation initialized"),
                    mip_level: 0,
                    origin: default(),
                    aspect: default(),
                },
                &frame.skins_rgba8,
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(STANDARD_SKIN_SIDE as u32 * 4),
                    rows_per_image: Some(STANDARD_SKIN_SIDE as u32),
                },
                Extent3d {
                    width: STANDARD_SKIN_SIDE as u32,
                    height: STANDARD_SKIN_SIDE as u32,
                    depth_or_array_layers: plan.layer_count,
                },
            );
        }
        gpu.skin_revision = frame.skin_revision;
        gpu.bind_group = None;
    }
    witness.observe_prepare(ActorPrepareWitness {
        input_instances: rig.instances.len(),
        input_manifest: rig.manifest.len(),
        skin_bytes: frame.skins_rgba8.len(),
        skin_plan: skin_upload_plan.is_some(),
        valid: structurally_valid,
        prepared_instances: gpu.instance_count,
        maximum_vertices: gpu.maximum_vertex_count,
    });
}

struct ActorPipelineSpecializer;

#[derive(Resource)]
struct ActorPipeline {
    variants: Variants<RenderPipeline, ActorPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ActorPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = actor_bind_group_layout();
        let descriptor = actor_pipeline_descriptor(bind_group_layout.clone());
        Self {
            variants: Variants::new(ActorPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

fn actor_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "instanced actor bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                // The fragment stage reads the camera position for distance fog.
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
                    min_binding_size: BufferSize::new(size_of::<ActorGpuInstance>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigVertex>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigGeometrySpan>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 5,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 6,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 7,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 8,
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

fn actor_pipeline_descriptor(
    bind_group_layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("bounded shared actor pipeline".into()),
        layout: vec![bind_group_layout, crate::lighting::layout()],
        vertex: VertexState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_vertex".into()),
            buffers: vec![],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: CompareFunction::GreaterEqual,
            stencil: default(),
            bias: default(),
        }),
        ..default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct ActorPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for ActorPipelineSpecializer {
    type Key = ActorPipelineKey;

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

fn prepare_actor_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<ActorPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<ActorGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        return;
    };
    let Some(geometry_vertex_buffer) = gpu.geometry_vertices.buffer() else {
        gpu.bind_group = None;
        return;
    };
    let Some(geometry_span_buffer) = gpu.geometry_span_buffer.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let Some(skin_view) = gpu.skin_view.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.bind_group.is_some()
        && gpu.view_buffer_id == Some(view_buffer.id())
        && gpu
            .artwork
            .pages
            .iter()
            .all(|page| page.bind_group.is_some())
    {
        return;
    }
    let generic_groups: Vec<_> = gpu
        .artwork
        .pages
        .iter()
        .map(|page| {
            render_device.create_bind_group(
                "neutral actor page bind group",
                &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: view_binding.clone(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: gpu.instance_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: geometry_vertex_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: geometry_span_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 4,
                        resource: gpu.previous_bone_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 5,
                        resource: gpu.current_bone_buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 6,
                        resource: BindingResource::TextureView(&page.view),
                    },
                    BindGroupEntry {
                        binding: 7,
                        resource: BindingResource::Sampler(&gpu.sampler),
                    },
                    BindGroupEntry {
                        binding: 8,
                        resource: if page.multitexture {
                            gpu.multitexture_material.as_entire_binding()
                        } else if page.color_mask {
                            gpu.color_mask_material.as_entire_binding()
                        } else {
                            gpu.neutral_material.as_entire_binding()
                        },
                    },
                ],
            )
        })
        .collect();
    gpu.bind_group = Some(render_device.create_bind_group(
        "instanced standard actor bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: gpu.instance_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: geometry_vertex_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: geometry_span_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: gpu.previous_bone_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 5,
                resource: gpu.current_bone_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::TextureView(skin_view),
            },
            BindGroupEntry {
                binding: 7,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
            BindGroupEntry {
                binding: 8,
                resource: gpu.player_material.as_entire_binding(),
            },
        ],
    ));
    for (page, group) in gpu.artwork.pages.iter_mut().zip(generic_groups) {
        page.bind_group = Some(group);
    }
    gpu.view_buffer_id = Some(view_buffer.id());
}

#[derive(SystemParam)]
struct QueueActorParams<'w, 's> {
    pipeline_cache: Res<'w, PipelineCache>,
    pipeline: ResMut<'w, ActorPipeline>,
    gpu: Res<'w, ActorGpu>,
    phases: ResMut<'w, ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<'w, DrawFunctions<Opaque3d>>,
    views: Query<
        'w,
        's,
        (
            Entity,
            &'static MainEntity,
            &'static ExtractedView,
            &'static Msaa,
        ),
    >,
    draw_tracker: Res<'w, ActorDrawTracker>,
    witness: Res<'w, ActorRuntimeWitness>,
}

fn queue_actors(
    mut params: QueueActorParams<'_, '_>,
    mut next_tick: Local<Tick>,
    mut next_draw_generation: Local<u64>,
) {
    params.draw_tracker.clear();
    let view_count = params.views.iter().count();
    if params.gpu.instance_count == 0 || params.gpu.bind_group.is_none() {
        params.witness.observe_queue(ActorQueueWitness {
            prepared_instances: params.gpu.instance_count,
            bind_group: params.gpu.bind_group.is_some(),
            view_count,
            queued: false,
        });
        return;
    }
    let draw_function = params.draw_functions.read().id::<DrawActorCommands>();
    let mut queued = false;
    let mut intended_view = None;
    for (view_entity, main_entity, view, msaa) in &params.views {
        let Some(phase) = params.phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = params.pipeline.variants.specialize(
            &params.pipeline_cache,
            ActorPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };
        let this_tick = next_tick.get() + 1;
        next_tick.set(this_tick);
        phase.add(
            Opaque3dBatchSetKey {
                draw_function,
                pipeline: pipeline_id,
                material_bind_group_index: None,
                lightmap_slab: None,
                vertex_slab: default(),
                index_slab: None,
            },
            Opaque3dBinKey {
                asset_id: AssetId::<Shader>::invalid().untyped(),
            },
            (view_entity, *main_entity),
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
            *next_tick,
        );
        queued = true;
        intended_view = Some(intended_view.map_or(view_entity.to_bits(), |current: u64| {
            current.min(view_entity.to_bits())
        }));
    }
    if queued {
        let Some(draw_generation) = next_draw_generation.checked_add(1) else {
            return;
        };
        *next_draw_generation = draw_generation;
        let _ = params.draw_tracker.begin(
            ActorDrawFrame {
                artwork_identity: params.gpu.artwork_identity,
                skin_revision: params.gpu.skin_revision,
                geometry_revision: params.gpu.geometry_revision,
                frame_generation: params.gpu.frame_generation,
                draw_generation,
                manifest: std::sync::Arc::clone(&params.gpu.manifest),
            },
            intended_view.expect("queued view exists"),
            &params.gpu.spans,
        );
    }
    params.witness.observe_queue(ActorQueueWitness {
        prepared_instances: params.gpu.instance_count,
        bind_group: params.gpu.bind_group.is_some(),
        view_count,
        queued,
    });
}

type DrawActorCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawActors,
);

struct DrawActors;

impl<P: PhaseItem> RenderCommand<P> for DrawActors {
    type Param = (
        SRes<ActorGpu>,
        SRes<ActorDrawTracker>,
        SRes<ActorRuntimeWitness>,
    );
    type ViewQuery = (Entity, Read<ViewUniformOffset>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        params: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (gpu, tracker, witness) = params;
        let gpu = gpu.into_inner();
        let tracker = tracker.into_inner();
        let mut executed_instances = 0;
        let mut bound_page = None;
        for span in &gpu.spans {
            if span.page != 0 && !gpu.artwork_current {
                continue;
            }
            if bound_page != Some(span.page) {
                let bind_group = if span.page == 0 {
                    gpu.bind_group.as_ref()
                } else {
                    gpu.artwork
                        .pages
                        .get(usize::from(span.page) - 1)
                        .and_then(|page| page.bind_group.as_ref())
                };
                let Some(bind_group) = bind_group else {
                    continue;
                };
                pass.set_bind_group(0, bind_group, &[view.1.offset]);
                bound_page = Some(span.page);
            }
            pass.draw(0..span.vertex_count, span.first..span.first + span.count);
            tracker.record_draw(view.0.to_bits(), *span);
            executed_instances += span.count;
        }
        witness.into_inner().observe_draw(ActorDrawWitness {
            executed: executed_instances != 0,
            instances: executed_instances,
            maximum_vertices: gpu.maximum_vertex_count,
        });
        RenderCommandResult::Success
    }
}

fn submit_actor_presented_frame(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    tracker: Res<ActorDrawTracker>,
    gate: Res<ActorPresentationGate>,
    witness: Res<ActorRuntimeWitness>,
) {
    let Some(draw) = tracker.take_drawn() else {
        witness.observe_submit(ActorSubmitWitness {
            drawn_frame: false,
            exact: false,
            reserved: false,
            acknowledged: false,
        });
        if let Err(error) = render_device.poll(PollType::Poll) {
            bevy::log::warn!(
                ?error,
                "could not nonblockingly poll actor presentation fence"
            );
        }
        return;
    };
    let exact = draw.is_exact();
    let Some(token) = gate.try_reserve_callback(draw) else {
        witness.observe_submit(ActorSubmitWitness {
            drawn_frame: true,
            exact,
            reserved: false,
            acknowledged: false,
        });
        return;
    };
    witness.observe_submit(ActorSubmitWitness {
        drawn_frame: true,
        exact,
        reserved: true,
        acknowledged: false,
    });
    let present_returned_at = std::time::Instant::now();
    let encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("actor presented-frame completion sentinel"),
    });
    let command_buffer = encoder.finish();
    let callback_gate = gate.clone();
    let callback_witness = witness.clone();
    command_buffer.on_submitted_work_done(move || {
        let acknowledged =
            callback_gate.publish_reserved(token, present_returned_at, std::time::Instant::now());
        callback_witness.observe_submit(ActorSubmitWitness {
            drawn_frame: true,
            exact: true,
            reserved: true,
            acknowledged,
        });
    });
    render_queue.submit([command_buffer]);
}

#[cfg(test)]
mod tests;
