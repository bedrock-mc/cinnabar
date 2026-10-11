use bytemuck::Zeroable;
use std::collections::HashMap;

use assets::{AtmosphereRole, AtmosphereTexture};
use bevy::image::BevyDefault;
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    mesh::Mesh,
    prelude::{
        App, BevyError, Commands, Entity, FromWorld, Handle, IntoScheduleConfigs, Local, Msaa,
        Plugin, Query, Res, ResMut, Resource, Result, Shader, World, default,
    },
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
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferBindingType, BufferId, BufferInitDescriptor,
            BufferUsages, CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites,
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

use crate::{
    AtmosphereFrame, AtmosphereTextureAssets, PrecipitationScene, RainSplashQueue,
    WeatherTextureAssets, cloud_render::install_cloud_render,
    lightning_render::install_lightning_render, weather_render::install_weather_render,
};

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
    app.init_resource::<crate::SkyboxSettings>();
    app.init_resource::<crate::AtmosphereViewInputs>();
    app.init_resource::<AtmosphereTextureAssets>();
    app.init_resource::<PrecipitationScene>();
    app.init_resource::<WeatherTextureAssets>();
    app.init_resource::<RainSplashQueue>();
    app.init_resource::<crate::PrecipitationMix>();
    app.init_resource::<crate::LightningScene>();
    app.init_resource::<crate::MediaScreenScene>();
    crate::pipeline_warmup::register::<AtmospherePipeline>(app);
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
        ExtractResourcePlugin::<crate::SkyboxSettings>::default(),
        ExtractResourcePlugin::<crate::AtmosphereViewInputs>::default(),
        ExtractResourcePlugin::<AtmosphereTextureAssets>::default(),
        ExtractResourcePlugin::<PrecipitationScene>::default(),
        ExtractResourcePlugin::<WeatherTextureAssets>::default(),
        ExtractResourcePlugin::<crate::LightningScene>::default(),
        ExtractResourcePlugin::<crate::MediaScreenScene>::default(),
    ));
    load_internal_asset!(
        app,
        ATMOSPHERE_SHADER_HANDLE,
        "atmosphere.wgsl",
        crate::shader_safety::from_wgsl
    );
    crate::lighting::install(app);
    install_cloud_render(app);
    install_weather_render(app);
    install_lightning_render(app);
    crate::media_screen::install_media_screen_render(app);

    crate::install_opaque_phase_reset(app.sub_app_mut(RenderApp));
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
                queue_atmosphere
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct AtmosphereGpu {
    pub(crate) buffer: Buffer,
    skybox: Buffer,
    skybox_active: bool,
    stars: Buffer,
    star_vertex_count: u32,
    prepared: Option<PreparedAtmosphereAssets>,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
    bound_asset_identity: Option<[u8; 32]>,
    additional_pipelines: HashMap<AtmospherePipelineKey, CachedRenderPipelineId>,
    #[cfg(test)]
    upload_count: u32,
}

struct PreparedAtmosphereAssets {
    identity: [u8; 32],
    end_sky_identity: Option<[u8; 32]>,
    dimension: assets::SkyboxDimension,
    _skybox_textures: [Texture; assets::SKYBOX_FACE_COUNT],
    skybox_views: [TextureView; assets::SKYBOX_FACE_COUNT],
    skybox_sampler: Sampler,
    _textures: [Texture; 3],
    views: [TextureView; 3],
    sampler: Sampler,
}

fn init_atmosphere_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    AtmosphereFrame::assert_uniform_compat();
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global atmosphere frame"),
        contents: bytemuck::bytes_of(&AtmosphereFrame::default()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let skybox = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("pack skybox transform"),
        contents: bytemuck::bytes_of(&crate::pack_skybox::SkyboxUniform::zeroed()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let vertices = crate::stars::vertices();
    let stars = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("seeded classic star quads"),
        contents: bytemuck::cast_slice(&vertices),
        usage: BufferUsages::STORAGE,
    });
    commands.insert_resource(AtmosphereGpu {
        buffer,
        skybox,
        skybox_active: false,
        stars,
        star_vertex_count: vertices.len() as u32,
        prepared: None,
        bind_group: None,
        view_buffer_id: None,
        bound_asset_identity: None,
        additional_pipelines: HashMap::new(),
        #[cfg(test)]
        upload_count: 0,
    });
}

fn prepare_atmosphere_uniform(
    frame: Res<AtmosphereFrame>,
    settings: Res<crate::SkyboxSettings>,
    assets: Res<AtmosphereTextureAssets>,
    mut gpu: ResMut<AtmosphereGpu>,
    render_queue: Res<RenderQueue>,
) {
    render_queue.write_buffer(&gpu.buffer, 0, bytemuck::bytes_of(&*frame));
    let mask = assets
        .skybox(crate::pack_skybox::dimension(*frame))
        .map_or(0, assets::PackSkybox::face_mask);
    gpu.skybox_active = mask != 0;
    let skybox = crate::pack_skybox::SkyboxUniform::new(*frame, *settings, mask);
    render_queue.write_buffer(&gpu.skybox, 0, bytemuck::bytes_of(&skybox));
}

pub(crate) mod textures;
use textures::prepare_atmosphere_textures;

struct AtmospherePipelineSpecializer;

