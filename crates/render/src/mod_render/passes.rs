//! Mod post passes: each enabled pass reads the previous scene colour and writes the next,
//! after post-processing and before the HUD, timed per pass slot.

use super::ModRenderScene;
use crate::RuntimeStage;
use bevy::tasks::{AsyncComputeTaskPool, Task};
use bevy::{
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::query::QueryItem,
    prelude::*,
    render::{
        Render, RenderStartup, RenderSystems,
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferSize, BufferUsages, ColorTargetState, ColorWrites, Extent3d,
            FilterMode, LoadOp, Operations, PipelineCache, RenderPassColorAttachment,
            RenderPassDescriptor, RenderPipeline, Sampler, SamplerBindingType, SamplerDescriptor,
            ShaderStages, StoreOp, Texture, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDimension, TextureViewId,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use mod_render::shader::{FRAGMENT_ENTRY, FRAME_UNIFORM_BYTES, VERTEX_ENTRY};
use std::{collections::HashMap, sync::Mutex};

#[derive(Debug, Clone, Hash, Eq, PartialEq, RenderLabel)]
pub struct ModPassLabel;

/// Mirrors `ModFrame` in the sandbox prelude.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct FrameUniform {
    pub(crate) clip_from_world: [[f32; 4]; 4],
    pub(crate) world_from_clip: [[f32; 4]; 4],
    pub(crate) eye: [f32; 4],
    pub(crate) resolution: [f32; 4],
    pub(crate) time: [f32; 4],
    pub(crate) params: [f32; mod_api::MAX_PASS_PARAMS],
}

#[derive(Resource)]
pub(crate) struct PassGpu {
    /// Indexed by whether the pass reads depth.
    layouts: [BindGroupLayoutDescriptor; 2],
    sampler: Sampler,
    _dummy_depth: Texture,
    dummy_depth_view: TextureView,
    /// Owned rather than in Bevy's append-only cache, so dropping an entry frees it.
    pub(crate) pipelines: HashMap<(u64, TextureFormat), PipelineState>,
    uniforms: HashMap<(Entity, u64), Buffer>,
    /// The scene alternates between two main textures, so each pass reuses two bind groups.
    bind_groups: Mutex<HashMap<BindGroupKey, BindGroup>>,
}

pub(crate) enum PipelineState {
    Creating(Task<Option<RenderPipeline>>),
    Ready(RenderPipeline),
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct BindGroupKey {
    view: Entity,
    revision: u64,
    source: TextureViewId,
    depth: Option<TextureViewId>,
}

impl PassGpu {
    /// Drops pipelines whose pass revision is no longer in the scene.
    pub(crate) fn retain_pipelines(&mut self, scene: &ModRenderScene) {
        let current = |revision: u64| scene.passes.iter().any(|p| p.revision == revision);
        self.pipelines.retain(|(revision, _), _| current(*revision));
    }

    /// Starts compiling `pass` for `format` once, and adopts finished compiles.
    pub(crate) fn ensure_pipeline(
        &mut self,
        device: &RenderDevice,
        cache: &PipelineCache,
        pass: &mod_render::Pass,
        format: TextureFormat,
    ) {
        let state = self
            .pipelines
            .entry((pass.revision, format))
            .or_insert_with(|| {
                let layout = cache.get_bind_group_layout(&self.layouts[usize::from(pass.depth)]);
                compile(device.clone(), layout, pass.shader.clone(), format)
            });
        if let PipelineState::Creating(task) = state
            && let Some(result) = bevy::tasks::futures::check_ready(task)
        {
            *state = result.map_or(PipelineState::Failed, PipelineState::Ready);
        }
    }

    pub(crate) fn pipeline(&self, revision: u64, format: TextureFormat) -> Option<&RenderPipeline> {
        match self.pipelines.get(&(revision, format)) {
            Some(PipelineState::Ready(pipeline)) => Some(pipeline),
            _ => None,
        }
    }
}

/// Compiles off the render thread where Bevy does, and inline where it compiles inline.
fn compile(
    device: RenderDevice,
    layout: BindGroupLayout,
    shader: std::sync::Arc<str>,
    format: TextureFormat,
) -> PipelineState {
    let task = async move { create_pipeline(&device, &layout, &shader, format) };
    if cfg!(any(target_os = "macos", target_arch = "wasm32")) {
        bevy::tasks::block_on(task).map_or(PipelineState::Failed, PipelineState::Ready)
    } else {
        PipelineState::Creating(AsyncComputeTaskPool::get().spawn(task))
    }
}

/// Builds a checked pipeline; validation errors are logged rather than reaching the device.
pub(crate) fn create_pipeline(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    shader: &str,
    format: TextureFormat,
) -> Option<RenderPipeline> {
    let wgpu_device = device.wgpu_device();
    wgpu_device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mod post pass"),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mod post pass"),
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mod post pass"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(VERTEX_ENTRY),
            compilation_options: default(),
            buffers: &[],
        },
        primitive: default(),
        depth_stencil: None,
        multisample: default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(FRAGMENT_ENTRY),
            compilation_options: default(),
            targets: &[Some(color_target(format))],
        }),
        multiview: None,
        cache: None,
    });
    match bevy::tasks::block_on(wgpu_device.pop_error_scope()) {
        None => Some(pipeline),
        Some(error) => {
            warn!("mod post pass rejected by the device: {error}");
            None
        }
    }
}

