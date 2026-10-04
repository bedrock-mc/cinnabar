use crate::ui_render::DeviceObservation;
use crate::viewmodel::{
    HandVertex, ViewmodelCompletionGate, ViewmodelScene, ViewmodelToken, hand_projection,
    viewmodel_depth_bytes,
};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::graph::Core3d,
    ecs::system::{SystemChangeTick, SystemParam},
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_graph::{RenderGraph, RenderLabel, ViewNodeRunner},
        render_resource::*,
        renderer::{RenderAdapter, RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::ExtractedView,
    },
};
use std::{mem::size_of, sync::Mutex};
mod node;
#[cfg(test)]
mod tests;
const HAND_SHADER: Handle<Shader> = uuid_handle!("05c3d760-7ab6-4f19-b6b3-dea197927fa5");

#[derive(Debug, Clone, Copy, Default)]
pub struct ViewmodelRenderPlugin;
#[derive(Resource)]
struct Installed;
#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct HandLabel;
impl Plugin for ViewmodelRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }
    fn finish(&self, app: &mut App) {
        install(app);
    }
}
fn install(app: &mut App) {
    app.init_resource::<ViewmodelScene>()
        .init_resource::<ViewmodelCompletionGate>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        install_hand_graph(app.sub_app_mut(RenderApp).world_mut());
        return;
    }
    let gate = app.world().resource::<ViewmodelCompletionGate>().clone();
    app.add_plugins(ExtractResourcePlugin::<ViewmodelScene>::default());
    load_internal_asset!(
        app,
        HAND_SHADER,
        "viewmodel.wgsl",
        crate::shader_safety::from_wgsl
    );
    let render_app = app.sub_app_mut(RenderApp);
    render_app
        .insert_resource(Installed)
        .insert_resource(gate)
        .init_resource::<HandDrawn>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare.in_set(RenderSystems::PrepareResources),
                submit_completion
                    .in_set(RenderSystems::Render)
                    .after(bevy::render::renderer::render_system),
            ),
        );
    install_hand_graph(render_app.world_mut());
}

/// The hand pass Enhanced views run after Bloom and grading.
#[cfg(feature = "enhanced")]
pub(crate) fn enhanced_post_node(world: &mut World) -> impl bevy::render::render_graph::Node {
    ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, true>(node::HandViewNode),
        world,
    )
}