#[derive(Resource)]
struct AtmospherePipeline {
    variants: Variants<RenderPipeline, AtmospherePipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for AtmospherePipeline {
    fn from_world(_world: &mut World) -> Self {
        let mut entries = vec![
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(ViewUniform::min_size()),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX_FRAGMENT,
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
            BindGroupLayoutEntry {
                binding: 5,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 6,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ];
        entries.push(BindGroupLayoutEntry {
            binding: 7,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: Some(crate::pack_skybox::SkyboxUniform::min_size()),
            },
            count: None,
        });
        for binding in 8..14 {
            entries.push(BindGroupLayoutEntry {
                binding,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            });
        }
        entries.push(BindGroupLayoutEntry {
            binding: 14,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Sampler(SamplerBindingType::Filtering),
            count: None,
        });
        let bind_group_layout =
            BindGroupLayoutDescriptor::new("texture-backed atmosphere bind group layout", &entries);
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
                    blend: Some(BlendState::ALPHA_BLENDING),
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
    stars: bool,
    skybox: bool,
}

// Target Stars material uses OneMinusDestColor→One and disables alpha writes.
// The fragment already multiplies RGB by vertex alpha, so ordinary source-alpha
// blending would incorrectly square that alpha and darken the star quads.
fn native_star_blend() -> BlendState {
    BlendState {
        color: BlendComponent {
            src_factor: BlendFactor::OneMinusDst,
            dst_factor: BlendFactor::One,
            operation: BlendOperation::Add,
        },
        alpha: BlendComponent::REPLACE,
    }
}

impl Specializer<RenderPipeline> for AtmospherePipelineSpecializer {
    type Key = AtmospherePipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        let target = descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap();
        target.format = if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        target.blend = if key.stars {
            Some(native_star_blend())
        } else if key.skybox {
            Some(BlendState::ALPHA_BLENDING)
        } else {
            None
        };
        target.write_mask = ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE;
        if key.skybox {
            descriptor.depth_stencil.as_mut().unwrap().depth_compare = CompareFunction::Greater;
        }
        descriptor.fragment.as_mut().unwrap().entry_point = Some(
            if key.skybox {
                "pack_skybox_fragment"
            } else {
                "atmosphere_fragment"
            }
            .into(),
        );
        Ok(key)
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for AtmospherePipeline {
    /// The sky and its star layer, before the menu's panorama hands over to the world.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        for (stars, skybox) in [(false, false), (true, false), (false, true)] {
            ids.push(self.variants.specialize(
                cache,
                AtmospherePipelineKey {
                    msaa: view.msaa,
                    hdr: view.hdr,
                    stars,
                    skybox,
                },
            )?);
        }
        Ok(())
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
    let mut entries = vec![
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
        BindGroupEntry {
            binding: 5,
            resource: BindingResource::TextureView(&prepared.views[2]),
        },
        BindGroupEntry {
            binding: 6,
            resource: gpu.stars.as_entire_binding(),
        },
    ];
    entries.push(BindGroupEntry {
        binding: 7,
        resource: gpu.skybox.as_entire_binding(),
    });
    for (face, view) in prepared.skybox_views.iter().enumerate() {
        entries.push(BindGroupEntry {
            binding: 8 + face as u32,
            resource: BindingResource::TextureView(view),
        });
    }
    entries.push(BindGroupEntry {
        binding: 14,
        resource: BindingResource::Sampler(&prepared.skybox_sampler),
    });
    gpu.bind_group = Some(render_device.create_bind_group(
        "texture-backed atmosphere bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &entries,
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
    mut gpu: ResMut<AtmosphereGpu>,
    mut next_tick: Local<Tick>,
) {
    let draw_function = draw_functions.read().id::<DrawAtmosphereCommands>();
    // Only current views contribute keys; the finite MSAA/HDR combinations do
    // not accumulate across view recreation or graphics setting changes.
    gpu.additional_pipelines.clear();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            AtmospherePipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                stars: false,
                skybox: false,
            },
        ) else {
            continue;
        };
        for (stars, skybox) in [(true, false), (false, true)] {
            let star_key = AtmospherePipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                stars,
                skybox,
            };
            if let Ok(star_pipeline) = pipeline.variants.specialize(&pipeline_cache, star_key) {
                gpu.additional_pipelines.insert(star_key, star_pipeline);
            }
        }
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

type DrawAtmosphereCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuSky as usize },
    (SetItemPipeline, SetAtmosphereBindGroup<0>, DrawAtmosphere),
>;

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
    type Param = (SRes<AtmosphereGpu>, SRes<PipelineCache>);
    type ViewQuery = (Read<Msaa>, Read<ExtractedView>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        (msaa, view): ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        (gpu, pipelines): SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.draw(0..3, 0..1);
        let gpu = gpu.into_inner();
        let pipelines = pipelines.into_inner();
        for (stars, skybox) in [(true, false), (false, true)] {
            if skybox && !gpu.skybox_active {
                continue;
            }
            let key = AtmospherePipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                stars,
                skybox,
            };
            if let Some(pipeline) = gpu
                .additional_pipelines
                .get(&key)
                .and_then(|id| pipelines.get_render_pipeline(*id))
            {
                pass.set_render_pipeline(pipeline);
                if stars {
                    pass.draw(3..3 + gpu.star_vertex_count, 0..1);
                } else {
                    pass.draw(0..3, 0..1);
                }
            }
        }
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "atmosphere_pipeline_tests.rs"]
mod pipeline_tests;
