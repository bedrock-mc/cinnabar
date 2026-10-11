use bevy::render::render_resource::ShaderType;
use bytemuck::Zeroable;
use std::collections::HashMap;

use assets::{AtmosphereRole, AtmosphereTexture};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
    prelude::{
        App, BevyError, Commands, FromWorld, Handle, IntoScheduleConfigs, Msaa, Plugin, Query, Res,
        ResMut, Resource, Result, Shader, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferBindingType, BufferId, BufferInitDescriptor,
            BufferUsages, CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites,
            CompareFunction, DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache,
            RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, Specializer, SpecializerKey, Texture,
            TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        view::{ViewUniform, ViewUniforms},
    },
};

use {
    crate::{
        cloud_render::install_cloud_render, lightning_render::install_lightning_render,
        weather_render::install_weather_render,
    },
    render_scene::{
        AtmosphereFrame, AtmosphereTextureAssets, PrecipitationMixResource, PrecipitationScene,
        RainSplashQueue, WeatherTextureAssets,
    },
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
    app.init_resource::<render_scene::SkyboxSettings>();
    app.init_resource::<render_scene::AtmosphereViewInputs>();
    app.init_resource::<AtmosphereTextureAssets>();
    app.init_resource::<PrecipitationScene>();
    app.init_resource::<WeatherTextureAssets>();
    app.init_resource::<RainSplashQueue>();
    app.init_resource::<PrecipitationMixResource>();
    app.init_resource::<render_scene::LightningScene>();
    app.init_resource::<render_scene::MediaScreenScene>();
    crate::pipeline_warmup::register::<AtmospherePipeline>(app);
    let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<AtmosphereRenderInstalled>()
    {
        sky_pass::install(render_app.world_mut());
        return;
    }

    app.add_plugins((
        ExtractResourcePlugin::<AtmosphereFrame>::default(),
        ExtractResourcePlugin::<render_scene::SkyboxSettings>::default(),
        ExtractResourcePlugin::<render_scene::AtmosphereViewInputs>::default(),
        ExtractResourcePlugin::<AtmosphereTextureAssets>::default(),
        ExtractResourcePlugin::<PrecipitationScene>::default(),
        ExtractResourcePlugin::<WeatherTextureAssets>::default(),
        ExtractResourcePlugin::<render_scene::LightningScene>::default(),
        ExtractResourcePlugin::<render_scene::MediaScreenScene>::default(),
    ));
    load_internal_asset!(
        app,
        ATMOSPHERE_SHADER_HANDLE,
        "atmosphere.wesl",
        crate::shader_safety::from_wesl
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
        .add_systems(RenderStartup, init_atmosphere_gpu)
        .add_systems(
            Render,
            (
                prepare_atmosphere_uniform.in_set(RenderSystems::PrepareResources),
                prepare_atmosphere_textures.in_set(RenderSystems::PrepareResources),
                prepare_atmosphere_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_atmosphere
                    .run_if(render_scene::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
    sky_pass::install(app.sub_app_mut(RenderApp).world_mut());
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
    pipelines: HashMap<AtmospherePipelineKey, CachedRenderPipelineId>,
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
        pipelines: HashMap::new(),
        #[cfg(test)]
        upload_count: 0,
    });
}

fn prepare_atmosphere_uniform(
    frame: Res<AtmosphereFrame>,
    settings: Res<render_scene::SkyboxSettings>,
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
                    format: render_scene::SCENE_COLOR_FORMAT,
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
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
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
            descriptor.depth_stencil.as_mut().unwrap().depth_compare =
                Some(CompareFunction::Greater);
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

/// Prepares only the current views' sky pipelines for the ordered sky pass.
fn queue_atmosphere(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<AtmospherePipeline>,
    views: Query<(&bevy::render::camera::ExtractedCamera, &Msaa)>,
    mut gpu: ResMut<AtmosphereGpu>,
) {
    gpu.pipelines.clear();
    for (view, msaa) in &views {
        for (stars, skybox) in [(false, false), (true, false), (false, true)] {
            let key = AtmospherePipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
                stars,
                skybox,
            };
            if let Ok(id) = pipeline.variants.specialize(&pipeline_cache, key) {
                gpu.pipelines.insert(key, id);
            }
        }
    }
}

pub(crate) mod sky_pass;

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "atmosphere_render/sky_pass_tests.rs"]
mod sky_pass_tests;

#[cfg(test)]
#[path = "atmosphere_pipeline_tests.rs"]
mod pipeline_tests;
