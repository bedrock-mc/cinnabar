use std::mem::size_of;

use crate::actor::{
    ActorDrawFrame, ActorGpuInstance, ActorPresentationGate, ActorRenderFrame,
    ActorRigGeometrySpan, ActorRigVertex, STANDARD_SKIN_BYTES, STANDARD_SKIN_SIDE,
    gpu::ActorDrawTracker,
};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParam, SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
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
            BufferDescriptor, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CommandEncoderDescriptor, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, PipelineCache, PollType,
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

const ACTOR_SHADER_HANDLE: Handle<Shader> = uuid_handle!("09d34708-6fd4-4c65-b27e-ce22f172cc73");
#[cfg(test)]
const ACTOR_SHADER_SOURCE: &str = include_str!("actor.wgsl");

#[derive(Debug, Clone, Copy, Default)]
pub struct ActorRenderPlugin;

impl Plugin for ActorRenderPlugin {
    fn build(&self, app: &mut App) {
        install_actor_render(app);
    }

    fn finish(&self, app: &mut App) {
        install_actor_render(app);
    }
}

#[derive(Resource)]
struct ActorRenderInstalled;

fn install_actor_render(app: &mut App) {
    app.init_resource::<ActorRenderFrame>()
        .init_resource::<ActorPresentationGate>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<ActorRenderInstalled>()
    {
        return;
    }
    let presentation_gate = app.world().resource::<ActorPresentationGate>().clone();
    app.add_plugins(ExtractResourcePlugin::<ActorRenderFrame>::default());
    load_internal_asset!(app, ACTOR_SHADER_HANDLE, "actor.wgsl", Shader::from_wgsl);
    app.sub_app_mut(RenderApp)
        .insert_resource(ActorRenderInstalled)
        .insert_resource(presentation_gate)
        .init_resource::<ActorPipeline>()
        .init_resource::<ActorDrawTracker>()
        .add_render_command::<Opaque3d, DrawActorCommands>()
        .add_systems(RenderStartup, init_actor_gpu)
        .add_systems(
            Render,
            (
                prepare_actor_resources.in_set(RenderSystems::PrepareResources),
                prepare_actor_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_actors.in_set(RenderSystems::Queue),
                submit_actor_presented_frame
                    .in_set(RenderSystems::Render)
                    .after(bevy::render::renderer::render_system),
            ),
        );
}

