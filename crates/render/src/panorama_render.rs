//! Draws the menu panorama as one full-screen triangle that ray-casts into the
//! six-face cube array, so the view is an exact perspective cube.
//!
//! The opaque backdrop draws after world geometry and before screen UI, so startup
//! can keep gathering GPU visibility evidence without exposing unfinished terrain.
use crate::queue_writes::ProfiledQueue as _;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{Core3d, Core3dSystems, core_3d::CORE_3D_DEPTH_FORMAT},
    prelude::{
        App, BevyError, Commands, Entity, FromWorld, Handle, IntoScheduleConfigs, Msaa, Plugin,
        Query, Res, ResMut, Resource, Result, Shader, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        camera::ExtractedCamera,
        extract_resource::ExtractResourcePlugin,
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferInitDescriptor, BufferSize, BufferUsages, CachedRenderPipelineId, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Extent3d,
            FilterMode, FragmentState, LoadOp, PipelineCache, RenderPassDescriptor, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            Specializer, SpecializerKey, StoreOp, Texture, TextureDataOrder, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue, ViewQuery},
        view::{ViewDepthStencilTexture, ViewTarget},
    },
};
use render_scene::PanoramaScene;
use std::collections::BTreeMap;

const PANORAMA_SHADER_HANDLE: Handle<Shader> = uuid_handle!("7b1e9c42-5d3a-4f60-9e8b-2a4c6d1f0e93");
const UNIFORM_BYTES: usize = std::mem::size_of::<PanoramaUniform>();

/// Mirrors `Panorama` in `panorama.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
struct PanoramaUniform {
    /// yaw, pitch, tan(half vertical fov), aspect
    view: [f32; 4],
    tint: [f32; 4],
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PanoramaRenderPlugin;

impl Plugin for PanoramaRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }

    fn finish(&self, app: &mut App) {
        install(app);
    }
}

#[derive(Resource)]
struct Installed;

/// Installs GPU resources and the backdrop pass once the camera schedule is available.
fn install(app: &mut App) {
    app.init_resource::<PanoramaScene>();
    crate::pipeline_warmup::register::<PanoramaPipeline>(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        install_pass(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<PanoramaScene>::default());
    load_internal_asset!(
        app,
        PANORAMA_SHADER_HANDLE,
        "../../render-model/src/panorama.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(Installed)
        .init_resource::<PanoramaPipeline>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare_panorama.in_set(RenderSystems::PrepareResources),
                prepare_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_panorama.in_set(RenderSystems::Queue),
            ),
        );
    install_pass(app.sub_app_mut(RenderApp).world_mut());
}

#[derive(Resource)]
struct PassInstalled;

/// Covers terrain after its submission, retaining the same scene samples for post-processing.
fn install_pass(world: &mut World) {
    if world.contains_resource::<PassInstalled>() {
        return;
    }
    if world
        .try_schedule_scope(Core3d, |_, schedule| {
            schedule.add_systems(
                crate::gpu_timing::profiled(
                    draw_panorama,
                    Some(render_scene::RuntimeStage::GpuPanorama),
                    "PanoramaPass",
                )
                .after(bevy::core_pipeline::core_3d::main_transparent_pass_3d)
                .after(crate::scene_target::ScenePass::Transparent)
                .after(crate::ui_render::UiWorldLabel)
                .before(crate::scene_target::ScenePass::Finish)
                .in_set(Core3dSystems::MainPass),
            );
        })
        .is_ok()
    {
        world.insert_resource(PassInstalled);
    }
}

#[derive(Resource)]
struct PanoramaGpu {
    uniform: Buffer,
    sampler: Sampler,
    _texture: Texture,
    texture_view: TextureView,
    faces_revision: Option<u64>,
    visible: bool,
    bind_group: Option<BindGroup>,
    pipelines: BTreeMap<Entity, CachedRenderPipelineId>,
}

fn texture_array(
    device: &RenderDevice,
    queue: &RenderQueue,
    side: u32,
    pixels: &[u8],
) -> (Texture, TextureView) {
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("menu panorama faces"),
            size: Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        pixels,
    );
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some("menu panorama face layers"),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    (texture, view)
}

