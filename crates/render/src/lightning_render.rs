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
            BindingType, BlendComponent, BlendFactor, BlendOperation, BlendState, Buffer,
            BufferBindingType, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, FragmentState,
            PipelineCache, RenderPipeline, RenderPipelineDescriptor, ShaderStages, ShaderType,
            Specializer, SpecializerKey, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use crate::lightning::{BoltRecord, LightningScene, MAX_BOLT_RECORDS};

const LIGHTNING_SHADER_HANDLE: Handle<Shader> =
    uuid_handle!("c2a4d7e0-91b3-4f6a-8d15-6e0b7f3a9c42");
const RECORD_BYTES: usize = std::mem::size_of::<BoltRecord>();

pub(crate) fn install_lightning_render(app: &mut App) {
    crate::pipeline_warmup::register::<LightningPipeline>(app);
    load_internal_asset!(
        app,
        LIGHTNING_SHADER_HANDLE,
        "lightning.wgsl",
        crate::shader_safety::from_wgsl
    );
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .init_resource::<LightningPipeline>()
        .add_render_command::<Transparent3d, DrawLightningCommands>()
        .add_systems(RenderStartup, init_lightning_gpu)
        .add_systems(
            Render,
            (
                prepare_lightning_records.in_set(RenderSystems::PrepareResources),
                prepare_lightning_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_lightning
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
struct LightningGpu {
    record_buffer: Buffer,
    record_count: u32,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
}

fn init_lightning_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let record_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("lightning ribbon records"),
        contents: &vec![0_u8; MAX_BOLT_RECORDS * RECORD_BYTES],
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
    });
    commands.insert_resource(LightningGpu {
        record_buffer,
        record_count: 0,
        bind_group: None,
        view_buffer_id: None,
    });
}

fn prepare_lightning_records(
    scene: Res<LightningScene>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<LightningGpu>,
) {
    let count = scene.records.len().min(MAX_BOLT_RECORDS);
    gpu.record_count = u32::try_from(count).expect("bounded bolt record count");
    if count > 0 {
        render_queue.write_buffer(
            &gpu.record_buffer,
            0,
            bytemuck::cast_slice::<BoltRecord, u8>(&scene.records[..count]),
        );
    }
}

struct LightningPipelineSpecializer;

#[derive(Resource)]
struct LightningPipeline {
    variants: Variants<RenderPipeline, LightningPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for LightningPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "lightning bind group layout",
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
                        min_binding_size: BufferSize::new(RECORD_BYTES as u64),
                    },
                    count: None,
                },
            ],
        );
        let additive = BlendState {
            color: BlendComponent {
                src_factor: BlendFactor::One,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
            alpha: BlendComponent {
                src_factor: BlendFactor::Zero,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            },
        };
        let descriptor = RenderPipelineDescriptor {
            label: Some("additive lightning pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: LIGHTNING_SHADER_HANDLE,
                entry_point: Some("lightning_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: LIGHTNING_SHADER_HANDLE,
                entry_point: Some("lightning_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: crate::SCENE_COLOR_FORMAT,
                    blend: Some(additive),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::GreaterEqual),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(LightningPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct LightningPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for LightningPipelineSpecializer {
    type Key = LightningPipelineKey;

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
            crate::SCENE_HDR_FORMAT
        } else {
            crate::SCENE_COLOR_FORMAT
        };
        Ok(key)
    }
}

fn prepare_lightning_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<LightningPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<LightningGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
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
        "lightning bind group",
        &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: gpu.record_buffer.as_entire_binding(),
            },
        ],
    ));
    gpu.view_buffer_id = Some(view_buffer.id());
}

fn queue_lightning(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<LightningPipeline>,
    scene: Res<LightningScene>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    if scene.records.is_empty() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawLightningCommands>();
    for (view_entity, main_entity, view, extracted_camera, msaa) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            LightningPipelineKey {
                msaa: *msaa,
                hdr: extracted_camera.hdr,
            },
        ) else {
            continue;
        };
        for (index, record) in scene.records.iter().take(MAX_BOLT_RECORDS).enumerate() {
            let midpoint = (Vec3::from_array(record.start) + Vec3::from_array(record.end)) * 0.5;
            crate::transparent_phase::add(
                phase,
                Transparent3d {
                    sorting_info:
                        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                    entity: (view_entity, *main_entity),
                    pipeline: pipeline_id,
                    draw_function,
                    distance: view.rangefinder3d().distance(&midpoint),
                    batch_range: index as u32..index as u32 + 1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                },
            );
        }
    }
}

type DrawLightningCommands = (SetItemPipeline, SetLightningBindGroup<0>, DrawLightning);

struct SetLightningBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetLightningBindGroup<I> {
    type Param = SRes<LightningGpu>;
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

struct DrawLightning;

impl<P: PhaseItem> RenderCommand<P> for DrawLightning {
    type Param = SRes<LightningGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let range = item.batch_range();
        let count = gpu.into_inner().record_count;
        pass.draw(range.start.min(count) * 6..range.end.min(count) * 6, 0..1);
        RenderCommandResult::Success
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for LightningPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.push(self.variants.specialize(
            cache,
            LightningPipelineKey {
                msaa: view.msaa,
                hdr: view.hdr,
            },
        )?);
        Ok(())
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::queue_review_support as fixture;
    use bevy::ecs::system::RunSystemOnce;
    #[test]
    fn review_render_lightning_queue_uses_current_records() {
        let (mut app, view) = fixture::app();
        app.init_resource::<LightningScene>()
            .init_resource::<LightningPipeline>()
            .add_render_command::<Transparent3d, DrawLightningCommands>();
        app.world_mut().run_system_once(init_lightning_gpu).unwrap();
        app.world_mut()
            .resource_mut::<LightningScene>()
            .records
            .push(BoltRecord {
                start: [0.0; 3],
                end: [0.0, 1.0, 0.0],
                half_width: 0.1,
                intensity: 1.0,
            });
        app.world_mut().run_system_once(queue_lightning).unwrap();
        assert_eq!(fixture::items(&app, view).len(), 1);
        fixture::clear(&mut app, view);
        app.world_mut().resource_mut::<LightningGpu>().record_count = 1;
        app.world_mut()
            .resource_mut::<LightningScene>()
            .records
            .clear();
        app.world_mut().run_system_once(queue_lightning).unwrap();
        assert!(fixture::items(&app, view).is_empty());
    }
    #[test]
    fn review_render_lightning_ribbons_sort_from_their_geometry() {
        let (mut app, view) = fixture::app();
        app.init_resource::<LightningScene>()
            .init_resource::<LightningPipeline>()
            .add_render_command::<Transparent3d, DrawLightningCommands>();
        app.world_mut().run_system_once(init_lightning_gpu).unwrap();
        app.world_mut().resource_mut::<LightningGpu>().record_count = 2;
        app.world_mut().resource_mut::<LightningScene>().records = [-2.0, -20.0]
            .map(|z| BoltRecord {
                start: [0.0, 0.0, z],
                end: [0.0, 1.0, z],
                half_width: 0.1,
                intensity: 1.0,
            })
            .to_vec();
        app.world_mut().run_system_once(queue_lightning).unwrap();
        let items = fixture::items(&app, view);
        assert_eq!(items.len(), 2);
        assert_ne!(items[0].distance, items[1].distance);
        assert_eq!(items[0].batch_range, 0..1);
        assert_eq!(items[1].batch_range, 1..2);
    }
}
