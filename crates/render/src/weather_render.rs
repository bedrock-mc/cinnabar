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
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
            BufferBindingType, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, FilterMode,
            FragmentState, PipelineCache, RenderPipeline, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, Specializer,
            SpecializerKey, Texture, TextureSampleType, TextureView, TextureViewDimension,
            Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use crate::{
    AtmosphereFrame,
    atmosphere_render::{AtmosphereGpu, upload_rgba},
    weather::{
        MAX_PRECIPITATION_LAYERS, OCCLUSION_SIDE, PrecipitationLayerRecord, PrecipitationScene,
        WeatherTextureAssets, particle_mesh,
    },
};

const WEATHER_SHADER_HANDLE: Handle<Shader> = uuid_handle!("5b0f2f6e-6c1d-4a58-9f0a-3f1d7a9e2c11");
const LAYER_BYTES: usize = std::mem::size_of::<PrecipitationLayerRecord>();
const PARAMS_BYTES: usize = std::mem::size_of::<WeatherParamsGpu>();
const OCCLUSION_BYTES: usize = 2 * (OCCLUSION_SIDE * OCCLUSION_SIDE) as usize * 4;
/// Fixed seed so the particle mesh is identical across runs.
const PARTICLE_MESH_SEED: u64 = 0x5745_4154_4845_5231;

/// Uniform read by `weather.wgsl`: box forward offset plus sheet flag, then the grid origin.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct WeatherParamsGpu {
    forward: [f32; 4],
    grid: [i32; 4],
}

pub(crate) fn install_weather_render(app: &mut App) {
    crate::pipeline_warmup::register::<WeatherPipeline>(app);
    load_internal_asset!(
        app,
        WEATHER_SHADER_HANDLE,
        "weather.wgsl",
        crate::shader_safety::from_wgsl
    );
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .init_resource::<WeatherPipeline>()
        .add_render_command::<Transparent3d, DrawWeatherCommands>()
        .add_systems(RenderStartup, init_weather_gpu)
        .add_systems(
            Render,
            (
                prepare_weather_records.in_set(RenderSystems::PrepareResources),
                prepare_weather_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_weather
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct WeatherGpu {
    record_buffer: Buffer,
    params_buffer: Buffer,
    particle_buffer: Buffer,
    occlusion_buffer: Buffer,
    occlusion_generation: Option<u64>,
    layer_count: u32,
    max_particles: u32,
    _sheet: Texture,
    sheet_view: TextureView,
    sheet_identity: Option<[u8; 32]>,
    sampler: Sampler,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
    atmosphere_buffer_id: Option<BufferId>,
}

fn init_weather_gpu(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
) {
    let record_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("precipitation layer records"),
        contents: &vec![0_u8; MAX_PRECIPITATION_LAYERS * LAYER_BYTES],
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
    });
    let particle_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("precipitation particle mesh"),
        contents: bytemuck::cast_slice(&particle_mesh(PARTICLE_MESH_SEED)),
        usage: BufferUsages::STORAGE,
    });
    let occlusion_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("precipitation occlusion grid"),
        contents: &vec![0_u8; OCCLUSION_BYTES],
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
    });
    let params_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("precipitation parameters"),
        contents: bytemuck::bytes_of(&WeatherParamsGpu::default()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let (sheet, sheet_view) = upload_rgba(
        &render_device,
        &render_queue,
        1,
        1,
        &[255; 4],
        "absent precipitation sheet fallback",
    );
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("precipitation sheet clamp sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(WeatherGpu {
        record_buffer,
        params_buffer,
        particle_buffer,
        occlusion_buffer,
        occlusion_generation: None,
        layer_count: 0,
        max_particles: 0,
        _sheet: sheet,
        sheet_view,
        sheet_identity: None,
        sampler,
        bind_group: None,
        view_buffer_id: None,
        atmosphere_buffer_id: None,
    });
}

pub(crate) fn prepare_weather_records(
    scene: Res<PrecipitationScene>,
    textures: Option<Res<WeatherTextureAssets>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<WeatherGpu>,
) {
    let sheet = textures.as_deref().and_then(|assets| {
        assets
            .textures()
            .map(|textures| (assets.identity(), textures))
    });
    if gpu.sheet_identity != sheet.map(|(identity, _)| identity) {
        gpu.sheet_identity = sheet.map(|(identity, _)| identity);
        let (texture, view) = match sheet {
            Some((_, textures)) => upload_rgba(
                &render_device,
                &render_queue,
                textures.weather.width,
                textures.weather.height,
                &textures.weather.rgba8,
                "vanilla precipitation sheet",
            ),
            None => upload_rgba(
                &render_device,
                &render_queue,
                1,
                1,
                &[255; 4],
                "absent precipitation sheet fallback",
            ),
        };
        gpu._sheet = texture;
        gpu.sheet_view = view;
        gpu.bind_group = None;
    }
    let has_sheet = f32::from(u8::from(sheet.is_some()));
    let count = scene.layers.len().min(MAX_PRECIPITATION_LAYERS);
    gpu.layer_count = u32::try_from(count).expect("bounded precipitation layer count");
    gpu.max_particles = scene.layers[..count]
        .iter()
        .map(|layer| layer.particle_count)
        .max()
        .unwrap_or(0);
    if count == 0 {
        return;
    }
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "weather.frame_upload",
        layers = count,
        occlusion_generation = scene.occlusion_generation,
        upload_occlusion = gpu.occlusion_generation != Some(scene.occlusion_generation),
    )
    .entered();
    render_queue.write_buffer(
        &gpu.record_buffer,
        0,
        bytemuck::cast_slice::<PrecipitationLayerRecord, u8>(&scene.layers[..count]),
    );
    if gpu.occlusion_generation != Some(scene.occlusion_generation) {
        gpu.occlusion_generation = Some(scene.occlusion_generation);
        render_queue.write_buffer(
            &gpu.occlusion_buffer,
            0,
            bytemuck::cast_slice::<i32, u8>(&scene.occlusion.heights),
        );
    }
    let [x, y, z] = scene.forward_offset;
    let [origin_x, origin_z] = scene.occlusion.origin;
    let params = WeatherParamsGpu {
        forward: [x, y, z, has_sheet],
        grid: [origin_x, origin_z, 0, 0],
    };
    render_queue.write_buffer(&gpu.params_buffer, 0, bytemuck::bytes_of(&params));
}

