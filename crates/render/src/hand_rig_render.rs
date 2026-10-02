//! Near-camera first-person pass that draws the local player's own animated rig (arms + hands)
//! over the scene, reusing the actor rig's packed buffers with a hand-local view and lighting.
//! The rendered content is the player's own skin on the standard samples player geometry.
use crate::{ActorGpuInstance, ActorRigGeometrySpan, ActorRigRenderFrame, ActorRigVertex};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, graph::Core3d},
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{RenderGraph, RenderLabel, ViewNodeRunner},
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        view::{ExtractedView, ViewTarget},
    },
};
use std::{mem::size_of, sync::Arc};

mod node;
#[cfg(test)]
mod tests;

const HAND_RIG_SHADER: Handle<Shader> = uuid_handle!("6f2b1c74-4a2e-49d8-9c1a-2f7b0d5e3a61");
/// Near plane of vanilla's first-person projection.
const HAND_RIG_NEAR_PLANE: f32 = 0.025;

/// Instance texture-selector bits shared with the hand shader.
pub const HAND_ITEM_LAYER_FLAG: u32 = 0x8000_0000;
pub const HAND_OFFHAND_LAYER_FLAG: u32 = 0x4000_0000;

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct HandRigLabel;

#[derive(Debug, Clone, Copy, Default)]
pub struct HandRigRenderPlugin;

impl Plugin for HandRigRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
    fn finish(&self, app: &mut App) {
        install(app);
    }
}

/// Block/sky are raw 0..=15 levels at the player; daylight scales the sky channel. 16 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct HandRigLight {
    pub block_level: u32,
    pub sky_level: u32,
    pub daylight: f32,
    pub pad: u32,
}

const _: () = assert!(size_of::<HandRigLight>() == 16);

/// The equipment atlas page an item instance samples (layer chosen by the instance's
/// `texture_layer` with its top bit set).
#[derive(Clone, Debug)]
pub struct HandItemAtlas {
    pub width: u16,
    pub height: u16,
    pub layers: u32,
    pub rgba8: Arc<[u8]>,
}

/// One frame's local first-person arms and held items in camera space, with their
/// artwork, lighting, and base FOV.
#[derive(Clone, Debug)]
pub(crate) struct HandRigFrame {
    pub(crate) rig: ActorRigRenderFrame,
    pub(crate) skin: Arc<[u8]>,
    pub(crate) light: HandRigLight,
    pub(crate) fov_radians: f32,
    pub(crate) revision: u64,
    pub(crate) item_atlases: [Option<HandItemAtlas>; 2],
}

/// Published by the app each frame the first-person hand should draw; empty otherwise.
#[derive(Clone, Default, Debug, Resource, ExtractResource)]
pub struct HandRigScene {
    pub(crate) frame: Option<HandRigFrame>,
}

impl HandRigScene {
    pub fn clear(&mut self) {
        self.frame = None;
    }

    /// Accepts a single-instance rig frame with a 64x64 RGBA skin and a finite positive FOV;
    /// anything else clears the scene so the fallback keeps rendering.
    pub fn publish(
        &mut self,
        rig: ActorRigRenderFrame,
        skin: Arc<[u8]>,
        light: HandRigLight,
        fov_radians: f32,
        revision: u64,
    ) -> bool {
        if rig.instances.is_empty()
            || rig.previous_bones.is_empty()
            || rig.previous_bones.len() != rig.current_bones.len()
            || rig.maximum_vertex_count == 0
            || skin.len() != crate::STANDARD_SKIN_BYTES
            || !(fov_radians > 0.0 && fov_radians < std::f32::consts::PI)
            || revision == 0
        {
            self.clear();
            return false;
        }
        self.frame = Some(HandRigFrame {
            rig,
            skin,
            light,
            fov_radians,
            revision,
            item_atlases: [None, None],
        });
        true
    }

