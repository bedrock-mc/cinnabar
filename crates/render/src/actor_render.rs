use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    mesh::VertexBufferLayout,
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
            BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical, ColorTargetState,
            ColorWrites, CompareFunction, DepthStencilState, Extent3d, FilterMode, FragmentState,
            PipelineCache, RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey, Texture,
            TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexAttribute, VertexFormat, VertexState,
            VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use bytemuck::{Pod, Zeroable};

use crate::actor::{
    ActorRenderFrame, ActorVertex, STANDARD_BIPED_VERTEX_COUNT, STANDARD_SKIN_BYTES,
    STANDARD_SKIN_SIDE, standard_biped_vertices,
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
    app.init_resource::<ActorRenderFrame>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<ActorRenderInstalled>()
    {
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<ActorRenderFrame>::default());
    load_internal_asset!(app, ACTOR_SHADER_HANDLE, "actor.wgsl", Shader::from_wgsl);
    app.sub_app_mut(RenderApp)
        .insert_resource(ActorRenderInstalled)
        .init_resource::<ActorPipeline>()
        .add_render_command::<Opaque3d, DrawActorCommands>()
        .add_systems(RenderStartup, init_actor_gpu)
        .add_systems(
            Render,
            (
                prepare_actor_resources.in_set(RenderSystems::PrepareResources),
                prepare_actor_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_actors.in_set(RenderSystems::Queue),
            ),
        );
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GpuActorInstance {
    position_yaw: [f32; 4],
    look_skin: [f32; 4],
}

#[derive(Resource)]
struct ActorGpu {
    vertex_buffer: Buffer,
    instance_buffer: Option<Buffer>,
    instance_count: u32,
    skin_texture: Option<Texture>,
    skin_view: Option<TextureView>,
    sampler: Sampler,
    bind_group: Option<BindGroup>,
    instance_revision: u64,
    skin_revision: u64,
    view_buffer_id: Option<BufferId>,
}

fn init_actor_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let vertices = standard_biped_vertices();
    debug_assert_eq!(vertices.len(), STANDARD_BIPED_VERTEX_COUNT);
    let vertex_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("shared standard Bedrock biped vertices"),
        contents: bytemuck::cast_slice::<ActorVertex, u8>(&vertices),
        usage: BufferUsages::VERTEX,
    });
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("nearest standard player skin sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: FilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(ActorGpu {
        vertex_buffer,
        instance_buffer: None,
        instance_count: 0,
        skin_texture: None,
        skin_view: None,
        sampler,
        bind_group: None,
        instance_revision: u64::MAX,
        skin_revision: u64::MAX,
        view_buffer_id: None,
    });
}