#[derive(Resource)]
struct ActorGpu {
    instance_buffer: Buffer,
    previous_bone_buffer: Buffer,
    current_bone_buffer: Buffer,
    geometry_vertex_buffer: Option<Buffer>,
    geometry_span_buffer: Option<Buffer>,
    instance_count: u32,
    maximum_vertex_count: u32,
    skin_texture: Option<Texture>,
    skin_view: Option<TextureView>,
    sampler: Sampler,
    bind_group: Option<BindGroup>,
    frame_generation: u64,
    geometry_revision: u64,
    skin_revision: u64,
    view_buffer_id: Option<BufferId>,
    manifest: std::sync::Arc<[crate::actor::ActorDrawManifestEntry]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ActorSkinUploadPlan {
    layer_count: u32,
}

fn actor_skin_upload_plan(frame: &ActorRenderFrame) -> Option<ActorSkinUploadPlan> {
    if frame.rig.instances.is_empty()
        || frame.skins_rgba8.is_empty()
        || !frame.skins_rgba8.len().is_multiple_of(STANDARD_SKIN_BYTES)
    {
        return None;
    }
    let layer_count = frame.skins_rgba8.len() / STANDARD_SKIN_BYTES;
    if layer_count > crate::actor::MAX_RENDERED_PLAYERS
        || frame
            .rig
            .instances
            .iter()
            .any(|instance| instance.texture_layer as usize >= layer_count)
    {
        return None;
    }
    Some(ActorSkinUploadPlan {
        layer_count: u32::try_from(layer_count).ok()?,
    })
}

fn init_actor_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("nearest standard player skin sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        address_mode_w: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        mipmap_filter: FilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(ActorGpu {
        instance_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor instance arena"),
            size: (crate::actor::MAX_RENDERED_PLAYERS * size_of::<ActorGpuInstance>()) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        previous_bone_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor previous-bone arena"),
            size: (crate::actor::MAX_ACTOR_BONE_ARENA_BYTES / 2) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        current_bone_buffer: render_device.create_buffer(&BufferDescriptor {
            label: Some("bounded shared actor current-bone arena"),
            size: (crate::actor::MAX_ACTOR_BONE_ARENA_BYTES / 2) as u64,
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        geometry_vertex_buffer: None,
        geometry_span_buffer: None,
        instance_count: 0,
        maximum_vertex_count: 0,
        skin_texture: None,
        skin_view: None,
        sampler,
        bind_group: None,
        frame_generation: u64::MAX,
        geometry_revision: u64::MAX,
        skin_revision: u64::MAX,
        view_buffer_id: None,
        manifest: std::sync::Arc::from([]),
    });
}

fn prepare_actor_resources(
    frame: Res<ActorRenderFrame>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<ActorGpu>,
) {
    let rig = &frame.rig;
    let skin_upload_plan = actor_skin_upload_plan(&frame);
    if gpu.geometry_revision != rig.geometry_revision {
        if rig.geometry_vertices.is_empty() || rig.geometry_spans.is_empty() {
            gpu.geometry_vertex_buffer = None;
            gpu.geometry_span_buffer = None;
        } else {
            gpu.geometry_vertex_buffer = Some(render_device.create_buffer_with_data(
                &BufferInitDescriptor {
                    label: Some("shared immutable actor rig vertices"),
                    contents: bytemuck::cast_slice::<ActorRigVertex, u8>(&rig.geometry_vertices),
                    usage: BufferUsages::STORAGE,
                },
            ));
            gpu.geometry_span_buffer = Some(render_device.create_buffer_with_data(
                &BufferInitDescriptor {
                    label: Some("shared immutable actor rig geometry spans"),
                    contents: bytemuck::cast_slice::<ActorRigGeometrySpan, u8>(&rig.geometry_spans),
                    usage: BufferUsages::STORAGE,
                },
            ));
        }
        gpu.geometry_revision = rig.geometry_revision;
        gpu.bind_group = None;
    }
    if gpu.frame_generation != rig.frame_generation {
        let valid = !rig.instances.is_empty()
            && rig.instances.len() <= crate::actor::MAX_RENDERED_PLAYERS
            && rig.previous_bones.len() == rig.current_bones.len()
            && rig.previous_bones.len()
                <= crate::actor::MAX_RENDERED_PLAYERS * crate::actor::MAX_RENDER_BONES_PER_ACTOR
            && rig.manifest.len() == rig.instances.len()
            && rig.maximum_vertex_count != 0
            && skin_upload_plan.is_some();
        if valid {
            render_queue.write_buffer(
                &gpu.instance_buffer,
                0,
                bytemuck::cast_slice::<ActorGpuInstance, u8>(&rig.instances),
            );
            render_queue.write_buffer(
                &gpu.previous_bone_buffer,
                0,
                bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&rig.previous_bones),
            );
            render_queue.write_buffer(
                &gpu.current_bone_buffer,
                0,
                bytemuck::cast_slice::<[[f32; 4]; 3], u8>(&rig.current_bones),
            );
            gpu.instance_count = rig.instances.len() as u32;
            gpu.maximum_vertex_count = rig.maximum_vertex_count;
            gpu.manifest = std::sync::Arc::clone(&rig.manifest);
        } else {
            gpu.instance_count = 0;
            gpu.maximum_vertex_count = 0;
            gpu.manifest = std::sync::Arc::from([]);
        }
        gpu.frame_generation = rig.frame_generation;
    }
    if gpu.skin_revision != frame.skin_revision {
        let Some(plan) = skin_upload_plan else {
            gpu.skin_texture = None;
            gpu.skin_view = None;
            gpu.instance_count = 0;
            gpu.skin_revision = frame.skin_revision;
            gpu.bind_group = None;
            return;
        };
        let texture = render_device.create_texture_with_data(
            &render_queue,
            &TextureDescriptor {
                label: Some("bounded normalized server player skins"),
                size: Extent3d {
                    width: STANDARD_SKIN_SIDE as u32,
                    height: STANDARD_SKIN_SIDE as u32,
                    depth_or_array_layers: plan.layer_count,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format: TextureFormat::Rgba8UnormSrgb,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                view_formats: &[],
            },
            TextureDataOrder::LayerMajor,
            &frame.skins_rgba8,
        );
        let view = texture.create_view(&TextureViewDescriptor {
            label: Some("bounded normalized server player skin array"),
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
        gpu.skin_texture = Some(texture);
        gpu.skin_view = Some(view);
        gpu.skin_revision = frame.skin_revision;
        gpu.bind_group = None;
    }
}

struct ActorPipelineSpecializer;

#[derive(Resource)]
struct ActorPipeline {
    variants: Variants<RenderPipeline, ActorPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ActorPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = actor_bind_group_layout();
        let descriptor = actor_pipeline_descriptor(bind_group_layout.clone());
        Self {
            variants: Variants::new(ActorPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

fn actor_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "instanced actor bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: Some(ViewUniform::min_size()),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorGpuInstance>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigVertex>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<ActorRigGeometrySpan>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 5,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<[[f32; 4]; 3]>() as u64),
                },
                count: None,
            },
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
        ],
    )
}

fn actor_pipeline_descriptor(
    bind_group_layout: BindGroupLayoutDescriptor,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("instanced standard Bedrock biped pipeline".into()),
        layout: vec![bind_group_layout],
        vertex: VertexState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_vertex".into()),
            buffers: vec![],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: ACTOR_SHADER_HANDLE,
            entry_point: Some("actor_fragment".into()),
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

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct ActorPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for ActorPipelineSpecializer {
    type Key = ActorPipelineKey;

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

fn prepare_actor_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<ActorPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<ActorGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        return;
    };
    let Some(geometry_vertex_buffer) = gpu.geometry_vertex_buffer.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let Some(geometry_span_buffer) = gpu.geometry_span_buffer.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let Some(skin_view) = gpu.skin_view.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.bind_group.is_some() && gpu.view_buffer_id == Some(view_buffer.id()) {
        return;
    }
    gpu.bind_group = Some(render_device.create_bind_group(
        "instanced standard actor bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: gpu.instance_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: geometry_vertex_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 3,
                resource: geometry_span_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: gpu.previous_bone_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 5,
                resource: gpu.current_bone_buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 6,
                resource: BindingResource::TextureView(skin_view),
            },
            BindGroupEntry {
                binding: 7,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
}

#[derive(SystemParam)]
struct QueueActorParams<'w, 's> {
    pipeline_cache: Res<'w, PipelineCache>,
    pipeline: ResMut<'w, ActorPipeline>,
    gpu: Res<'w, ActorGpu>,
    phases: ResMut<'w, ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<'w, DrawFunctions<Opaque3d>>,
    views: Query<
        'w,
        's,
        (
            Entity,
            &'static MainEntity,
            &'static ExtractedView,
            &'static Msaa,
        ),
    >,
    draw_tracker: Res<'w, ActorDrawTracker>,
}

fn queue_actors(
    mut params: QueueActorParams<'_, '_>,
    mut next_tick: Local<Tick>,
    mut next_draw_generation: Local<u64>,
) {
    params.draw_tracker.clear();
    if params.gpu.instance_count == 0 || params.gpu.bind_group.is_none() {
        return;
    }
    let draw_function = params.draw_functions.read().id::<DrawActorCommands>();
    let mut queued = false;
    for (view_entity, main_entity, view, msaa) in &params.views {
        let Some(phase) = params.phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = params.pipeline.variants.specialize(
            &params.pipeline_cache,
            ActorPipelineKey {
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
                asset_id: AssetId::<Shader>::invalid().untyped(),
            },
            (view_entity, *main_entity),
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
            *next_tick,
        );
        queued = true;
    }
    if queued {
        let Some(draw_generation) = next_draw_generation.checked_add(1) else {
            return;
        };
        *next_draw_generation = draw_generation;
        let _ = params.draw_tracker.begin(ActorDrawFrame {
            frame_generation: params.gpu.frame_generation,
            draw_generation,
            manifest: std::sync::Arc::clone(&params.gpu.manifest),
        });
    }
}

type DrawActorCommands = (SetItemPipeline, SetActorBindGroup<0>, DrawActors);

struct SetActorBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetActorBindGroup<I> {
    type Param = SRes<ActorGpu>;
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

struct DrawActors;

impl<P: PhaseItem> RenderCommand<P> for DrawActors {
    type Param = (SRes<ActorGpu>, SRes<ActorDrawTracker>);
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        params: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (gpu, tracker) = params;
        let gpu = gpu.into_inner();
        pass.draw(0..gpu.maximum_vertex_count, 0..gpu.instance_count);
        tracker.into_inner().record_draw();
        RenderCommandResult::Success
    }
}

fn submit_actor_presented_frame(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    tracker: Res<ActorDrawTracker>,
    gate: Res<ActorPresentationGate>,
) {
    let Some(draw) = tracker.take_drawn() else {
        if let Err(error) = render_device.poll(PollType::Poll) {
            bevy::log::warn!(
                ?error,
                "could not nonblockingly poll actor presentation fence"
            );
        }
        return;
    };
    let Some(token) = gate.try_reserve_callback(draw) else {
        return;
    };
    let present_returned_at = std::time::Instant::now();
    let encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("actor presented-frame completion sentinel"),
    });
    let command_buffer = encoder.finish();
    let callback_gate = gate.clone();
    command_buffer.on_submitted_work_done(move || {
        callback_gate.publish_reserved(token, present_returned_at, std::time::Instant::now());
    });
    render_queue.submit([command_buffer]);
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use bevy::{
        app::SubApp,
        asset::Assets,
        core_pipeline::core_3d::{Opaque3d, Transparent3d},
        ecs::schedule::Schedule,
        prelude::{App, Shader},
        render::{
            ExtractSchedule, Render, RenderApp, RenderStartup,
            render_phase::DrawFunctions,
            renderer::{RenderDevice, RenderQueue, WgpuWrapper},
        },
    };

    use super::{
        ACTOR_SHADER_SOURCE, ActorGpu, ActorPipelineKey, ActorPipelineSpecializer,
        ActorRenderInstalled, ActorRenderPlugin, actor_bind_group_layout,
        actor_pipeline_descriptor, actor_skin_upload_plan,
    };

    #[test]
    fn shared_skin_layer_prepares_one_texture_layer_for_multiple_actors() {
        let mut frame = crate::actor::ActorRenderFrame::default();
        frame.rig.instances = Arc::from([
            crate::actor::ActorGpuInstance {
                texture_layer: 0,
                ..Default::default()
            },
            crate::actor::ActorGpuInstance {
                texture_layer: 0,
                ..Default::default()
            },
        ]);
        frame.skins_rgba8 = vec![255; crate::actor::STANDARD_SKIN_BYTES].into();

        let plan = actor_skin_upload_plan(&frame)
            .expect("a shared normalized skin family remains drawable");

        assert_eq!(plan.layer_count, 1);
    }

    #[test]
    fn skin_upload_preparation_rejects_misaligned_bytes_and_out_of_range_layers() {
        let mut frame = crate::actor::ActorRenderFrame::default();
        frame.rig.instances = Arc::from([crate::actor::ActorGpuInstance {
            texture_layer: 0,
            ..Default::default()
        }]);
        frame.skins_rgba8 = vec![255; crate::actor::STANDARD_SKIN_BYTES - 1].into();
        assert!(actor_skin_upload_plan(&frame).is_none());

        frame.skins_rgba8 = vec![255; crate::actor::STANDARD_SKIN_BYTES].into();
        Arc::make_mut(&mut frame.rig.instances)[0].texture_layer = 1;
        assert!(actor_skin_upload_plan(&frame).is_none());
    }

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

    #[test]
    fn actor_shader_parses_as_wgsl() {
        let source = ACTOR_SHADER_SOURCE.replace(
            "#import bevy_render::view::View",
            "struct View { clip_from_world: mat4x4<f32>, }",
        );
        naga::front::wgsl::parse_str(&source).expect("actor shader parses");
    }

    #[test]
    fn plugin_install_is_idempotent_and_starts_one_shared_gpu_state() {
        let mut app = app_with_noop_render_sub_app();
        app.add_plugins(ActorRenderPlugin);
        app.finish();

        let render_app = app.sub_app_mut(RenderApp);
        assert!(
            render_app
                .world()
                .contains_resource::<ActorRenderInstalled>()
        );
        render_app.world_mut().run_schedule(RenderStartup);
        assert!(render_app.world().contains_resource::<ActorGpu>());
    }

    #[test]
    fn pipeline_descriptor_specializes_and_noop_backend_accepts_the_binding_layout() {
        use bevy::prelude::Msaa;
        use bevy::render::{
            render_resource::{ShaderStages, Specializer},
            view::ViewTarget,
        };

        let layout = actor_bind_group_layout();
        assert_eq!(layout.entries.len(), 8);
        assert_eq!(layout.entries[0].visibility, ShaderStages::VERTEX);
        assert_eq!(layout.entries[1].visibility, ShaderStages::VERTEX);
        assert_eq!(layout.entries[2].visibility, ShaderStages::VERTEX);
        assert_eq!(layout.entries[3].visibility, ShaderStages::VERTEX);
        assert_eq!(layout.entries[4].visibility, ShaderStages::VERTEX);
        assert_eq!(layout.entries[5].visibility, ShaderStages::VERTEX);
        assert_eq!(layout.entries[6].visibility, ShaderStages::FRAGMENT);
        assert_eq!(layout.entries[7].visibility, ShaderStages::FRAGMENT);

        let mut descriptor = actor_pipeline_descriptor(layout.clone());
        ActorPipelineSpecializer
            .specialize(
                ActorPipelineKey {
                    msaa: Msaa::Sample4,
                    hdr: true,
                },
                &mut descriptor,
            )
            .expect("actor pipeline specializes");
        assert_eq!(descriptor.multisample.count, 4);
        assert_eq!(
            descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .format,
            ViewTarget::TEXTURE_FORMAT_HDR
        );

        let app = app_with_noop_render_sub_app();
        let render_device = app.sub_app(RenderApp).world().resource::<RenderDevice>();
        render_device.create_bind_group_layout("actor layout validation", &layout.entries);
    }
}