    /// Supplies the atlas page for item instances of the published frame; ignored when inactive
    /// or when the page is malformed.
    pub fn set_item_atlases(&mut self, atlases: [Option<HandItemAtlas>; 2]) {
        let valid = |atlas: &HandItemAtlas| {
            atlas.width != 0
                && atlas.height != 0
                && atlas.layers != 0
                && usize::from(atlas.width)
                    .checked_mul(usize::from(atlas.height))
                    .and_then(|pixels| pixels.checked_mul(atlas.layers as usize))
                    .and_then(|pixels| pixels.checked_mul(4))
                    == Some(atlas.rgba8.len())
        };
        if let Some(frame) = &mut self.frame {
            frame.item_atlases = atlases.map(|atlas| atlas.filter(valid));
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        self.frame.is_some()
    }
}

fn install(app: &mut App) {
    app.init_resource::<HandRigScene>();
    crate::lighting::install(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        install_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<HandRigScene>::default());
    load_internal_asset!(app, HAND_RIG_SHADER, "hand_rig.wgsl", Shader::from_wgsl);
    let render_app = app.sub_app_mut(RenderApp);
    render_app
        .insert_resource(Installed)
        .add_systems(RenderStartup, init_gpu)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources));
    install_graph(render_app.world_mut());
}

/// The rig pass Enhanced views run after Bloom and grading.
pub(crate) fn enhanced_post_node(world: &mut World) -> impl bevy::render::render_graph::Node {
    ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, true>(node::HandRigViewNode),
        world,
    )
}

fn install_graph(world: &mut World) {
    if !world.contains_resource::<Installed>() {
        return;
    }
    let runner = ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, false>(node::HandRigViewNode),
        world,
    );
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    if graph
        .get_node_state(crate::ui_render::UiOverlayLabel)
        .is_err()
    {
        return;
    }
    if graph.get_node_state(HandRigLabel).is_err() {
        graph.add_node(HandRigLabel, runner);
    }
    graph.add_node_edges((
        crate::ui_render::UiWorldLabel,
        HandRigLabel,
        crate::ui_render::UiOverlayLabel,
    ));
}

#[derive(Resource)]
struct Installed;

struct HandRigDepth {
    _texture: Texture,
    view: TextureView,
    size: [u32; 2],
    samples: u32,
}

struct HandRigAtlas {
    _texture: Texture,
    view: TextureView,
    pixels: Arc<[u8]>,
    size: [u32; 3],
}

struct HandRigSkin {
    _texture: Texture,
    view: TextureView,
    pixels: Arc<[u8]>,
}

#[derive(Resource)]
struct HandRigGpu {
    layout: BindGroupLayoutDescriptor,
    sampler: Sampler,
    view_uniform: Buffer,
    material: Buffer,
    light_uniform: Buffer,
    instances: Option<Buffer>,
    vertices: crate::actor::gpu::SegmentedVertexBuffer,
    spans: Option<Buffer>,
    previous_bones: Option<Buffer>,
    current_bones: Option<Buffer>,
    skin: Option<HandRigSkin>,
    atlases: [Option<HandRigAtlas>; 2],
    instance_count: u32,
    depth: Option<HandRigDepth>,
    bind_group: Option<BindGroup>,
    pipeline: Option<CachedRenderPipelineId>,
    pipeline_variants: [Option<CachedRenderPipelineId>; 8],
    geometry_revision: Option<u64>,
    revision: Option<u64>,
    maximum_vertex_count: u32,
}

