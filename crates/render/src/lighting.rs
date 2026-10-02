use crate::lightmap::LightmapInputs;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderSystems,
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::{PhaseItem, RenderCommand, RenderCommandResult, TrackedRenderPass},
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
};

/// Shared classic lightmap inputs, published with the current environment frame.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldLighting(pub LightmapInputs);

#[derive(Resource)]
pub(crate) struct LightmapGpu {
    buffer: Buffer,
    pub(crate) bind_group: BindGroup,
    inputs: LightmapInputs,
    atmosphere: Buffer,
}

/// Describes group one shared by every ordinary lit world pipeline.
pub(crate) fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "world RGB lightmap",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<[[f32; 4]; 256]>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(
                        std::mem::size_of::<crate::AtmosphereFrame>() as u64,
                    ),
                },
                count: None,
            },
        ],
    )
}

#[derive(Resource)]
struct LightingInstalled;

/// Installs one lightmap upload, independent of terrain or actor publication revisions.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<WorldLighting>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<LightingInstalled>() {
        return;
    }
    app.sub_app_mut(RenderApp)
        .insert_resource(LightingInstalled);
    app.add_plugins(ExtractResourcePlugin::<WorldLighting>::default());
    const MATERIAL_SHADER: Handle<Shader> = uuid_handle!("40309d5a-76a4-4e3b-aed0-d5c76aa5d52e");

    const SHADER: Handle<Shader> = uuid_handle!("4562a3ce-92ab-46f2-823f-af9faf2cc5c8");
    load_internal_asset!(app, SHADER, "lighting.wgsl", Shader::from_wgsl);
    load_internal_asset!(app, MATERIAL_SHADER, "material.wgsl", Shader::from_wgsl);
    app.sub_app_mut(RenderApp)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources));
}

/// Rebuilds the small table only when an environment input changes.
fn prepare(
    mut commands: Commands,
    input: Res<WorldLighting>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    gpu: Option<ResMut<LightmapGpu>>,
    atmosphere: Option<Res<crate::atmosphere_render::AtmosphereGpu>>,
) {
    if let Some(mut gpu) = gpu {
        if gpu.inputs != input.0 {
            queue.write_buffer(&gpu.buffer, 0, bytemuck::cast_slice(&input.0.build()));
            gpu.inputs = input.0;
        }
        if let Some(atmosphere) = atmosphere.as_deref()
            && gpu.atmosphere.id() != atmosphere.buffer.id()
        {
            gpu.atmosphere = atmosphere.buffer.clone();
            gpu.bind_group = bind_group(&device, &cache, &gpu.buffer, &gpu.atmosphere);
        }
        return;
    }
    let buffer = device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("world RGB lightmap"),
        contents: bytemuck::cast_slice(&input.0.build()),
        usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
    });
    let atmosphere = atmosphere.map(|gpu| gpu.buffer.clone()).unwrap_or_else(|| {
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("fallback world atmosphere"),
            contents: bytemuck::bytes_of(&crate::AtmosphereFrame::default()),
            usage: BufferUsages::UNIFORM,
        })
    });
    let bind_group = bind_group(&device, &cache, &buffer, &atmosphere);
    commands.insert_resource(LightmapGpu {
        buffer,
        bind_group,
        inputs: input.0,
        atmosphere,
    });
}

/// Binds the shared environment buffers for ordinary world passes.
fn bind_group(
    device: &RenderDevice,
    cache: &PipelineCache,
    light: &Buffer,
    atmosphere: &Buffer,
) -> BindGroup {
    device.create_bind_group(
        "world lighting and fog",
        &cache.get_bind_group_layout(&layout()),
        &[
            BindGroupEntry {
                binding: 0,
                resource: light.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 1,
                resource: atmosphere.as_entire_binding(),
            },
        ],
    )
}

pub(crate) struct SetWorldLightmap;
impl<P: PhaseItem> RenderCommand<P> for SetWorldLightmap {
    type Param = SRes<LightmapGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _: &P,
        _: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(1, &gpu.into_inner().bind_group, &[]);
        RenderCommandResult::Success
    }
}
