use assets::AtmosphereRole;
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        render_phase::{
            AddRenderCommand, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, BlendState, Buffer, BufferBindingType, BufferId, BufferInitDescriptor,
            BufferSize, BufferUsages, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, ShaderStages, ShaderType, Specializer, SpecializerKey,
            TextureFormat, Variants, VertexState,
        },
        renderer::RenderDevice,
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use crate::{
    AtmosphereFrame, AtmosphereTextureAssets, PackedCloudQuad, atmosphere_render::AtmosphereGpu,
    mesh_cloud_texture,
};
use meshing::{CLOUD_MASK_SIZE, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, cloud_instance_origins};

const CLOUD_SHADER_HANDLE: Handle<Shader> = uuid_handle!("8dcfe9d0-c182-44cc-ae4c-7e5233b68659");
pub(crate) fn install_cloud_render(app: &mut App) {
    load_internal_asset!(app, CLOUD_SHADER_HANDLE, "cloud.wgsl", Shader::from_wgsl);
    app.sub_app_mut(RenderApp)
        .init_resource::<CloudPipeline>()
        .add_render_command::<Transparent3d, DrawCloudCommands>()
        .add_systems(RenderStartup, init_cloud_gpu)
        .add_systems(
            Render,
            (
                prepare_cloud_records.in_set(RenderSystems::PrepareResources),
                prepare_cloud_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_clouds.in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct CloudGpu {
    pub(crate) record_buffer: Option<Buffer>,
    pub(crate) record_count: u32,
    prepared_identity: Option<[u8; 32]>,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
    atmosphere_buffer_id: Option<BufferId>,
    bound_asset_identity: Option<[u8; 32]>,
    #[cfg(test)]
    pub(crate) upload_count: u32,
}

fn init_cloud_gpu(mut commands: Commands) {
    commands.insert_resource(CloudGpu {
        record_buffer: None,
        record_count: 0,
        prepared_identity: None,
        bind_group: None,
        view_buffer_id: None,
        atmosphere_buffer_id: None,
        bound_asset_identity: None,
        #[cfg(test)]
        upload_count: 0,
    });
}

pub(crate) fn prepare_cloud_records(
    requested: Res<AtmosphereTextureAssets>,
    render_device: Res<RenderDevice>,
    mut gpu: ResMut<CloudGpu>,
) {
    let Some(runtime) = requested.runtime() else {
        clear_cloud_gpu(&mut gpu);
        return;
    };
    let identity = requested.identity();
    if gpu.prepared_identity == Some(identity) {
        return;
    }

    let cloud_texture = runtime
        .texture(AtmosphereRole::Clouds)
        .expect("validated MCBEATM2 always contains the cloud texture");
    let records = mesh_cloud_texture(cloud_texture)
        .expect("validated MCBEATM2 cloud texture must satisfy the finite mesh contract");
    let record_count = u32::try_from(records.len()).expect("bounded cloud record count fits u32");
    let record_buffer = if records.is_empty() {
        None
    } else {
        Some(
            render_device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("immutable finite cloud quad records"),
                contents: bytemuck::cast_slice::<PackedCloudQuad, u8>(&records),
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            }),
        )
    };

    gpu.record_buffer = record_buffer;
    gpu.record_count = record_count;
    gpu.prepared_identity = Some(identity);
    gpu.bind_group = None;
    gpu.view_buffer_id = None;
    gpu.atmosphere_buffer_id = None;
    gpu.bound_asset_identity = None;
    #[cfg(test)]
    {
        gpu.upload_count += 1;
    }
}

fn clear_cloud_gpu(gpu: &mut CloudGpu) {
    gpu.record_buffer = None;
    gpu.record_count = 0;
    gpu.prepared_identity = None;
    gpu.bind_group = None;
    gpu.view_buffer_id = None;
    gpu.atmosphere_buffer_id = None;
    gpu.bound_asset_identity = None;
}

struct CloudPipelineSpecializer;

#[derive(Resource)]
struct CloudPipeline {
    variants: Variants<RenderPipeline, CloudPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for CloudPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "finite cloud bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(AtmosphereFrame::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(8),
                    },
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("finite depth-aware cloud pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: CLOUD_SHADER_HANDLE,
                entry_point: Some("cloud_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: CLOUD_SHADER_HANDLE,
                entry_point: Some("cloud_fragment".into()),
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
                depth_compare: CompareFunction::GreaterEqual,
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(CloudPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct CloudPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for CloudPipelineSpecializer {
    type Key = CloudPipelineKey;

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

fn prepare_cloud_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<CloudPipeline>,
    view_uniforms: Res<ViewUniforms>,
    atmosphere: Res<AtmosphereGpu>,
    mut gpu: ResMut<CloudGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        return;
    };
    let Some(record_buffer) = gpu.record_buffer.as_ref() else {
        gpu.bind_group = None;
        return;
    };
    let Some(identity) = gpu.prepared_identity else {
        gpu.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.bind_group.is_some()
        && gpu.view_buffer_id == Some(view_buffer.id())
        && gpu.atmosphere_buffer_id == Some(atmosphere.buffer.id())
        && gpu.bound_asset_identity == Some(identity)
    {
        return;
    }

    gpu.bind_group = Some(render_device.create_bind_group(
        "finite cloud bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: atmosphere.buffer.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 2,
                resource: record_buffer.as_entire_binding(),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
    gpu.atmosphere_buffer_id = Some(atmosphere.buffer.id());
    gpu.bound_asset_identity = Some(identity);
}

fn queue_clouds(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<CloudPipeline>,
    gpu: Res<CloudGpu>,
    atmosphere: Res<AtmosphereFrame>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(Entity, &MainEntity, &ExtractedView, &Msaa)>,
) {
    if gpu.record_count == 0 {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawCloudCommands>();
    for (view_entity, main_entity, view, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            CloudPipelineKey {
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
            distance: cloud_phase_distance(view, &atmosphere),
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        });
    }
}

fn cloud_phase_distance(view: &ExtractedView, atmosphere: &AtmosphereFrame) -> f32 {
    let camera = view.world_from_view.translation();
    let offset_blocks =
        f64::from(atmosphere.cloud_texture_offset()[0]) * f64::from(CLOUD_MASK_SIZE);
    let cloud_center = Vec3::from_array(cloud_bounds_center(
        [f64::from(camera.x), f64::from(camera.z)],
        offset_blocks,
    ));
    view.rangefinder3d().distance(&cloud_center)
}

fn cloud_bounds_center(camera_xz: [f64; 2], offset_blocks: f64) -> [f32; 3] {
    let center_origin = cloud_instance_origins(camera_xz, offset_blocks)[4];
    let half_period = CLOUD_MASK_SIZE as f32 * 0.5;
    [
        center_origin[0] + half_period,
        (CLOUD_UNDERSIDE_Y + CLOUD_TOP_Y) * 0.5,
        center_origin[1] + half_period,
    ]
}

type DrawCloudCommands = (SetItemPipeline, SetCloudBindGroup<0>, DrawClouds);

struct SetCloudBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetCloudBindGroup<I> {
    type Param = SRes<CloudGpu>;
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

struct DrawClouds;

impl<P: PhaseItem> RenderCommand<P> for DrawClouds {
    type Param = SRes<CloudGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let vertex_count = gpu.record_count.checked_mul(6).expect("bounded cloud draw");
        pass.draw(0..vertex_count, 0..9);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
mod tests {
    use super::cloud_bounds_center;

    #[test]
    fn cloud_bounds_center_is_symmetric_across_negative_period_boundaries() {
        assert_eq!(cloud_bounds_center([0.0, 0.0], 0.0), [128.0, 130.0, 128.0]);
        assert_eq!(
            cloud_bounds_center([-0.001, -0.001], 0.0),
            [-128.0, 130.0, -128.0]
        );
        assert_eq!(
            cloud_bounds_center([-256.0, -256.0], 0.0),
            [-128.0, 130.0, -128.0]
        );
        assert_eq!(
            cloud_bounds_center([-256.001, -256.001], 0.0),
            [-384.0, 130.0, -384.0]
        );
    }

    #[test]
    fn cloud_bounds_center_preserves_wrapped_scroll_at_period_crossings() {
        assert_eq!(
            cloud_bounds_center([1.25, 0.0], 257.25),
            [129.25, 130.0, 128.0]
        );
        assert_eq!(
            cloud_bounds_center([1.249, 0.0], 257.25),
            [-126.75, 130.0, 128.0]
        );
    }
}
