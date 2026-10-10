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

/// A transient personal-mod override; the ordinary environment inputs remain intact.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldFullbright(pub bool);

#[derive(Resource)]
pub(crate) struct LightmapGpu {
    buffer: Buffer,
    pub(crate) bind_group: BindGroup,
    inputs: Option<LightmapInputs>,
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
    app.init_resource::<WorldLighting>()
        .init_resource::<WorldFullbright>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<LightingInstalled>() {
        return;
    }
    app.sub_app_mut(RenderApp)
        .insert_resource(LightingInstalled);
    app.add_plugins(ExtractResourcePlugin::<WorldLighting>::default());
    app.add_plugins(ExtractResourcePlugin::<WorldFullbright>::default());
    const MATERIAL_SHADER: Handle<Shader> = uuid_handle!("40309d5a-76a4-4e3b-aed0-d5c76aa5d52e");

    const SHADER: Handle<Shader> = uuid_handle!("4562a3ce-92ab-46f2-823f-af9faf2cc5c8");
    load_internal_asset!(app, SHADER, "lighting.wgsl", |source, path| {
        crate::shader_safety::from_wgsl(crate::material_shader::source(source), path)
    });
    load_internal_asset!(app, MATERIAL_SHADER, "material.wgsl", |source, path| {
        crate::shader_safety::from_wgsl(crate::material_shader::source(source), path)
    });
    // Terrain, models, liquids and block overlays share one projection.
    const WORLD_PROJECTION_SHADER: Handle<Shader> =
        uuid_handle!("b6411dfb-6a07-4283-ab58-3aedb7cdb856");
    load_internal_asset!(
        app,
        WORLD_PROJECTION_SHADER,
        "world_projection.wgsl",
        crate::shader_safety::from_wgsl
    );
    app.sub_app_mut(RenderApp)
        .add_systems(Render, prepare.in_set(RenderSystems::PrepareResources));
}

/// Rebuilds the small table only when an environment input changes.
fn prepare(
    mut commands: Commands,
    (input, fullbright): (Res<WorldLighting>, Res<WorldFullbright>),
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    gpu: Option<ResMut<LightmapGpu>>,
    atmosphere: Option<Res<crate::atmosphere_render::AtmosphereGpu>>,
) {
    let inputs = (!fullbright.0).then_some(input.0);
    if let Some(mut gpu) = gpu {
        if gpu.inputs != inputs {
            queue.write_buffer(&gpu.buffer, 0, bytemuck::cast_slice(&light_table(inputs)));
            gpu.inputs = inputs;
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
        contents: bytemuck::cast_slice(&light_table(inputs)),
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
        inputs,
        atmosphere,
    });
}

fn light_table(inputs: Option<LightmapInputs>) -> [[f32; 4]; 256] {
    inputs.map_or([[1.0; 4]; 256], LightmapInputs::build)
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

#[cfg(test)]
mod fullbright_tests {
    use super::*;

    #[test]
    fn fullbright_lights_every_sample_and_disabling_restores_current_environment() {
        let dark = LightmapInputs {
            sky_darken: 0.0,
            darkness: 1.0,
            ..Default::default()
        };
        let ordinary = dark.build();
        assert!(ordinary[0][0] < 1.0);
        assert_eq!(light_table(None), [[1.0; 4]; 256]);
        assert_eq!(light_table(Some(dark)), ordinary);
        let day = LightmapInputs {
            brightness: 1.0,
            ..Default::default()
        };
        assert_eq!(light_table(Some(day)), day.build());
    }
}
