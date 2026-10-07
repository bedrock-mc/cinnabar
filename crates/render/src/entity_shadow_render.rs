//! Draws [`EntityShadowScene`] after opaque geometry and before translucency: one instanced
//! draw of every shadow volume's back faces darkens the opaque surface inside it by a constant
//! encoded-colour multiplier. Rules: `docs/reference/entity-shadows.md`.
use std::num::NonZeroU64;

use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::graph::{Core3d, Node3d},
    ecs::query::QueryItem,
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        camera::ExtractedCamera,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_graph::{
            NodeRunError, RenderGraph, RenderGraphContext, RenderLabel, ViewNode, ViewNodeRunner,
        },
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingResource, BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, Extent3d, Face,
            FragmentState, LoadOp, Operations, Origin3d, PipelineCache, PrimitiveState,
            RenderPassColorAttachment, RenderPassDescriptor, RenderPipelineDescriptor,
            ShaderStages, ShaderType, StoreOp, Texture, TextureDescriptor, TextureDimension,
            TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDimension,
            TextureViewId, VertexAttribute, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderContext, RenderDevice, RenderQueue},
        view::{
            ExtractedView, ViewDepthTexture, ViewTarget, ViewUniform, ViewUniformOffset,
            ViewUniforms,
        },
    },
};
use render_model::{
    EntityShadow, EntityShadowFrame, EntityShadowParams, SHADOW_VOLUME_VERTICES,
    entity_shadow_colour, shadow_screen_rect, shadow_volume_mesh,
};

use crate::{AtmosphereFrame, SkyKind};

const SHADER: Handle<Shader> = uuid_handle!("6b0e5c1d-3f8a-4e27-9b41-2d7c0a5e8f13");
const INSTANCE_BYTES: u64 = size_of::<EntityShadow>() as u64;

/// Main-world holder of this frame's casters, cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Default, Debug)]
pub struct EntityShadowScene(pub EntityShadowFrame);

#[derive(Debug, Hash, PartialEq, Eq, Clone, RenderLabel)]
pub(crate) struct EntityShadowLabel;

#[derive(Debug, Clone, Copy, Default)]
pub struct EntityShadowRenderPlugin;

impl Plugin for EntityShadowRenderPlugin {
    fn build(&self, app: &mut App) {
        load_internal_asset!(
            app,
            SHADER,
            "entity_shadow.wgsl",
            crate::shader_safety::from_wgsl
        );
        app.init_resource::<EntityShadowScene>()
            .add_plugins(ExtractResourcePlugin::<EntityShadowScene>::default())
            .add_systems(Last, crate::chunk::admit_depth_sampling);
    }

    fn finish(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        let device = render_app.world().resource::<RenderDevice>().clone();
        render_app
            .insert_resource(EntityShadowGpu::new(&device))
            .add_systems(
                Render,
                (
                    prepare_shadow_buffers.in_set(RenderSystems::PrepareResources),
                    prepare_shadow_views
                        .in_set(RenderSystems::PrepareResources)
                        .after(bevy::render::view::prepare_view_targets),
                    prepare_shadow_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                ),
            );
        install_graph(render_app.world_mut());
    }
}

fn install_graph(world: &mut World) {
    let node = ViewNodeRunner::new(EntityShadowNode, world);
    let Some(mut graphs) = world.get_resource_mut::<RenderGraph>() else {
        return;
    };
    let Some(graph) = graphs.get_sub_graph_mut(Core3d) else {
        return;
    };
    graph.add_node(EntityShadowLabel, node);
    graph.add_node_edges((
        Node3d::MainOpaquePass,
        EntityShadowLabel,
        Node3d::MainTransmissivePass,
    ));
    // Whichever plugin installs second orders shadows after late terrain draws.
    if graph.get_node_state(crate::chunk::GpuCullLateLabel).is_ok() {
        let _ = graph.try_add_node_edge(crate::chunk::GpuCullLateLabel, EntityShadowLabel);
    }
}

#[derive(Resource)]
pub(crate) struct EntityShadowGpu {
    mesh: Buffer,
    instances: Buffer,
    capacity: u64,
    params: Buffer,
    params_value: Option<EntityShadowParams>,
    uploaded_revision: Option<u64>,
    count: u32,
    layout: BindGroupLayoutDescriptor,
    /// Pipelines keyed by the view's colour format; at most HDR and the sRGB default.
    pipelines: Vec<(TextureFormat, CachedRenderPipelineId)>,
    /// Instance and parameter uploads, for the unchanged-frame contract.
    pub(crate) uploads: u64,
}