struct WeatherPipelineSpecializer;

#[derive(Resource)]
struct WeatherPipeline {
    variants: Variants<RenderPipeline, WeatherPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for WeatherPipeline {
    fn from_world(_world: &mut World) -> Self {
        let uniform = |binding: u32, size: BufferSize, dynamic: bool| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: dynamic,
                min_binding_size: Some(size),
            },
            count: None,
        };
        let storage = |binding: u32, visibility: ShaderStages, size: usize| BindGroupLayoutEntry {
            binding,
            visibility,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: BufferSize::new(size as u64),
            },
            count: None,
        };
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "precipitation bind group layout",
            &[
                uniform(0, ViewUniform::min_size(), true),
                uniform(1, AtmosphereFrame::min_size(), false),
                storage(2, ShaderStages::VERTEX, LAYER_BYTES),
                uniform(
                    3,
                    BufferSize::new(PARAMS_BYTES as u64).expect("non-zero params size"),
                    false,
                ),
                BindGroupLayoutEntry {
                    binding: 4,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 5,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                storage(6, ShaderStages::VERTEX, 16),
                storage(7, ShaderStages::FRAGMENT, 4),
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("precipitation pipeline".into()),
            layout: vec![bind_group_layout.clone(), crate::lighting::layout()],
            vertex: VertexState {
                shader: WEATHER_SHADER_HANDLE,
                entry_point: Some("weather_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: WEATHER_SHADER_HANDLE,
                entry_point: Some("weather_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: crate::SCENE_COLOR_FORMAT,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
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
            variants: Variants::new(WeatherPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct WeatherPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for WeatherPipelineSpecializer {
    type Key = WeatherPipelineKey;

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
            crate::SCENE_HDR_FORMAT
        } else {
            crate::SCENE_COLOR_FORMAT
        };
        Ok(key)
    }
}

fn prepare_weather_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<WeatherPipeline>,
    view_uniforms: Res<ViewUniforms>,
    atmosphere: Res<AtmosphereGpu>,
    mut gpu: ResMut<WeatherGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.bind_group.is_some()
        && gpu.view_buffer_id == Some(view_buffer.id())
        && gpu.atmosphere_buffer_id == Some(atmosphere.buffer.id())
    {
        return;
    }
    gpu.bind_group = Some(render_device.create_bind_group(
        "precipitation bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: atmosphere.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: gpu.record_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: gpu.params_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::TextureView(&gpu.sheet_view),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
            BindGroupEntry {
                binding: 6,
                resource: gpu.particle_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 7,
                resource: gpu.occlusion_buffer.as_entire_binding(),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
    gpu.atmosphere_buffer_id = Some(atmosphere.buffer.id());
}

fn queue_weather(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<WeatherPipeline>,
    gpu: Res<WeatherGpu>,
    scene: Res<PrecipitationScene>,
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
    if gpu.layer_count == 0 || gpu.max_particles == 0 || scene.layers.is_empty() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawWeatherCommands>();
    for (view_entity, main_entity, view, extracted_camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            WeatherPipelineKey {
                msaa: *msaa,
                hdr: extracted_camera.hdr,
            },
        ) else {
            continue;
        };
        crate::transparent_phase::add(
            phase,
            Transparent3d {
                sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                entity: (view_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function,
                distance: view
                    .rangefinder3d()
                    .distance(&view.world_from_view.translation()),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            },
        );
    }
}

type DrawWeatherCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    SetWeatherBindGroup<0>,
    DrawWeather,
);

struct SetWeatherBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetWeatherBindGroup<I> {
    type Param = SRes<WeatherGpu>;
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

struct DrawWeather;

impl<P: PhaseItem> RenderCommand<P> for DrawWeather {
    type Param = SRes<WeatherGpu>;
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
        pass.draw(0..gpu.max_particles.saturating_mul(6), 0..gpu.layer_count);
        RenderCommandResult::Success
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for WeatherPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.push(self.variants.specialize(
            cache,
            WeatherPipelineKey {
                msaa: view.msaa,
                hdr: view.hdr,
            },
        )?);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{LAYER_BYTES, OCCLUSION_BYTES, PARAMS_BYTES};

    #[test]
    fn gpu_records_match_the_wgsl_layouts() {
        assert_eq!(LAYER_BYTES, 80);
        assert_eq!(PARAMS_BYTES, 32);
        assert_eq!(OCCLUSION_BYTES, 2 * 64 * 64 * 4);
    }
}