fn prepare_actor_resources(
    frame: Res<ActorRenderFrame>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<ActorGpu>,
) {
    if gpu.instance_revision != frame.instance_revision {
        let instances = frame
            .instances
            .iter()
            .map(|instance| GpuActorInstance {
                position_yaw: [
                    instance.position[0],
                    instance.position[1],
                    instance.position[2],
                    instance.yaw_radians,
                ],
                look_skin: [
                    instance.pitch_radians,
                    instance.head_yaw_radians,
                    instance.skin_layer as f32,
                    0.0,
                ],
            })
            .collect::<Vec<_>>();
        gpu.instance_buffer = (!instances.is_empty()).then(|| {
            render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("bounded interpolated actor instances"),
                contents: bytemuck::cast_slice::<GpuActorInstance, u8>(&instances),
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            })
        });
        gpu.instance_count = u32::try_from(instances.len()).expect("bounded actor count");
        gpu.instance_revision = frame.instance_revision;
        gpu.bind_group = None;
    }
    if gpu.skin_revision != frame.skin_revision {
        let layer_count = u32::try_from(frame.instances.len()).expect("bounded skin layer count");
        let expected = frame.instances.len().saturating_mul(STANDARD_SKIN_BYTES);
        if layer_count == 0 || frame.skins_rgba8.len() != expected {
            gpu.skin_texture = None;
            gpu.skin_view = None;
            gpu.instance_count = 0;
        } else {
            let texture = render_device.create_texture_with_data(
                &render_queue,
                &TextureDescriptor {
                    label: Some("bounded normalized server player skins"),
                    size: Extent3d {
                        width: STANDARD_SKIN_SIDE as u32,
                        height: STANDARD_SKIN_SIDE as u32,
                        depth_or_array_layers: layer_count,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8UnormSrgb,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                TextureDataOrder::LayerMajor,
                &frame.skins_rgba8,
            );
            let view = texture.create_view(&TextureViewDescriptor {
                label: Some("bounded normalized server player skin array"),
                dimension: Some(TextureViewDimension::D2Array),
                ..default()
            });
            gpu.skin_texture = Some(texture);
            gpu.skin_view = Some(view);
        }
        gpu.skin_revision = frame.skin_revision;
        gpu.bind_group = None;
    }
}

struct ActorPipelineSpecializer;

#[derive(Resource)]
struct ActorPipeline {
    variants: Variants<RenderPipeline, ActorPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ActorPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "instanced actor bind group layout",
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
                        min_binding_size: BufferSize::new(size_of::<GpuActorInstance>() as u64),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
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
            label: Some("instanced standard Bedrock biped pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: ACTOR_SHADER_HANDLE,
                entry_point: Some("actor_vertex".into()),
                buffers: vec![VertexBufferLayout {
                    array_stride: size_of::<ActorVertex>() as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: vec![
                        VertexAttribute {
                            format: VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x2,
                            offset: 12,
                            shader_location: 1,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32,
                            offset: 20,
                            shader_location: 2,
                        },
                    ],
                }],
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
        };
        Self {
            variants: Variants::new(ActorPipelineSpecializer, descriptor),
            bind_group_layout,
        }
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
    let Some(instance_buffer) = gpu.instance_buffer.as_ref() else {
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
    if gpu.bind_group.is_some() && gpu.view_buffer_id == Some(view_buffer.id()) {
        return;
    }
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
                resource: instance_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(skin_view),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
}

fn queue_actors(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<ActorPipeline>,
    gpu: Res<ActorGpu>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
    mut next_tick: Local<Tick>,
) {
    if gpu.instance_count == 0 || gpu.bind_group.is_none() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawActorCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
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
    }
}

type DrawActorCommands = (SetItemPipeline, SetActorBindGroup<0>, DrawActors);

struct SetActorBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetActorBindGroup<I> {
    type Param = SRes<ActorGpu>;
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

struct DrawActors;

impl<P: PhaseItem> RenderCommand<P> for DrawActors {
    type Param = SRes<ActorGpu>;
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
        pass.set_vertex_buffer(0, gpu.vertex_buffer.slice(..));
        pass.draw(0..STANDARD_BIPED_VERTEX_COUNT as u32, 0..gpu.instance_count);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::{
        app::SubApp,
        asset::Assets,
        core_pipeline::core_3d::{Opaque3d, Transparent3d},
        ecs::schedule::Schedule,
        prelude::{App, Shader},
        render::{
            ExtractSchedule, Render, RenderApp, RenderStartup,
            render_phase::DrawFunctions,
            renderer::{RenderDevice, RenderQueue, WgpuWrapper},
        },
    };

    use super::{ACTOR_SHADER_SOURCE, ActorGpu, ActorRenderInstalled, ActorRenderPlugin};

    fn app_with_noop_render_sub_app() -> App {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let mut render_app = SubApp::new();
        render_app
            .insert_resource(RenderDevice::from(device))
            .insert_resource(RenderQueue(Arc::new(WgpuWrapper::new(queue))))
            .insert_resource(DrawFunctions::<Opaque3d>::default())
            .insert_resource(DrawFunctions::<Transparent3d>::default())
            .add_schedule(Schedule::new(RenderStartup))
            .add_schedule(Render::base_schedule())
            .add_schedule(Schedule::new(ExtractSchedule));
        let mut app = App::new();
        app.insert_resource(Assets::<Shader>::default())
            .insert_sub_app(RenderApp, render_app);
        app
    }

    #[test]
    fn actor_shader_parses_as_wgsl() {
        let source = ACTOR_SHADER_SOURCE.replace(
            "#import bevy_render::view::View",
            "struct View { clip_from_world: mat4x4<f32>, }",
        );
        naga::front::wgsl::parse_str(&source).expect("actor shader parses");
    }

    #[test]
    fn plugin_install_is_idempotent_and_starts_one_shared_gpu_state() {
        let mut app = app_with_noop_render_sub_app();
        app.add_plugins(ActorRenderPlugin);
        app.finish();

        let render_app = app.sub_app_mut(RenderApp);
        assert!(
            render_app
                .world()
                .contains_resource::<ActorRenderInstalled>()
        );
        render_app.world_mut().run_schedule(RenderStartup);
        assert!(render_app.world().contains_resource::<ActorGpu>());
    }
}
