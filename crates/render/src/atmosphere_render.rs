use assets::{AtmosphereRole, AtmosphereTexture};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    mesh::Mesh,
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
            BufferId, BufferInitDescriptor, BufferUsages, Canonical, ColorTargetState, ColorWrites,
            CompareFunction, DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache,
            RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey, Texture,
            TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use crate::{AtmosphereFrame, AtmosphereTextureAssets, cloud_render::install_cloud_render};

const ATMOSPHERE_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("7612c9f4-c152-4f57-95d4-a22d85018c3d");

/// Installs the procedural sky and the shared atmosphere uniform used by
/// distance fog in every custom world pipeline.
#[derive(Debug, Clone, Copy, Default)]
pub struct AtmospherePlugin;

impl Plugin for AtmospherePlugin {
    fn build(&self, app: &mut App) {
        install_atmosphere(app);
    }

    fn finish(&self, app: &mut App) {
        install_atmosphere(app);
    }
}

#[derive(Resource)]
struct AtmosphereRenderInstalled;

pub(crate) fn install_atmosphere(app: &mut App) {
    app.init_resource::<AtmosphereFrame>();
    app.init_resource::<AtmosphereTextureAssets>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<AtmosphereRenderInstalled>()
    {
        return;
    }

    app.add_plugins((
        ExtractResourcePlugin::<AtmosphereFrame>::default(),
        ExtractResourcePlugin::<AtmosphereTextureAssets>::default(),
    ));
    load_internal_asset!(
        app,
        ATMOSPHERE_SHADER_HANDLE,
        "atmosphere.wgsl",
        Shader::from_wgsl
    );
    install_cloud_render(app);

    app.sub_app_mut(RenderApp)
        .insert_resource(AtmosphereRenderInstalled)
        .init_resource::<AtmospherePipeline>()
        .add_render_command::<Opaque3d, DrawAtmosphereCommands>()
        .add_systems(RenderStartup, init_atmosphere_gpu)
        .add_systems(
            Render,
            (
                prepare_atmosphere_uniform.in_set(RenderSystems::PrepareResources),
                prepare_atmosphere_textures.in_set(RenderSystems::PrepareResources),
                prepare_atmosphere_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_atmosphere.in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct AtmosphereGpu {
    pub(crate) buffer: Buffer,
    prepared: Option<PreparedAtmosphereAssets>,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
    bound_asset_identity: Option<[u8; 32]>,
    #[cfg(test)]
    upload_count: u32,
}

struct PreparedAtmosphereAssets {
    identity: [u8; 32],
    _textures: [Texture; 2],
    views: [TextureView; 2],
    sampler: Sampler,
}

fn init_atmosphere_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    AtmosphereFrame::assert_uniform_compat();
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global atmosphere frame"),
        contents: bytemuck::bytes_of(&AtmosphereFrame::default()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    commands.insert_resource(AtmosphereGpu {
        buffer,
        prepared: None,
        bind_group: None,
        view_buffer_id: None,
        bound_asset_identity: None,
        #[cfg(test)]
        upload_count: 0,
    });
}

fn prepare_atmosphere_uniform(
    frame: Res<AtmosphereFrame>,
    gpu: Res<AtmosphereGpu>,
    render_queue: Res<RenderQueue>,
) {
    render_queue.write_buffer(&gpu.buffer, 0, bytemuck::bytes_of(&*frame));
}

fn prepare_atmosphere_textures(
    requested: Res<AtmosphereTextureAssets>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<AtmosphereGpu>,
) {
    let Some(runtime) = requested.runtime() else {
        gpu.prepared = None;
        gpu.bind_group = None;
        gpu.bound_asset_identity = None;
        return;
    };
    if gpu
        .prepared
        .as_ref()
        .is_some_and(|prepared| prepared.identity == requested.identity())
    {
        return;
    }

    let sun = runtime
        .texture(AtmosphereRole::Sun)
        .expect("validated MCBEATM2 always contains the sun texture");
    let moon_phases = runtime
        .texture(AtmosphereRole::MoonPhases)
        .expect("validated MCBEATM2 always contains the moon atlas");
    let (sun_texture, sun_view) =
        upload_atmosphere_texture(&render_device, &render_queue, sun, "pinned vanilla sun");
    let (moon_texture, moon_view) = upload_atmosphere_texture(
        &render_device,
        &render_queue,
        moon_phases,
        "pinned vanilla moon phases",
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("pinned vanilla atmosphere repeat sampler"),
        address_mode_u: AddressMode::Repeat,
        address_mode_v: AddressMode::Repeat,
        address_mode_w: AddressMode::Repeat,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: FilterMode::Nearest,
        ..default()
    });
    gpu.prepared = Some(PreparedAtmosphereAssets {
        identity: requested.identity(),
        _textures: [sun_texture, moon_texture],
        views: [sun_view, moon_view],
        sampler,
    });
    gpu.bind_group = None;
    gpu.view_buffer_id = None;
    gpu.bound_asset_identity = None;
    #[cfg(test)]
    {
        gpu.upload_count += 1;
    }
}

fn upload_atmosphere_texture(
    render_device: &RenderDevice,
    render_queue: &RenderQueue,
    texture: &AtmosphereTexture,
    label: &'static str,
) -> (Texture, TextureView) {
    let gpu_texture = render_device.create_texture_with_data(
        render_queue,
        &TextureDescriptor {
            label: Some(label),
            size: Extent3d {
                width: texture.width,
                height: texture.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        &texture.rgba8,
    );
    let view = gpu_texture.create_view(&TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2),
        ..default()
    });
    (gpu_texture, view)
}

struct AtmospherePipelineSpecializer;

#[derive(Resource)]
struct AtmospherePipeline {
    variants: Variants<RenderPipeline, AtmospherePipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for AtmospherePipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "texture-backed atmosphere bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(AtmosphereFrame::min_size()),
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
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("texture-backed atmosphere pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: ATMOSPHERE_SHADER_HANDLE,
                entry_point: Some("atmosphere_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: ATMOSPHERE_SHADER_HANDLE,
                entry_point: Some("atmosphere_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: TextureFormat::bevy_default(),
                    blend: None,
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
            variants: Variants::new(AtmospherePipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct AtmospherePipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for AtmospherePipelineSpecializer {
    type Key = AtmospherePipelineKey;

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

fn prepare_atmosphere_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<AtmospherePipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<AtmosphereGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        gpu.view_buffer_id = None;
        gpu.bound_asset_identity = None;
        return;
    };
    let Some(prepared) = gpu.prepared.as_ref() else {
        gpu.bind_group = None;
        gpu.view_buffer_id = None;
        gpu.bound_asset_identity = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.bind_group.is_some()
        && gpu.view_buffer_id == Some(view_buffer.id())
        && gpu.bound_asset_identity == Some(prepared.identity)
    {
        return;
    }
    let asset_identity = prepared.identity;
    gpu.bind_group = Some(render_device.create_bind_group(
        "texture-backed atmosphere bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: gpu.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(&prepared.views[0]),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::TextureView(&prepared.views[1]),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::Sampler(&prepared.sampler),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
    gpu.bound_asset_identity = Some(asset_identity);
}

fn queue_atmosphere(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<AtmospherePipeline>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
    mut next_tick: Local<Tick>,
) {
    let draw_function = draw_functions.read().id::<DrawAtmosphereCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            AtmospherePipelineKey {
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
                asset_id: AssetId::<Mesh>::invalid().untyped(),
            },
            (view_entity, *main_entity),
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
            *next_tick,
        );
    }
}

type DrawAtmosphereCommands = (SetItemPipeline, SetAtmosphereBindGroup<0>, DrawAtmosphere);

struct SetAtmosphereBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetAtmosphereBindGroup<I> {
    type Param = SRes<AtmosphereGpu>;
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

struct DrawAtmosphere;

impl<P: PhaseItem> RenderCommand<P> for DrawAtmosphere {
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use assets::{
        AtmosphereRole, AtmosphereTexture, CompiledAtmosphereAssets, RuntimeAtmosphereAssets,
        encode_atmosphere_blob,
    };
    use bevy::{
        app::SubApp,
        asset::Assets,
        core_pipeline::core_3d::{Opaque3d, Transparent3d},
        ecs::{schedule::Schedule, system::RunSystemOnce},
        prelude::{App, Shader},
        render::{
            ExtractSchedule, Render, RenderApp, RenderStartup,
            render_phase::DrawFunctions,
            renderer::{RenderDevice, RenderQueue, WgpuWrapper},
        },
    };
    use sha2::{Digest, Sha256};

    use super::{AtmosphereGpu, AtmosphereRenderInstalled, prepare_atmosphere_textures};
    use crate::cloud_render::{CloudGpu, prepare_cloud_records};
    use crate::{AtmosphereTextureAssets, ChunkRenderPlugin};

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

    fn synthetic_runtime(seed: u8) -> Arc<RuntimeAtmosphereAssets> {
        let textures = [
            (AtmosphereRole::Sun, "textures/environment/sun.png", 32, 32),
            (
                AtmosphereRole::MoonPhases,
                "textures/environment/moon_phases.png",
                128,
                64,
            ),
            (
                AtmosphereRole::Clouds,
                "textures/environment/clouds.png",
                256,
                256,
            ),
        ]
        .into_iter()
        .enumerate()
        .map(|(index, (role, source_path, width, height))| {
            let rgba8 = vec![seed.wrapping_add(index as u8); (width * height * 4) as usize];
            AtmosphereTexture {
                role,
                source_path: source_path.into(),
                source_bytes: 1,
                source_sha256: [index as u8 + 1; 32],
                pixels_sha256: Sha256::digest(&rgba8).into(),
                width,
                height,
                rgba8: rgba8.into_boxed_slice(),
            }
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
        let blob = encode_atmosphere_blob(&CompiledAtmosphereAssets {
            source_manifest_sha256: [0x66; 32],
            textures,
            biome_profiles: Box::new([]),
            fog_profiles: Box::new([]),
        })
        .unwrap();
        Arc::new(RuntimeAtmosphereAssets::decode(&blob).unwrap())
    }

    #[test]
    fn standalone_chunk_renderer_installs_and_starts_atmosphere_gpu_resources() {
        let mut app = app_with_noop_render_sub_app();
        app.add_plugins(ChunkRenderPlugin::new(1));
        app.finish();

        let render_app = app.sub_app_mut(RenderApp);
        assert!(
            render_app
                .world()
                .contains_resource::<AtmosphereRenderInstalled>()
        );
        render_app.world_mut().run_schedule(RenderStartup);
        assert!(render_app.world().contains_resource::<AtmosphereGpu>());
    }

    #[test]
    fn gpu_preparation_uploads_once_per_stable_asset_identity() {
        let mut app = app_with_noop_render_sub_app();
        app.add_plugins(ChunkRenderPlugin::new(1));
        app.finish();

        let render_app = app.sub_app_mut(RenderApp);
        render_app.world_mut().run_schedule(RenderStartup);
        let identity = [0x31; 32];
        render_app
            .world_mut()
            .insert_resource(AtmosphereTextureAssets::new(
                synthetic_runtime(0x10),
                identity,
            ));
        render_app
            .world_mut()
            .run_system_once(prepare_atmosphere_textures)
            .unwrap();
        assert_eq!(
            render_app.world().resource::<AtmosphereGpu>().upload_count,
            1
        );

        render_app
            .world_mut()
            .insert_resource(AtmosphereTextureAssets::new(
                synthetic_runtime(0x20),
                identity,
            ));
        render_app
            .world_mut()
            .run_system_once(prepare_atmosphere_textures)
            .unwrap();
        assert_eq!(
            render_app.world().resource::<AtmosphereGpu>().upload_count,
            1
        );

        render_app
            .world_mut()
            .insert_resource(AtmosphereTextureAssets::new(
                synthetic_runtime(0x20),
                [0x32; 32],
            ));
        render_app
            .world_mut()
            .run_system_once(prepare_atmosphere_textures)
            .unwrap();
        assert_eq!(
            render_app.world().resource::<AtmosphereGpu>().upload_count,
            2
        );
    }

    #[test]
    fn cloud_record_preparation_reuses_equal_identity_and_rebuilds_replacement_once() {
        let mut app = app_with_noop_render_sub_app();
        app.add_plugins(ChunkRenderPlugin::new(1));
        app.finish();

        let render_app = app.sub_app_mut(RenderApp);
        render_app.world_mut().run_schedule(RenderStartup);
        let identity = [0x41; 32];
        render_app
            .world_mut()
            .insert_resource(AtmosphereTextureAssets::new(
                synthetic_runtime(0xfd),
                identity,
            ));
        render_app
            .world_mut()
            .run_system_once(prepare_cloud_records)
            .unwrap();
        let gpu = render_app.world().resource::<CloudGpu>();
        assert_eq!(gpu.record_count, 2);
        assert_eq!(gpu.upload_count, 1);
        let first_buffer = gpu.record_buffer.as_ref().unwrap().id();

        render_app
            .world_mut()
            .insert_resource(AtmosphereTextureAssets::new(
                synthetic_runtime(0xfc),
                identity,
            ));
        render_app
            .world_mut()
            .run_system_once(prepare_cloud_records)
            .unwrap();
        let gpu = render_app.world().resource::<CloudGpu>();
        assert_eq!(gpu.upload_count, 1);
        assert_eq!(gpu.record_buffer.as_ref().unwrap().id(), first_buffer);

        render_app
            .world_mut()
            .insert_resource(AtmosphereTextureAssets::new(
                synthetic_runtime(0xfd),
                [0x42; 32],
            ));
        render_app
            .world_mut()
            .run_system_once(prepare_cloud_records)
            .unwrap();
        let gpu = render_app.world().resource::<CloudGpu>();
        assert_eq!(gpu.upload_count, 2);
        assert_ne!(gpu.record_buffer.as_ref().unwrap().id(), first_buffer);
    }
}
