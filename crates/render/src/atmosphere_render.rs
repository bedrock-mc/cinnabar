use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    mesh::Mesh,
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
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, Buffer, BufferBindingType, BufferId, BufferInitDescriptor, BufferUsages,
            Canonical, ColorTargetState, ColorWrites, CompareFunction, DepthStencilState,
            FragmentState, PipelineCache, RenderPipeline, RenderPipelineDescriptor, ShaderStages,
            ShaderType, Specializer, SpecializerKey, TextureFormat, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use crate::AtmosphereFrame;

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
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<AtmosphereRenderInstalled>()
    {
        return;
    }

    app.add_plugins(ExtractResourcePlugin::<AtmosphereFrame>::default());
    load_internal_asset!(
        app,
        ATMOSPHERE_SHADER_HANDLE,
        "atmosphere.wgsl",
        Shader::from_wgsl
    );

    app.sub_app_mut(RenderApp)
        .insert_resource(AtmosphereRenderInstalled)
        .init_resource::<AtmospherePipeline>()
        .add_render_command::<Opaque3d, DrawAtmosphereCommands>()
        .add_systems(RenderStartup, init_atmosphere_gpu)
        .add_systems(
            Render,
            (
                prepare_atmosphere_uniform.in_set(RenderSystems::PrepareResources),
                prepare_atmosphere_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_atmosphere.in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct AtmosphereGpu {
    pub(crate) buffer: Buffer,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
}

fn init_atmosphere_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    AtmosphereFrame::assert_uniform_compat();
    let buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("global atmosphere frame"),
        contents: bytemuck::bytes_of(&AtmosphereFrame::default()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    commands.insert_resource(AtmosphereGpu {
        buffer,
        bind_group: None,
        view_buffer_id: None,
    });
}

fn prepare_atmosphere_uniform(
    frame: Res<AtmosphereFrame>,
    gpu: Res<AtmosphereGpu>,
    render_queue: Res<RenderQueue>,
) {
    render_queue.write_buffer(&gpu.buffer, 0, bytemuck::bytes_of(&*frame));
}

struct AtmospherePipelineSpecializer;

#[derive(Resource)]
struct AtmospherePipeline {
    variants: Variants<RenderPipeline, AtmospherePipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for AtmospherePipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "procedural atmosphere bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(AtmosphereFrame::min_size()),
                    },
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("procedural atmosphere pipeline".into()),
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
                    blend: None,
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
}

impl Specializer<RenderPipeline> for AtmospherePipelineSpecializer {
    type Key = AtmospherePipelineKey;

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
        "procedural atmosphere bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: gpu.buffer.as_entire_binding(),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
}

fn queue_atmosphere(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<AtmospherePipeline>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
    mut next_tick: Local<Tick>,
) {
    let draw_function = draw_functions.read().id::<DrawAtmosphereCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            AtmospherePipelineKey {
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
                asset_id: AssetId::<Mesh>::invalid().untyped(),
            },
            (view_entity, *main_entity),
            InputUniformIndex::default(),
            BinnedRenderPhaseType::NonMesh,
            *next_tick,
        );
    }
}

type DrawAtmosphereCommands = (SetItemPipeline, SetAtmosphereBindGroup<0>, DrawAtmosphere);

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
    type Param = ();
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        _param: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.draw(0..3, 0..1);
        RenderCommandResult::Success
    }
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

    use super::{AtmosphereGpu, AtmosphereRenderInstalled};
    use crate::DebugWorldPlugin;

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
    fn standalone_chunk_renderer_installs_and_starts_atmosphere_gpu_resources() {
        let mut app = app_with_noop_render_sub_app();
        app.add_plugins(DebugWorldPlugin::new(1));
        app.finish();

        let render_app = app.sub_app_mut(RenderApp);
        assert!(
            render_app
                .world()
                .contains_resource::<AtmosphereRenderInstalled>()
        );
        render_app.world_mut().run_schedule(RenderStartup);
        assert!(render_app.world().contains_resource::<AtmosphereGpu>());
    }
}