pub(crate) fn install_hand_graph(world: &mut World) {
    if !world.contains_resource::<Installed>() {
        return;
    }
    let runner = ViewNodeRunner::new(
        crate::ui_render::overlay::GradeStage::<_, false>(node::HandViewNode),
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
    if graph.get_node_state(HandLabel).is_err() {
        graph.add_node(HandLabel, runner);
    }
    graph.add_node_edges((
        crate::ui_render::UiWorldLabel,
        HandLabel,
        crate::ui_render::UiOverlayLabel,
    ));
}

#[derive(Default, Resource)]
struct HandDrawn(Mutex<Option<ViewmodelToken>>);
struct PixelGpu {
    _texture: Texture,
    view: TextureView,
    identity: [u8; 32],
    geometry: [u8; 32],
    pixels: std::sync::Arc<[u8]>,
}
struct DepthGpu {
    _texture: Texture,
    view: TextureView,
    size: [u32; 2],
    samples: u32,
}
#[derive(Resource)]
struct HandGpu {
    device: wgpu::Device,
    device_observation: DeviceObservation,
    projection: Buffer,
    sampler: Sampler,
    layout: BindGroupLayoutDescriptor,
    vertices: Option<Buffer>,
    geometry: Option<[u8; 32]>,
    skin: Option<PixelGpu>,
    depth: Option<DepthGpu>,
    bind_group: Option<BindGroup>,
    token: Option<ViewmodelToken>,
    pipeline: Option<CachedRenderPipelineId>,
    pipeline_variants: [Option<CachedRenderPipelineId>; 8],
    vertex_count: u32,
}

fn init_gpu(
    mut commands: Commands,
    device: Res<RenderDevice>,
    tick: SystemChangeTick,
    gate: Res<ViewmodelCompletionGate>,
    drawn: Res<HandDrawn>,
    coverage: Option<Res<crate::ui_render::UiHandCoverage>>,
) {
    gate.select(None);
    *drawn.0.lock().expect("hand drawn lock") = None;
    if let Some(coverage) = coverage {
        coverage.clear();
    }
    commands.insert_resource(HandGpu {
        device: device.wgpu_device().clone(),
        device_observation: DeviceObservation::new(tick.this_run()),
        projection: device.create_buffer(&BufferDescriptor {
            label: Some("neutral hand projection"),
            size: 64,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("neutral binary-alpha hand nearest"),
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: FilterMode::Nearest,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            ..default()
        }),
        layout: hand_layout(),
        vertices: None,
        geometry: None,
        skin: None,
        depth: None,
        bind_group: None,
        token: None,
        pipeline: None,
        pipeline_variants: [None; 8],
        vertex_count: 0,
    });
}

#[derive(SystemParam)]
struct PrepareViewmodel<'w, 's> {
    scene: Res<'w, ViewmodelScene>,
    background: Option<Res<'w, crate::panorama::PanoramaScene>>,
    device: Res<'w, RenderDevice>,
    adapter: Res<'w, RenderAdapter>,
    queue: Res<'w, RenderQueue>,
    cache: Res<'w, PipelineCache>,
    gpu: ResMut<'w, HandGpu>,
    gate: Res<'w, ViewmodelCompletionGate>,
    drawn: Res<'w, HandDrawn>,
    views: Query<'w, 's, (&'static MainEntity, &'static ExtractedView, &'static Msaa)>,
    coverage: Option<Res<'w, crate::ui_render::UiHandCoverage>>,
    tick: SystemChangeTick,
}

fn prepare(params: PrepareViewmodel) {
    let PrepareViewmodel {
        scene,
        background,
        device,
        adapter,
        queue,
        cache,
        mut gpu,
        gate,
        drawn,
        views,
        coverage,
        tick,
    } = params;
    let same_device = &gpu.device == device.wgpu_device();
    let device_valid =
        gpu.device_observation
            .observe(device.last_changed(), tick.this_run(), same_device);
    if let Some(coverage) = coverage {
        coverage.clear();
    }
    *drawn.0.lock().expect("hand drawn lock") = None;
    if !device_valid {
        ViewmodelCompletionGate::observe_stage(2, 1, scene.frame.as_ref().map(|frame| frame.token));
        if let Some(frame) = &scene.frame {
            gate.reject(frame.token);
        }
        invalidate_hand_resources(&mut gpu);
        return;
    }
    if background.is_some_and(|background| !background.game_visible()) {
        deactivate_hand(&mut gpu);
        return;
    }
    let Some(frame) = &scene.frame else {
        ViewmodelCompletionGate::observe_stage(2, 2, None);
        deactivate_hand(&mut gpu);
        return;
    };
    let token = frame.token;
    if frame.fallback.is_none() {
        ViewmodelCompletionGate::observe_stage(2, 3, Some(token));
        gate.reject(token);
        gpu.token = None;
        return;
    }
    if gpu.skin.as_ref().is_some_and(|old| {
        old.identity == token.skin
            && !std::sync::Arc::ptr_eq(&old.pixels, &frame.skin.rgba8)
            && old.pixels != frame.skin.rgba8
    }) {
        ViewmodelCompletionGate::observe_stage(2, 4, Some(token));
        gate.reject(token);
        gpu.token = None;
        return;
    }
    let view_valid = views.iter().any(|(owner, view, msaa)| {
        owner.id() == token.owner
            && view.hdr == token.hdr
            && msaa.samples() == token.samples
            && view.viewport == UVec4::new(0, 0, token.viewport[0], token.viewport[1])
    });
    let features = adapter.get_texture_format_features(TextureFormat::Depth32Float);
    if !view_valid
        || viewmodel_depth_bytes(token.viewport, token.samples).is_none()
        || token
            .viewport
            .iter()
            .any(|v| *v > device.limits().max_texture_dimension_2d)
        || !features.flags.sample_count_supported(token.samples)
        || !features
            .allowed_usages
            .contains(TextureUsages::RENDER_ATTACHMENT)
    {
        ViewmodelCompletionGate::observe_stage(2, 5, Some(token));
        gate.reject(token);
        gpu.token = None;
        gpu.depth = None;
        return;
    }
    if gpu.token == Some(token) {
        ViewmodelCompletionGate::observe_stage(2, 0, Some(token));
        return;
    }
    if gpu.geometry != Some(token.geometry) {
        gpu.vertices = None;
        gpu.vertices = Some(device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("validated neutral arm and sleeve"),
            contents: bytemuck::cast_slice(&frame.geometry.vertices),
            usage: BufferUsages::VERTEX,
        }));
        gpu.geometry = Some(token.geometry);
        gpu.vertex_count = frame.geometry.vertices.len() as u32;
    }
    if gpu
        .skin
        .as_ref()
        .is_none_or(|skin| skin.identity != token.skin || skin.geometry != token.geometry)
    {
        gpu.bind_group = None;
        gpu.skin = None;
        let texture = device.create_texture_with_data(
            &queue,
            &TextureDescriptor {
                label: Some("validated neutral hand skin"),
                size: Extent3d {
                    width: crate::viewmodel::VIEWMODEL_TEXTURE_SIDE,
                    height: crate::viewmodel::VIEWMODEL_TEXTURE_SIDE,
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
            &frame.skin.rgba8,
        );
        let view = texture.create_view(&TextureViewDescriptor::default());
        gpu.skin = Some(PixelGpu {
            _texture: texture,
            view,
            identity: token.skin,
            geometry: token.geometry,
            pixels: std::sync::Arc::clone(&frame.skin.rgba8),
        });
    }
    if gpu
        .depth
        .as_ref()
        .is_none_or(|depth| depth.size != token.viewport || depth.samples != token.samples)
    {
        // Drop the old declared allocation before constructing its replacement.
        gpu.depth = None;
        let texture = device.create_texture(&TextureDescriptor {
            label: Some("bounded private hand reverse-Z depth"),
            size: Extent3d {
                width: token.viewport[0],
                height: token.viewport[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: token.samples,
            dimension: TextureDimension::D2,
            format: TextureFormat::Depth32Float,
            usage: TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&TextureViewDescriptor::default());
        gpu.depth = Some(DepthGpu {
            _texture: texture,
            view,
            size: token.viewport,
            samples: token.samples,
        });
    }
    queue.write_buffer(
        &gpu.projection,
        0,
        bytemuck::cast_slice(&hand_projection(token.viewport).to_cols_array()),
    );
    if gpu.bind_group.is_none() {
        gpu.bind_group = Some(device.create_bind_group(
            "neutral hand binding",
            &cache.get_bind_group_layout(&gpu.layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: gpu.projection.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&gpu.skin.as_ref().unwrap().view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&gpu.sampler),
                },
            ],
        ));
    }
    let gpu = &mut *gpu;
    let layout = &gpu.layout;
    gpu.pipeline =
        memoized_hand_pipeline(&mut gpu.pipeline_variants, token.samples, token.hdr, || {
            cache.queue_render_pipeline(specialized_hand_pipeline(
                layout.clone(),
                token.samples,
                token.hdr,
            ))
        });
    if gpu.pipeline.is_none() {
        ViewmodelCompletionGate::observe_stage(2, 6, Some(token));
        gate.reject(token);
        gpu.token = None;
        return;
    }
    gpu.token = Some(token);
    ViewmodelCompletionGate::observe_stage(2, 0, Some(token));
}

fn invalidate_hand_resources(gpu: &mut HandGpu) {
    gpu.token = None;
    gpu.depth = None;
    gpu.bind_group = None;
    gpu.vertices = None;
    gpu.skin = None;
    gpu.geometry = None;
    gpu.pipeline = None;
    gpu.pipeline_variants = [None; 8];
    gpu.vertex_count = 0;
}

fn deactivate_hand(gpu: &mut HandGpu) {
    gpu.token = None;
    gpu.depth = None;
    // Immutable pipeline variants remain bounded and reusable on re-enable.
}

fn memoized_hand_pipeline<T: Copy>(
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

fn specialized_hand_pipeline(
    layout: BindGroupLayoutDescriptor,
    samples: u32,
    hdr: bool,
) -> RenderPipelineDescriptor {
    let mut descriptor = hand_pipeline_descriptor(layout);
    descriptor.multisample.count = samples;
    descriptor.fragment.as_mut().unwrap().targets[0]
        .as_mut()
        .unwrap()
        .format = if hdr {
        bevy::render::view::ViewTarget::TEXTURE_FORMAT_HDR
    } else {
        TextureFormat::bevy_default()
    };
    descriptor
}

fn hand_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "neutral hand layout",
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
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
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
    )
}
fn hand_pipeline_descriptor(layout: BindGroupLayoutDescriptor) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("neutral static empty hand".into()),
        layout: vec![layout],
        vertex: VertexState {
            shader: HAND_SHADER,
            entry_point: Some("hand_vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: size_of::<HandVertex>() as u64,
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
                ],
            }],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: HAND_SHADER,
            entry_point: Some("hand_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: CompareFunction::GreaterEqual,
            stencil: default(),
            bias: default(),
        }),
        ..default()
    }
}