fn init_gpu(mut commands: Commands, device: Res<RenderDevice>, queue: Res<RenderQueue>) {
    let (texture, texture_view) = texture_array(&device, &queue, 1, &[0; 24]);
    commands.insert_resource(PanoramaGpu {
        uniform: device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("menu panorama view"),
            contents: bytemuck::bytes_of(&PanoramaUniform::default()),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        }),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("menu panorama sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..default()
        }),
        _texture: texture,
        texture_view,
        faces_revision: None,
        visible: false,
        bind_group: None,
        pipelines: BTreeMap::new(),
    });
}

fn prepare_panorama(
    scene: Res<PanoramaScene>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<PanoramaGpu>,
) {
    if gpu.faces_revision != Some(scene.faces_revision) {
        let (texture, view) = match &scene.faces {
            Some(faces) => {
                let uploaded = texture_array(&device, &queue, faces.side(), faces.layer_major());
                scene.mark_uploaded(scene.faces_revision);
                uploaded
            }
            None => texture_array(&device, &queue, 1, &[0; 24]),
        };
        gpu._texture = texture;
        gpu.texture_view = view;
        gpu.faces_revision = Some(scene.faces_revision);
        gpu.bind_group = None;
    }
    let Some(view) = scene.view.filter(|_| scene.has_faces()).or_else(|| {
        // Missing faces still need an opaque cover while startup renders underneath.
        (!scene.game_visible())
            .then(|| render_model::panorama::launcher_view(0.0, 1.0, [0.0, 0.0, 0.0, 1.0]))
    }) else {
        gpu.visible = false;
        return;
    };
    gpu.visible = true;
    queue.profiled_write_buffer(
        &gpu.uniform,
        0,
        bytemuck::cast_slice(&view.shader_uniform()),
    );
}

struct PanoramaPipelineSpecializer;

#[derive(Resource)]
struct PanoramaPipeline {
    variants: Variants<RenderPipeline, PanoramaPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for PanoramaPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "menu panorama bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(UNIFORM_BYTES as u64),
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
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("menu panorama pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: PANORAMA_SHADER_HANDLE,
                entry_point: Some("panorama_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: PANORAMA_SHADER_HANDLE,
                entry_point: Some("panorama_fragment".into()),
                // Every fragment writes alpha 1, so blending would only cost bandwidth.
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: None,
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::Always),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(PanoramaPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct PanoramaPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for PanoramaPipelineSpecializer {
    type Key = PanoramaPipelineKey;

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
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        Ok(key)
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for PanoramaPipeline {
    /// The menu backdrop, so leaving a world never compiles it on first sight.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.push(self.variants.specialize(
            cache,
            PanoramaPipelineKey {
                msaa: view.msaa,
                hdr: view.hdr,
            },
        )?);
        Ok(())
    }
}

fn prepare_bind_group(
    device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<PanoramaPipeline>,
    mut gpu: ResMut<PanoramaGpu>,
) {
    if gpu.bind_group.is_some() {
        return;
    }
    gpu.bind_group = Some(device.create_bind_group(
        "menu panorama bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: gpu.uniform.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(&gpu.texture_view),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
        ],
    ));
}

/// Prepares each active view's backdrop pipeline, forgetting views that left the scene.
fn queue_panorama(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<PanoramaPipeline>,
    scene: Res<PanoramaScene>,
    mut gpu: ResMut<PanoramaGpu>,
    views: Query<(Entity, &ExtractedCamera, &Msaa)>,
) {
    gpu.pipelines.retain(|entity, _| views.contains(*entity));
    if scene.game_visible() {
        gpu.pipelines.clear();
        return;
    }
    for (entity, camera, msaa) in &views {
        let key = PanoramaPipelineKey {
            msaa: *msaa,
            hdr: camera.hdr,
        };
        match pipeline.variants.specialize(&pipeline_cache, key) {
            Ok(id) => {
                gpu.pipelines.insert(entity, id);
            }
            Err(_) => {
                gpu.pipelines.remove(&entity);
            }
        }
    }
}

type PanoramaQuery = (
    &'static ExtractedCamera,
    &'static ViewTarget,
    Option<&'static crate::scene_target::SceneTarget>,
    &'static ViewDepthStencilTexture,
    Option<&'static MainPassResolutionOverride>,
);