/// Each pass replaces the scene colour outright.
pub(crate) fn color_target(format: TextureFormat) -> ColorTargetState {
    ColorTargetState {
        format,
        blend: None,
        write_mask: ColorWrites::ALL,
    }
}

pub(crate) fn layout(depth: bool) -> BindGroupLayoutDescriptor {
    let mut entries = vec![
        BindGroupLayoutEntry {
            binding: 0,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: BufferSize::new(FRAME_UNIFORM_BYTES as u64),
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
    ];
    if depth {
        entries.push(BindGroupLayoutEntry {
            binding: 3,
            visibility: ShaderStages::FRAGMENT,
            ty: BindingType::Texture {
                sample_type: TextureSampleType::Depth,
                view_dimension: TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        });
    }
    BindGroupLayoutDescriptor::new(
        if depth {
            "mod pass layout with depth"
        } else {
            "mod pass layout"
        },
        &entries,
    )
}

pub(super) fn install(render_app: &mut SubApp) {
    render_app
        .add_systems(RenderStartup, init_gpu)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources));
    install_graph(render_app.world_mut());
}

pub(crate) fn init_gpu(mut commands: Commands, device: Res<RenderDevice>) {
    let dummy_depth = device.create_texture(&TextureDescriptor {
        label: Some("mod pass absent depth"),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Depth32Float,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    commands.insert_resource(PassGpu {
        layouts: [layout(false), layout(true)],
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("mod pass scene sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            ..default()
        }),
        dummy_depth_view: dummy_depth.create_view(&default()),
        _dummy_depth: dummy_depth,
        pipelines: HashMap::new(),
        uniforms: HashMap::new(),
        bind_groups: Mutex::new(HashMap::new()),
    });
}

/// Packs one pass's view and parameters into the prelude's uniform layout.
pub(crate) fn frame_uniform(
    view: &ExtractedView,
    seconds: f32,
    delta: f32,
    slot: usize,
    params: [f32; mod_api::MAX_PASS_PARAMS],
) -> FrameUniform {
    let clip_from_world = view
        .clip_from_world
        .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
    let size = view.viewport.zw().max(UVec2::ONE).as_vec2();
    FrameUniform {
        clip_from_world: clip_from_world.to_cols_array_2d(),
        world_from_clip: clip_from_world.inverse().to_cols_array_2d(),
        eye: view.world_from_view.translation().extend(1.0).to_array(),
        resolution: [size.x, size.y, 1.0 / size.x, 1.0 / size.y],
        time: [seconds, delta, slot as f32, 0.0],
        params,
    }
}

fn prepare(
    scene: Option<Res<ModRenderScene>>,
    time: Res<Time>,
    (device, queue, cache): (Res<RenderDevice>, Res<RenderQueue>, Res<PipelineCache>),
    gpu: Option<ResMut<PassGpu>>,
    views: Query<(Entity, &ExtractedView, &ViewTarget)>,
) {
    let (Some(scene), Some(mut gpu)) = (scene, gpu) else {
        return;
    };
    let gpu = &mut *gpu;
    let current = |revision: u64| scene.passes.iter().any(|p| p.revision == revision);
    gpu.retain_pipelines(&scene);
    gpu.uniforms
        .retain(|(view, revision), _| views.contains(*view) && current(*revision));
    gpu.bind_groups
        .get_mut()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|key, _| {
            current(key.revision)
                && views.get(key.view).is_ok_and(|(_, _, target)| {
                    key.source == target.main_texture_view().id()
                        || key.source == target.main_texture_other_view().id()
                })
        });
    for (entity, view, target) in &views {
        let format = target.main_texture_format();
        for (slot, pass) in scene.passes.iter().enumerate() {
            if !pass.enabled {
                continue;
            }
            gpu.ensure_pipeline(&device, &cache, pass, format);
            let uniform = frame_uniform(
                view,
                time.elapsed_secs_wrapped(),
                time.delta_secs(),
                slot,
                pass.params,
            );
            let buffer = gpu
                .uniforms
                .entry((entity, pass.revision))
                .or_insert_with(|| {
                    device.create_buffer(&BufferDescriptor {
                        label: Some("mod pass frame"),
                        size: FRAME_UNIFORM_BYTES as u64,
                        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    })
                });
            queue.write_buffer(buffer, 0, bytemuck::bytes_of(&uniform));
        }
    }
}

