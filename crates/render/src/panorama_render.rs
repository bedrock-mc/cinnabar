//! Draws the menu panorama as one full-screen triangle that ray-casts into the
//! six-face cube array, so the view is an exact perspective cube.
use crate::panorama::PanoramaScene;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
            BufferBindingType, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Extent3d,
            FilterMode, FragmentState, PipelineCache, RenderPipeline, RenderPipelineDescriptor,
            Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, Specializer,
            SpecializerKey, Texture, TextureDataOrder, TextureDescriptor, TextureDimension,
            TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget},
    },
};

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

fn install(app: &mut App) {
    app.init_resource::<PanoramaScene>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<PanoramaScene>::default());
    load_internal_asset!(
        app,
        PANORAMA_SHADER_HANDLE,
        "panorama.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .insert_resource(Installed)
        .init_resource::<PanoramaPipeline>()
        .add_render_command::<Transparent3d, DrawPanoramaCommands>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare_panorama.in_set(RenderSystems::PrepareResources),
                prepare_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_panorama.in_set(RenderSystems::Queue),
            ),
        );
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
            mipmap_filter: FilterMode::Nearest,
            ..default()
        }),
        _texture: texture,
        texture_view,
        faces_revision: None,
        visible: false,
        bind_group: None,
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
            Some(faces) => texture_array(&device, &queue, faces.side(), faces.layer_major()),
            None => texture_array(&device, &queue, 1, &[0; 24]),
        };
        gpu._texture = texture;
        gpu.texture_view = view;
        gpu.faces_revision = Some(scene.faces_revision);
        gpu.bind_group = None;
    }
    let (Some(view), true) = (scene.view, scene.faces.is_some()) else {
        gpu.visible = false;
        return;
    };
    gpu.visible = true;
    queue.write_buffer(
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
                depth_compare: CompareFunction::Always,
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
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        Ok(key)
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

fn queue_panorama(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<PanoramaPipeline>,
    scene: Res<PanoramaScene>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
) {
    if scene.view.is_none() || scene.faces.is_none() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawPanoramaCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            PanoramaPipelineKey {
                msaa: *msaa,
                hdr: view.hdr,
            },
        ) else {
            continue;
        };
        phase.add(Transparent3d {
            entity: (view_entity, *main_entity),
            pipeline: pipeline_id,
            draw_function,
            // Sorts before every other transparent item: it is the backdrop.
            distance: f32::MIN,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
    }
}

type DrawPanoramaCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuPanorama as usize },
    (SetItemPipeline, SetPanoramaBindGroup, DrawPanorama),
>;

struct SetPanoramaBindGroup;

impl<P: PhaseItem> RenderCommand<P> for SetPanoramaBindGroup {
    type Param = SRes<PanoramaGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = &gpu.into_inner().bind_group else {
            return RenderCommandResult::Skip;
        };
        pass.set_bind_group(0, bind_group, &[]);
        RenderCommandResult::Success
    }
}

struct DrawPanorama;

impl<P: PhaseItem> RenderCommand<P> for DrawPanorama {
    type Param = SRes<PanoramaGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        if !gpu.into_inner().visible {
            return RenderCommandResult::Skip;
        }
        pass.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
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
    #[test]
    fn review_render_panorama_queue_uses_current_visibility() {
        let (mut app, view) = fixture::app();
        app.init_resource::<PanoramaScene>()
            .init_resource::<PanoramaPipeline>()
            .add_render_command::<Transparent3d, DrawPanoramaCommands>();
        app.world_mut()
            .resource_mut::<PanoramaScene>()
            .set_faces(Some(std::sync::Arc::new(
                render_model::PanoramaFaces::new(1, std::array::from_fn(|_| vec![255; 4])).unwrap(),
            )));
        app.world_mut().run_system_once(init_gpu).unwrap();
        app.world_mut()
            .resource_mut::<PanoramaScene>()
            .show(Some(render_model::PanoramaView {
                yaw_radians: 0.0,
                pitch_radians: 0.0,
                vertical_fov_radians: 1.0,
                aspect: 1.0,
                tint: [0.0; 4],
            }));
        app.world_mut().run_system_once(queue_panorama).unwrap();
        assert_eq!(fixture::items(&app, view).len(), 1);
        fixture::clear(&mut app, view);
        app.world_mut().resource_mut::<PanoramaGpu>().visible = true;
        app.world_mut().resource_mut::<PanoramaScene>().show(None);
        app.world_mut().run_system_once(queue_panorama).unwrap();
        assert!(fixture::items(&app, view).is_empty());
    }
}