fn init_gpu(mut commands: Commands, device: Res<RenderDevice>) {
    let uniform = |label: &'static str, contents: &[u8]| {
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some(label),
            contents,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        })
    };
    // The material class matches the standard player skin (alpha < 0.1 discards).
    let material = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("first-person rig material class"),
        contents: bytemuck::cast_slice(&[
            0,
            HAND_ITEM_LAYER_FLAG,
            HAND_OFFHAND_LAYER_FLAG,
            !(HAND_ITEM_LAYER_FLAG | HAND_OFFHAND_LAYER_FLAG),
        ]),
        usage: BufferUsages::UNIFORM,
    });
    commands.insert_resource(HandRigGpu {
        layout: hand_rig_layout(),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("first-person rig binary-alpha nearest"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        }),
        view_uniform: uniform("first-person rig view", &[0u8; 64]),
        material,
        light_uniform: uniform("first-person rig light", &[0u8; 16]),
        instances: None,
        vertices: default(),
        spans: None,
        previous_bones: None,
        current_bones: None,
        skin: None,
        atlases: [None, None],
        instance_count: 0,
        depth: None,
        bind_group: None,
        pipeline: None,
        pipeline_variants: [None; 8],
        geometry_revision: None,
        revision: None,
        maximum_vertex_count: 0,
    });
}

fn prepare(
    scene: Res<HandRigScene>,
    background: Option<Res<crate::panorama::PanoramaScene>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut gpu: ResMut<HandRigGpu>,
    views: Query<(&ExtractedView, &Msaa)>,
) {
    if background.is_some_and(|background| !background.game_visible()) {
        deactivate(&mut gpu);
        return;
    }
    let Some(frame) = &scene.frame else {
        deactivate(&mut gpu);
        return;
    };
    // The near-camera pass targets the widest 3d view (the main camera).
    let Some((viewport, samples, hdr)) = views
        .iter()
        .map(|(view, msaa)| (view.viewport, msaa.samples(), view.hdr))
        .filter(|(viewport, _, _)| viewport.z != 0 && viewport.w != 0)
        .max_by_key(|(viewport, _, _)| u64::from(viewport.z) * u64::from(viewport.w))
    else {
        deactivate(&mut gpu);
        return;
    };
    let size = [viewport.z, viewport.w];
    upload_geometry(&mut gpu, &device, &queue, frame);
    upload_pose(&mut gpu, &device, &queue, frame);
    upload_skin(&mut gpu, &device, &queue, frame);
    upload_atlas(&mut gpu, &device, &queue, frame);
    ensure_depth(&mut gpu, &device, size, samples);
    let aspect = viewport.z as f32 / viewport.w as f32;
    let projection =
        Mat4::perspective_infinite_reverse_rh(frame.fov_radians, aspect, HAND_RIG_NEAR_PLANE);
    queue.write_buffer(
        &gpu.view_uniform,
        0,
        bytemuck::cast_slice(&projection.to_cols_array()),
    );
    queue.write_buffer(&gpu.light_uniform, 0, bytemuck::bytes_of(&frame.light));
    build_bind_group(&mut gpu, &device, &cache);
    let gpu = &mut *gpu;
    let layout = gpu.layout.clone();
    gpu.pipeline = memoized_pipeline(&mut gpu.pipeline_variants, samples, hdr, || {
        cache.queue_render_pipeline(specialized_pipeline(layout.clone(), samples, hdr))
    });
    if gpu.bind_group.is_none() || gpu.pipeline.is_none() {
        gpu.maximum_vertex_count = 0;
    }
}

fn deactivate(gpu: &mut HandRigGpu) {
    gpu.bind_group = None;
    gpu.maximum_vertex_count = 0;
    gpu.instance_count = 0;
    gpu.revision = None;
}

fn upload_geometry(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    frame: &HandRigFrame,
) {
    if gpu.geometry_revision == Some(frame.rig.geometry_revision)
        && gpu.vertices.buffer().is_some()
        && gpu.spans.is_some()
    {
        return;
    }
    gpu.vertices.sync(
        device,
        queue,
        "first-person rig vertices",
        &frame.rig.geometry_vertices,
    );
    gpu.spans = Some(storage(
        device,
        "first-person rig spans",
        &frame.rig.geometry_spans,
    ));
    gpu.geometry_revision = Some(frame.rig.geometry_revision);
    gpu.bind_group = None;
}