pub(crate) fn install_graph(world: &mut World) {
    let runner = ViewNodeRunner::new(ModPassNode, world);
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
    if graph.get_node_state(ModPassLabel).is_err() {
        graph.add_node(ModPassLabel, runner);
    }
    let _ = graph.try_add_node_edge(Node3d::EndMainPassPostProcessing, ModPassLabel);
    for overlay in [
        crate::ui_render::UiOverlayLabel.intern(),
        crate::ui_render::overlay::UiOverlayPostLabel.intern(),
    ] {
        let _ = graph.try_add_node_edge(ModPassLabel, overlay);
    }
}

struct ModPassNode;

impl ViewNode for ModPassNode {
    type ViewQuery = (&'static ViewTarget, Option<&'static ViewDepthTexture>);

    fn run<'w>(
        &self,
        graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (target, depth): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let (Some(scene), Some(gpu), Some(cache)) = (
            world.get_resource::<ModRenderScene>(),
            world.get_resource::<PassGpu>(),
            world.get_resource::<PipelineCache>(),
        ) else {
            return Ok(());
        };
        if scene.passes.is_empty()
            || world
                .get_resource::<crate::PanoramaScene>()
                .is_some_and(|panorama| !panorama.game_visible())
        {
            return Ok(());
        }
        let view = graph.view_entity();
        let format = target.main_texture_format();
        let depth_view = depth
            .filter(|depth| {
                depth
                    .texture
                    .usage()
                    .contains(TextureUsages::TEXTURE_BINDING)
                    && depth.texture.sample_count() == 1
            })
            .map_or(&gpu.dummy_depth_view, ViewDepthTexture::view);
        for (slot, pass) in scene.passes.iter().enumerate() {
            let (true, Some(pipeline), Some(uniform)) = (
                pass.enabled,
                gpu.pipeline(pass.revision, format),
                gpu.uniforms.get(&(view, pass.revision)),
            ) else {
                continue;
            };
            let stage =
                RuntimeStage::GPU_MOD_PASSES[slot.min(RuntimeStage::GPU_MOD_PASSES.len() - 1)];
            crate::gpu_timing::timed(world, context, stage, |context| {
                let post = target.post_process_write();
                let key = BindGroupKey {
                    view,
                    revision: pass.revision,
                    source: post.source.id(),
                    depth: pass.depth.then(|| depth_view.id()),
                };
                let mut bind_groups = gpu
                    .bind_groups
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let bind_group = bind_groups.entry(key).or_insert_with(|| {
                    let colour = [
                        BindGroupEntry {
                            binding: 0,
                            resource: uniform.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: BindingResource::TextureView(post.source),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: BindingResource::Sampler(&gpu.sampler),
                        },
                        BindGroupEntry {
                            binding: 3,
                            resource: BindingResource::TextureView(depth_view),
                        },
                    ];
                    context.render_device().create_bind_group(
                        "mod pass",
                        &cache.get_bind_group_layout(&gpu.layouts[usize::from(pass.depth)]),
                        &colour[..if pass.depth { 4 } else { 3 }],
                    )
                });
                let bind_group = bind_group.clone();
                drop(bind_groups);
                let attachments = [Some(RenderPassColorAttachment {
                    view: post.destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: Operations {
                        load: LoadOp::Clear(default()),
                        store: StoreOp::Store,
                    },
                })];
                let mut render_pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                    label: Some("mod post pass"),
                    color_attachments: &attachments,
                    depth_stencil_attachment: None,
                    timestamp_writes: crate::gpu_timing::render_pass_timestamps(world, stage),
                    occlusion_query_set: None,
                });
                render_pass.set_render_pipeline(pipeline);
                render_pass.set_bind_group(0, &bind_group, &[]);
                render_pass.draw(0..3, 0..1);
            });
        }
        Ok(())
    }
}