fn submit_completion(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    drawn: Res<HandDrawn>,
    gate: Res<ViewmodelCompletionGate>,
    gpu: Res<HandGpu>,
) {
    let token = drawn.0.lock().expect("hand drawn lock").take();
    if token.is_none()
        && let Some(expected) = gpu.token
        && !gate.rejected(expected)
    {
        // Coverage is per current intended view, never accumulated from earlier
        // frames. A previously completed draw cannot authorize a missing pass.
        // This render-stage rejection restores CPU at the next UI publication;
        // it cannot retroactively restore a UI frame already encoded this frame.
        gate.reject(expected);
    }
    if let Some(reservation) = token.and_then(|token| gate.reserve(token)) {
        let command = device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("hand queue completion sentinel"),
            })
            .finish();
        let callback = gate.clone();
        command.on_submitted_work_done(move || {
            callback.complete(reservation);
        });
        queue.submit([command]);
        ViewmodelCompletionGate::observe_stage(4, 1, token);
    } else {
        ViewmodelCompletionGate::observe_stage(
            4,
            if token.is_some() { 2 } else { 3 },
            token.or(gpu.token),
        );
    }
    if let Err(error) = device.poll(PollType::Poll) {
        ViewmodelCompletionGate::observe_stage(4, 4, token);
        if let Some(token) = token {
            gate.reject(token);
        }
        bevy::log::warn!(?error, "hand completion polling failed");
    }
}