fn upload_pose(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    frame: &HandRigFrame,
) {
    if gpu.revision == Some(frame.revision) && gpu.instances.is_some() {
        return;
    }
    // The pose changes every frame; same-sized buffers are rewritten so the bind group survives.
    let mut recreated = false;
    for (slot, label, bytes) in [
        (
            &mut gpu.instances,
            "first-person rig instance",
            bytemuck::cast_slice::<_, u8>(&frame.rig.instances),
        ),
        (
            &mut gpu.previous_bones,
            "first-person rig previous bones",
            bytemuck::cast_slice::<_, u8>(&frame.rig.previous_bones),
        ),
        (
            &mut gpu.current_bones,
            "first-person rig current bones",
            bytemuck::cast_slice::<_, u8>(&frame.rig.current_bones),
        ),
    ] {
        match slot {
            Some(buffer) if buffer.size() == bytes.len() as u64 => {
                queue.write_buffer(buffer, 0, bytes);
            }
            _ => {
                *slot = Some(device.create_buffer_with_data(&BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                }));
                recreated = true;
            }
        }
    }
    gpu.maximum_vertex_count = frame.rig.maximum_vertex_count;
    gpu.instance_count = u32::try_from(frame.rig.instances.len()).unwrap_or(0);
    gpu.revision = Some(frame.revision);
    if recreated {
        gpu.bind_group = None;
    }
}

fn upload_skin(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    frame: &HandRigFrame,
) {
    if gpu
        .skin
        .as_ref()
        .is_some_and(|skin| Arc::ptr_eq(&skin.pixels, &frame.skin) || skin.pixels == frame.skin)
    {
        return;
    }
    let side = crate::STANDARD_SKIN_SIDE as u32;
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("first-person rig skin"),
            size: Extent3d {
                width: side,
                height: side,
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
        &frame.skin,
    );
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some("first-person rig skin layer"),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
    gpu.skin = Some(HandRigSkin {
        _texture: texture,
        view,
        pixels: Arc::clone(&frame.skin),
    });
    gpu.bind_group = None;
}

fn upload_atlas(
    gpu: &mut HandRigGpu,
    device: &RenderDevice,
    queue: &RenderQueue,
    frame: &HandRigFrame,
) {
    for (slot, atlas) in gpu.atlases.iter_mut().zip(&frame.item_atlases) {
        let Some(atlas) = atlas else {
            if slot.take().is_some() {
                gpu.bind_group = None;
            }
            continue;
        };
        if slot.as_ref().is_some_and(|current| {
            Arc::ptr_eq(&current.pixels, &atlas.rgba8)
                && current.size
                    == [
                        u32::from(atlas.width),
                        u32::from(atlas.height),
                        atlas.layers,
                    ]
        }) {
            continue;
        }
        let texture = device.create_texture_with_data(
            queue,
            &TextureDescriptor {
                label: Some("first-person item atlas"),
                size: Extent3d {
                    width: u32::from(atlas.width),
                    height: u32::from(atlas.height),
                    depth_or_array_layers: atlas.layers,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &atlas.rgba8,
        );
        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("first-person item atlas layers"),
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
        *slot = Some(HandRigAtlas {
            _texture: texture,
            view,
            pixels: Arc::clone(&atlas.rgba8),
            size: [
                u32::from(atlas.width),
                u32::from(atlas.height),
                atlas.layers,
            ],
        });
        gpu.bind_group = None;
    }
}

fn ensure_depth(gpu: &mut HandRigGpu, device: &RenderDevice, size: [u32; 2], samples: u32) {
    if gpu
        .depth
        .as_ref()
        .is_some_and(|depth| depth.size == size && depth.samples == samples)
    {
        return;
    }
    gpu.depth = None;
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("first-person rig private reverse-Z depth"),
        size: Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: samples,
        dimension: TextureDimension::D2,
        format: CORE_3D_DEPTH_FORMAT,
        usage: TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor::default());
    gpu.depth = Some(HandRigDepth {
        _texture: texture,
        view,
        size,
        samples,
    });
}

fn build_bind_group(gpu: &mut HandRigGpu, device: &RenderDevice, cache: &PipelineCache) {
    if gpu.bind_group.is_some() {
        return;
    }
    let (Some(instances), Some(vertices), Some(spans), Some(previous), Some(current), Some(skin)) = (
        gpu.instances.as_ref(),
        gpu.vertices.buffer(),
        gpu.spans.as_ref(),
        gpu.previous_bones.as_ref(),
        gpu.current_bones.as_ref(),
        gpu.skin.as_ref(),
    ) else {
        return;
    };
    gpu.bind_group = Some(
        device.create_bind_group(
            "first-person rig bind group",
            &cache.get_bind_group_layout(&gpu.layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: gpu.view_uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: instances.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: vertices.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: spans.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: previous.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: current.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::TextureView(&skin.view),
                },
                BindGroupEntry {
                    binding: 7,
                    resource: BindingResource::Sampler(&gpu.sampler),
                },
                BindGroupEntry {
                    binding: 8,
                    resource: gpu.material.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 9,
                    resource: gpu.light_uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 10,
                    // Without an item atlas the skin view stands in; no instance selects it.
                    resource: BindingResource::TextureView(
                        gpu.atlases[0]
                            .as_ref()
                            .map_or(&skin.view, |atlas| &atlas.view),
                    ),
                },
                BindGroupEntry {
                    binding: 11,
                    resource: BindingResource::TextureView(
                        gpu.atlases[1]
                            .as_ref()
                            .map_or(&skin.view, |atlas| &atlas.view),
                    ),
                },
            ],
        ),
    );
}