/// Draws over completed world geometry while leaving the later screen UI untouched.
fn draw_panorama(world: &World, query: ViewQuery<PanoramaQuery>, mut context: RenderContext) {
    let entity = query.entity();
    let (camera, target, scene, depth, resolution) = query.into_inner();
    let Some(gpu) = world
        .get_resource::<PanoramaGpu>()
        .filter(|gpu| gpu.visible)
    else {
        return;
    };
    let Some(bind_group) = gpu.bind_group.as_ref() else {
        return;
    };
    let cache = world.resource::<PipelineCache>();
    let Some(pipeline) = gpu
        .pipelines
        .get(&entity)
        .and_then(|id| cache.get_render_pipeline(*id))
    else {
        return;
    };
    let color = scene.map_or_else(
        || target.get_color_attachment(),
        |scene| {
            scene.resume();
            scene.color_attachment(target, false)
        },
    );
    let viewport = Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution).map(
        |viewport| {
            crate::render_bounds::viewport(&viewport, crate::render_bounds::extent(color.view))
        },
    );
    // Covered terrain must not supply depth to the panorama's post-processing.
    let mut depth = depth.get_attachment(StoreOp::Store);
    if let Some(ops) = &mut depth.depth_ops {
        ops.load = LoadOp::Clear(0.0);
    }
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("panorama over world geometry"),
        color_attachments: &[Some(color)],
        depth_stencil_attachment: Some(depth),
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuPanorama,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    match viewport {
        Some(Some(viewport)) => pass.set_camera_viewport(&viewport),
        Some(None) => return,
        None => {}
    }
    pass.set_render_pipeline(pipeline);
    pass.set_bind_group(0, bind_group, &[]);
    pass.draw(0..3, 0..1);
}

#[cfg(test)]
mod tests {
    use super::UNIFORM_BYTES;

    #[test]
    fn uniform_matches_the_wgsl_layout() {
        assert_eq!(UNIFORM_BYTES, 32);
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::queue_review_support as fixture;
    use bevy::ecs::system::RunSystemOnce;

    /// Hiding a panorama drops its prepared views in the same frame.
    #[test]
    fn panorama_preparation_uses_current_visibility() {
        let (mut app, _) = fixture::app();
        app.init_resource::<PanoramaScene>()
            .init_resource::<PanoramaPipeline>();
        app.world_mut().run_system_once(init_gpu).unwrap();
        let mut scene = app.world_mut().resource_mut::<PanoramaScene>();
        scene.set_faces(Some(std::sync::Arc::new(
            render_model::PanoramaFaces::new(1, std::array::from_fn(|_| vec![255; 4])).unwrap(),
        )));
        scene.show(Some(render_model::panorama::launcher_view(
            0.0, 1.0, [0.0; 4],
        )));
        app.world_mut().run_system_once(queue_panorama).unwrap();
        assert_eq!(app.world().resource::<PanoramaGpu>().pipelines.len(), 1);
        assert!(!app.world().resource::<PanoramaScene>().game_visible());
        app.world_mut().resource_mut::<PanoramaScene>().show(None);
        app.world_mut().run_system_once(queue_panorama).unwrap();
        assert!(app.world().resource::<PanoramaGpu>().pipelines.is_empty());
    }

    #[test]
    fn panorama_draws_without_blending() {
        let (mut app, _) = fixture::app();
        let mut pipeline = PanoramaPipeline::from_world(app.world_mut());
        let mut cache = app.world_mut().resource_mut::<PipelineCache>();
        let id = pipeline
            .variants
            .specialize(
                &cache,
                PanoramaPipelineKey {
                    msaa: Msaa::Off,
                    hdr: false,
                },
            )
            .unwrap();
        let descriptor = fixture::queued_descriptor(&mut cache, id);
        let target = descriptor.fragment.as_ref().unwrap().targets[0].as_ref();
        assert_eq!(target.unwrap().blend, None);
    }
}

#[cfg(test)]
mod latency_tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn latency_cadence_menu_preparation_can_qualify_for_one() {
        let (device, queue) = crate::gpu_timing::tests::noop_device(wgpu::Features::empty());
        let mut world = World::new();
        world.insert_resource(device);
        world.insert_resource(queue);
        let mut scene = PanoramaScene::default();
        scene.set_game_visible(false);
        world.insert_resource(scene);
        world.run_system_once(init_gpu).unwrap();
        crate::present_mode::frame_latency::adaptive::tests::assert_preparation_qualifies(|| {
            world.run_system_once(prepare_panorama).unwrap();
            assert!(world.resource::<PanoramaGpu>().visible);
        });
    }
}