impl EntityShadowGpu {
    fn new(device: &RenderDevice) -> Self {
        let mesh = shadow_volume_mesh();
        let mesh =
            device.create_buffer_with_data(&bevy::render::render_resource::BufferInitDescriptor {
                label: Some("entity shadow volume"),
                contents: bytemuck::cast_slice(&mesh),
                usage: BufferUsages::VERTEX,
            });
        let fragment = ShaderStages::FRAGMENT;
        let entry = |binding, visibility, ty| BindGroupLayoutEntry {
            binding,
            visibility,
            ty,
            count: None,
        };
        let layout = BindGroupLayoutDescriptor::new(
            "entity shadow bind group layout",
            &[
                entry(
                    0,
                    ShaderStages::VERTEX_FRAGMENT,
                    BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                ),
                entry(
                    1,
                    fragment,
                    BindingType::Texture {
                        sample_type: TextureSampleType::Depth,
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    2,
                    fragment,
                    BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                ),
                entry(
                    3,
                    ShaderStages::VERTEX_FRAGMENT,
                    BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(INSTANCE_BYTES),
                    },
                ),
                entry(
                    4,
                    fragment,
                    BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(size_of::<EntityShadowParams>() as u64),
                    },
                ),
            ],
        );
        Self {
            mesh,
            instances: instance_buffer(device, 64),
            capacity: 64,
            params: device.create_buffer(&BufferDescriptor {
                label: Some("entity shadow parameters"),
                size: size_of::<EntityShadowParams>() as u64,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            params_value: None,
            uploaded_revision: None,
            count: 0,
            layout,
            pipelines: Vec::new(),
            uploads: 0,
        }
    }

    fn pipeline(&mut self, cache: &PipelineCache, format: TextureFormat) -> CachedRenderPipelineId {
        if let Some((_, id)) = self.pipelines.iter().find(|(known, _)| *known == format) {
            return *id;
        }
        let id = cache.queue_render_pipeline(pipeline_descriptor(self.layout.clone(), format));
        self.pipelines.push((format, id));
        id
    }
}

fn instance_buffer(device: &RenderDevice, capacity: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some("entity shadow casters"),
        size: capacity * INSTANCE_BYTES,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

/// Back faces only, so each covered pixel shades once per volume; no blending, so overlapping
/// volumes rewrite the same value and never darken twice.
fn pipeline_descriptor(
    layout: BindGroupLayoutDescriptor,
    format: TextureFormat,
) -> RenderPipelineDescriptor {
    let shader_defs = if format.is_srgb() {
        vec!["GAMMA_TARGET".into()]
    } else {
        Vec::new()
    };
    RenderPipelineDescriptor {
        label: Some("entity shadow pipeline".into()),
        layout: vec![layout],
        vertex: VertexState {
            shader: SHADER,
            shader_defs: shader_defs.clone(),
            entry_point: Some("shadow_vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: 12,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            }],
        },
        fragment: Some(FragmentState {
            shader: SHADER,
            shader_defs,
            entry_point: Some("shadow_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::COLOR,
            })],
        }),
        primitive: PrimitiveState {
            cull_mode: Some(Face::Front),
            ..default()
        },
        ..default()
    }
}

/// The colour multiplier for this frame's sky; Nether and End have no sunrise glow.
fn shadow_params(atmosphere: &AtmosphereFrame) -> EntityShadowParams {
    let sunrise = if atmosphere.sky_kind() == SkyKind::Overworld {
        crate::celestial::raw_sunrise_band(atmosphere.celestial_angle())
    } else {
        [0.0; 4]
    };
    let sky = atmosphere.sky_zenith().map(linear_to_srgb);
    EntityShadowParams::new(entity_shadow_colour(sky, sunrise))
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

pub(crate) fn prepare_shadow_buffers(
    scene: Res<EntityShadowScene>,
    atmosphere: Option<Res<AtmosphereFrame>>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<EntityShadowGpu>,
) {
    let frame = &scene.0;
    if gpu.uploaded_revision != Some(frame.revision) {
        let needed = frame.shadows.len() as u64;
        if needed > gpu.capacity {
            gpu.capacity = needed.next_power_of_two();
            gpu.instances = instance_buffer(&device, gpu.capacity);
        }
        if needed > 0 {
            queue.write_buffer(&gpu.instances, 0, bytemuck::cast_slice(&frame.shadows));
            gpu.uploads += 1;
        }
        gpu.count = needed as u32;
        gpu.uploaded_revision = Some(frame.revision);
    }
    let params = shadow_params(&atmosphere.map(|frame| *frame).unwrap_or_default());
    if gpu.params_value != Some(params) {
        queue.write_buffer(&gpu.params, 0, bytemuck::bytes_of(&params));
        gpu.params_value = Some(params);
        gpu.uploads += 1;
    }
}

/// Per-view copy of the opaque scene the shader reads, plus this frame's covered rectangle.
#[derive(Component)]
pub(crate) struct EntityShadowView {
    scratch: Texture,
    scratch_view: TextureView,
    pipeline: CachedRenderPipelineId,
    rect: Option<[u32; 4]>,
    bind_group: Option<(BindGroupKey, BindGroup)>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct BindGroupKey {
    depth: TextureViewId,
    scratch: TextureViewId,
    instances: BufferId,
    view_uniforms: BufferId,
}

type ShadowViews<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static Msaa,
        Option<&'static mut EntityShadowView>,
    ),
    With<Camera3d>,
>;

pub(crate) fn prepare_shadow_views(
    mut commands: Commands,
    scene: Res<EntityShadowScene>,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    mut gpu: ResMut<EntityShadowGpu>,
    mut views: ShadowViews,
) {
    for (entity, view, target, msaa, state) in &mut views {
        let clip_from_world = view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
        let rect = (*msaa == Msaa::Off)
            .then(|| {
                shadow_screen_rect(clip_from_world, &scene.0.shadows, view.viewport.to_array())
            })
            .flatten();
        let main = target.main_texture();
        let format = main.format().remove_srgb_suffix();
        if let Some(mut state) = state {
            state.rect = rect;
            if rect.is_none()
                || (state.scratch.size() == main.size() && state.scratch.format() == format)
            {
                continue;
            }
        }
        if rect.is_none() {
            continue;
        }
        let scratch = device.create_texture(&TextureDescriptor {
            label: Some("entity shadow scene copy"),
            size: main.size(),
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let scratch_view = scratch.create_view(&Default::default());
        let pipeline = gpu.pipeline(&cache, target.main_texture_format());
        commands.entity(entity).insert(EntityShadowView {
            scratch,
            scratch_view,
            pipeline,
            rect,
            bind_group: None,
        });
    }
}

pub(crate) fn prepare_shadow_bind_groups(
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    gpu: Res<EntityShadowGpu>,
    view_uniforms: Res<ViewUniforms>,
    mut views: Query<(&mut EntityShadowView, &ViewDepthTexture)>,
) {
    let (Some(binding), Some(uniforms)) = (
        view_uniforms.uniforms.binding(),
        view_uniforms.uniforms.buffer(),
    ) else {
        return;
    };
    for (mut state, depth) in &mut views {
        if state.rect.is_none() || !depth_is_sampleable(depth) {
            continue;
        }
        let key = BindGroupKey {
            depth: depth.view().id(),
            scratch: state.scratch_view.id(),
            instances: gpu.instances.id(),
            view_uniforms: uniforms.id(),
        };
        if state
            .bind_group
            .as_ref()
            .is_some_and(|(known, _)| *known == key)
        {
            continue;
        }
        let bind_group = device.create_bind_group(
            "entity shadow bind group",
            &cache.get_bind_group_layout(&gpu.layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: binding.clone(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(depth.view()),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&state.scratch_view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: gpu.instances.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: gpu.params.as_entire_binding(),
                },
            ],
        );
        state.bind_group = Some((key, bind_group));
    }
}

fn depth_is_sampleable(depth: &ViewDepthTexture) -> bool {
    depth
        .texture
        .usage()
        .contains(TextureUsages::TEXTURE_BINDING)
        && depth.texture.sample_count() == 1
}

#[derive(Default)]
struct EntityShadowNode;

impl ViewNode for EntityShadowNode {
    type ViewQuery = (
        &'static ExtractedCamera,
        &'static ViewTarget,
        &'static ViewUniformOffset,
        &'static EntityShadowView,
    );

    fn run<'w>(
        &self,
        _graph: &mut RenderGraphContext,
        context: &mut RenderContext<'w>,
        (camera, target, offset, state): QueryItem<'w, '_, Self::ViewQuery>,
        world: &'w World,
    ) -> Result<(), NodeRunError> {
        let gpu = world.resource::<EntityShadowGpu>();
        let (Some([x0, y0, x1, y1]), Some((_, bind_group)), Some(pipeline)) = (
            state.rect,
            state.bind_group.as_ref(),
            world
                .resource::<PipelineCache>()
                .get_render_pipeline(state.pipeline),
        ) else {
            return Ok(());
        };
        if gpu.count == 0
            || world
                .get_resource::<crate::PanoramaScene>()
                .is_some_and(|panorama| !panorama.game_visible())
        {
            return Ok(());
        }
        let origin = Origin3d { x: x0, y: y0, z: 0 };
        let mut source = target.main_texture().as_image_copy();
        source.origin = origin;
        let mut destination = state.scratch.as_image_copy();
        destination.origin = origin;
        context.command_encoder().copy_texture_to_texture(
            source,
            destination,
            Extent3d {
                width: x1 - x0,
                height: y1 - y0,
                depth_or_array_layers: 1,
            },
        );
        let colour = RenderPassColorAttachment {
            view: target.main_texture_view(),
            depth_slice: None,
            resolve_target: None,
            ops: Operations {
                load: LoadOp::Load,
                store: StoreOp::Store,
            },
        };
        let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
            label: Some("entity shadows"),
            color_attachments: &[Some(colour)],
            depth_stencil_attachment: None,
            timestamp_writes: crate::gpu_timing::render_pass_timestamps(
                world,
                crate::RuntimeStage::GpuShadows,
            ),
            occlusion_query_set: None,
        });
        if let Some(viewport) = camera.viewport.as_ref() {
            pass.set_camera_viewport(viewport);
        }
        pass.set_scissor_rect(x0, y0, x1 - x0, y1 - y0);
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, bind_group, &[offset.offset]);
        pass.set_vertex_buffer(0, gpu.mesh.slice(..));
        pass.draw(0..SHADOW_VOLUME_VERTICES as u32, 0..gpu.count);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