fn storage<T: bytemuck::Pod>(device: &RenderDevice, label: &'static str, data: &[T]) -> Buffer {
    device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::cast_slice(data),
        usage: BufferUsages::STORAGE,
    })
}

fn memoized_pipeline<T: Copy>(
    entries: &mut [Option<T>; 8],
    samples: u32,
    hdr: bool,
    create: impl FnOnce() -> T,
) -> Option<T> {
    let sample = match samples {
        1 => 0,
        2 => 1,
        4 => 2,
        8 => 3,
        _ => return None,
    };
    let entry = &mut entries[sample + usize::from(hdr) * 4];
    Some(*entry.get_or_insert_with(create))
}

fn specialized_pipeline(
    layout: BindGroupLayoutDescriptor,
    samples: u32,
    hdr: bool,
) -> RenderPipelineDescriptor {
    let mut descriptor = pipeline_descriptor(layout);
    descriptor.multisample.count = samples;
    descriptor.fragment.as_mut().unwrap().targets[0]
        .as_mut()
        .unwrap()
        .format = if hdr {
        ViewTarget::TEXTURE_FORMAT_HDR
    } else {
        TextureFormat::bevy_default()
    };
    descriptor
}

fn pipeline_descriptor(layout: BindGroupLayoutDescriptor) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("first-person animated rig".into()),
        layout: vec![layout, crate::lighting::layout()],
        vertex: VertexState {
            shader: HAND_RIG_SHADER,
            entry_point: Some("hand_vertex".into()),
            buffers: vec![],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: HAND_RIG_SHADER,
            entry_point: Some("hand_fragment".into()),
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

fn hand_rig_layout() -> BindGroupLayoutDescriptor {
    let storage_entry = |binding: u32, min: u64| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(min),
        },
        count: None,
    };
    BindGroupLayoutDescriptor::new(
        "first-person rig bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(64),
                },
                count: None,
            },
            storage_entry(1, size_of::<ActorGpuInstance>() as u64),
            storage_entry(2, size_of::<ActorRigVertex>() as u64),
            storage_entry(3, size_of::<ActorRigGeometrySpan>() as u64),
            storage_entry(4, size_of::<[[f32; 4]; 3]>() as u64),
            storage_entry(5, size_of::<[[f32; 4]; 3]>() as u64),
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
            BindGroupLayoutEntry {
                binding: 9,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 10,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 11,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
        ],
    )
}
