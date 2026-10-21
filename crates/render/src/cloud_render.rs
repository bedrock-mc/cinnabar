use bevy::prelude::IntoScheduleConfigs;
#[cfg(test)]
use bevy::prelude::System;
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_resource::ShaderType;
#[cfg(test)]
use bevy::render::renderer::RenderQueue;
use render_scene::CloudVisibility;
use std::collections::HashMap;

#[cfg(test)]
use bevy::prelude::{GlobalTransform, IntoSystem, Mat4, UVec4};
use bevy::{
    asset::{load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    prelude::{
        App, BevyError, Camera3d, Commands, Entity, FromWorld, Handle, Msaa, Query, Res, ResMut,
        Resource, Result, Shader, Vec3, With, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand, RenderCommandResult,
            SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingType, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferInitDescriptor, BufferSize, BufferUsages, Canonical, ColorTargetState,
            ColorWrites, CompareFunction, DepthStencilState, Face, FragmentState, FrontFace,
            PipelineCache, PrimitiveState, RenderPipeline, RenderPipelineDescriptor, ShaderStages,
            Specializer, SpecializerKey, Variants, VertexState,
        },
        renderer::RenderDevice,
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use meshing::{
    CLOUD_CELL_BLOCKS, CLOUD_TOP_Y, CLOUD_UNDERSIDE_Y, CLOUD_WORLD_PERIOD,
    cloud_viewport::{CloudViewport, ViewportCloudQuad},
};

mod mesh_cache;
use mesh_cache::CloudMeshCache;
use {
    crate::atmosphere_render::AtmosphereGpu,
    render_scene::{
        AtmosphereFrame, AtmosphereTextureAssets, CloudGeometryDiagnostic, CloudRenderConfig,
    },
};

const CLOUD_SHADER_HANDLE: Handle<Shader> = uuid_handle!("8dcfe9d0-c182-44cc-ae4c-7e5233b68659");
pub(crate) fn install_cloud_render(app: &mut App) {
    crate::upload_staging::install(app);
    crate::pipeline_warmup::register::<CloudPipeline>(app);
    app.init_resource::<CloudVisibility>()
        .add_plugins(ExtractResourcePlugin::<CloudVisibility>::default());
    load_internal_asset!(
        app,
        CLOUD_SHADER_HANDLE,
        "cloud.wesl",
        |source: &str, path| crate::shader_safety::from_wesl(
            meshing::cloud_viewport::shader_source(source),
            path,
        )
    );
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .init_resource::<CloudPipeline>()
        .add_render_command::<Transparent3d, DrawCloudCommands>()
        .add_systems(RenderStartup, init_cloud_gpu)
        .add_systems(
            Render,
            (
                // Queue captures this admitted immutable window. Publish before
                // Queue, not later in PrepareResources while a queued item can
                // still refer to the previous window's bounds.
                prepare_cloud_records
                    .run_if(render_scene::panorama::world_passes_enabled)
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue),
                prepare_cloud_colour.in_set(RenderSystems::PrepareResources),
                prepare_cloud_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_clouds
                    .run_if(render_scene::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
pub(crate) struct CloudGpu {
    pub(crate) views: HashMap<Entity, CloudViewGpu>,
    colour_buffer: Buffer,
    colour: [f32; 8],
    diagnostics: bool,
    work: CloudWork,
    #[cfg(test)]
    colour_uploads: u64,
    #[cfg(test)]
    pub(crate) upload_count: u32,
}

#[derive(Default)]
struct CloudWork {
    rebuilds: u64,
    waited: u64,
    buffer_creations: u64,
    uploaded_bytes: u64,
}

pub(crate) struct CloudViewGpu {
    pub(crate) record_buffer: Option<Buffer>,
    pub(crate) record_count: u32,
    pub(crate) geometry_diagnostic: Option<CloudGeometryDiagnostic>,
    prepared_identity: Option<[u8; 32]>,
    mesh_cache: CloudMeshCache,
    viewport: CloudViewport,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
    atmosphere_buffer_id: Option<BufferId>,
    bound_asset_identity: Option<[u8; 32]>,
}

fn init_cloud_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    commands.insert_resource(CloudGpu {
        views: HashMap::new(),
        colour_buffer: render_device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("native cloud gamma RGBA uniform"),
            contents: bytemuck::cast_slice(&[0.0_f32; 8]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        }),
        colour: [0.0; 8],
        diagnostics: env_compat::var_os("CINNABAR_CLOUD_DIAGNOSTICS")
            .is_some_and(|value| value == "1"),
        work: CloudWork::default(),
        #[cfg(test)]
        colour_uploads: 0,
        #[cfg(test)]
        upload_count: 0,
    });
}

pub(crate) fn prepare_cloud_records(
    requested: Res<AtmosphereTextureAssets>,
    atmosphere: Res<AtmosphereFrame>,
    uploads: crate::upload_staging::FrameUploads,
    visibility: Res<CloudVisibility>,
    views: Query<(Entity, &ExtractedView), With<Camera3d>>,
    mut gpu: ResMut<CloudGpu>,
) {
    if !visibility.0 || !atmosphere.sky_kind().has_clouds() {
        gpu.views.clear();
        return;
    }
    let Some(runtime) = requested.runtime() else {
        gpu.views.clear();
        return;
    };
    let identity = requested.identity();
    gpu.views.retain(|entity, _| views.contains(*entity));
    let config = CloudRenderConfig::legacy_fancy();
    let CloudGpu {
        views: prepared_views,
        diagnostics,
        work,
        ..
    } = &mut *gpu;
    #[cfg(test)]
    let mut uploaded_views = 0;
    for (entity, view) in &views {
        let camera = view.world_from_view.translation();
        let above = camera.y >= CLOUD_UNDERSIDE_Y.ceil();
        let Some(viewport) = CloudViewport::try_new(
            [
                f64::from(camera.x) + f64::from(atmosphere.cloud_scroll_blocks()),
                f64::from(camera.z),
            ],
            config.mesh_size(),
            config.grid_size(),
            above,
            false,
        ) else {
            // A well-formed non-finite/out-of-range camera never feeds shader
            // storage indexing. Do not retain that view's old geometry.
            prepared_views.remove(&entity);
            continue;
        };
        let prepared = prepared_views
            .entry(entity)
            .or_insert_with(|| CloudViewGpu {
                record_buffer: None,
                record_count: 0,
                geometry_diagnostic: None,
                prepared_identity: None,
                mesh_cache: CloudMeshCache::default(),
                viewport,
                bind_group: None,
                view_buffer_id: None,
                atmosphere_buffer_id: None,
                bound_asset_identity: None,
            });
        prepared.mesh_cache.set_identity(identity);
        if prepared.prepared_identity != Some(identity) || prepared.viewport.needs_rebuild(viewport)
        {
            let span = bevy::log::info_span!(
                "cloud.admit",
                rebuilds = work.rebuilds + 1,
                waited = work.waited,
                buffer_creations = work.buffer_creations,
                uploaded_bytes = work.uploaded_bytes
            );
            let _guard = span.enter();
            let Some((mesh, waited)) =
                prepared
                    .mesh_cache
                    .get(runtime, viewport, above, *diagnostics)
            else {
                prepared.record_count = 0;
                prepared.prepared_identity = None;
                prepared.geometry_diagnostic = None;
                prepared.bind_group = None;
                prepared
                    .mesh_cache
                    .prefetch(runtime, viewport, *diagnostics);
                continue;
            };
            let records = &mesh.records;
            let bytes = bytemuck::cast_slice::<ViewportCloudQuad, u8>(records);
            if !bytes.is_empty() {
                if prepared
                    .record_buffer
                    .as_ref()
                    .is_none_or(|buffer| buffer.size() < bytes.len() as u64)
                {
                    prepared.record_buffer =
                        Some(uploads.device.create_buffer(&BufferDescriptor {
                            label: Some("retained viewport cloud quad records"),
                            size: (bytes.len() as u64).next_power_of_two(),
                            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                            mapped_at_creation: false,
                        }));
                    prepared.bind_group = None;
                    work.buffer_creations += 1;
                }
                uploads.write(prepared.record_buffer.as_ref().unwrap(), 0, bytes);
            }
            prepared.record_count =
                u32::try_from(records.len()).expect("bounded cloud records fit u32");
            prepared.geometry_diagnostic = mesh.diagnostic.clone();
            if let Some(diagnostic) = &prepared.geometry_diagnostic {
                bevy::log::info!("CLOUD_GEOMETRY_EVIDENCE {}", diagnostic.marker_fields());
            }
            prepared.prepared_identity = Some(identity);
            prepared.viewport = viewport;
            work.rebuilds += 1;
            work.waited += u64::from(waited);
            work.uploaded_bytes += bytes.len() as u64;
            span.record("waited", work.waited);
            span.record("buffer_creations", work.buffer_creations);
            span.record("uploaded_bytes", work.uploaded_bytes);
            #[cfg(test)]
            {
                uploaded_views += 1;
            }
        }
        prepared
            .mesh_cache
            .prefetch(runtime, viewport, *diagnostics);
    }
    #[cfg(test)]
    {
        gpu.upload_count += uploaded_views;
    }
}

fn prepare_cloud_colour(
    atmosphere: Res<AtmosphereFrame>,
    view: Res<render_scene::AtmosphereViewInputs>,
    mut gpu: ResMut<CloudGpu>,
    uploads: crate::upload_staging::FrameUploads,
) {
    let colour = atmosphere.cloud_colour_for_view(*view);
    let native = [
        colour[0],
        colour[1],
        colour[2],
        colour[3],
        CLOUD_CELL_BLOCKS,
        CLOUD_UNDERSIDE_Y,
        CLOUD_TOP_Y,
        CLOUD_WORLD_PERIOD,
    ];
    if gpu.colour == native {
        return;
    }
    uploads.write(&gpu.colour_buffer, 0, bytemuck::cast_slice(&native));
    gpu.colour = native;
    #[cfg(test)]
    {
        gpu.colour_uploads += 1;
    }
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
                        min_binding_size: BufferSize::new(size_of::<ViewportCloudQuad>() as u64),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::VERTEX,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(size_of::<[f32; 8]>() as u64),
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
            // Vanilla's cloud material culls clockwise faces:
            // preserve the outward counter-clockwise texel faces.
            primitive: PrimitiveState {
                front_face: FrontFace::Ccw,
                cull_mode: Some(Face::Back),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: CLOUD_SHADER_HANDLE,
                entry_point: Some("cloud_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::RED | ColorWrites::GREEN | ColorWrites::BLUE,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                // Native comparison2 translates to LESS; Bevy reverses Z.
                depth_compare: Some(CompareFunction::Greater),
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
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
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
        for prepared in gpu.views.values_mut() {
            prepared.bind_group = None;
        }
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    let CloudGpu {
        views,
        colour_buffer,
        ..
    } = &mut *gpu;
    for prepared in views.values_mut() {
        let Some(record_buffer) = prepared.record_buffer.as_ref() else {
            prepared.bind_group = None;
            continue;
        };
        let identity = prepared.prepared_identity;
        if prepared.bind_group.is_some()
            && prepared.view_buffer_id == Some(view_buffer.id())
            && prepared.atmosphere_buffer_id == Some(atmosphere.buffer.id())
            && prepared.bound_asset_identity == identity
        {
            continue;
        }
        prepared.bind_group = Some(render_device.create_bind_group(
            "finite native cloud window bind group",
            &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: view_binding.clone(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: atmosphere.buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: record_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: colour_buffer.as_entire_binding(),
                },
            ],
        ));
        prepared.view_buffer_id = Some(view_buffer.id());
        prepared.atmosphere_buffer_id = Some(atmosphere.buffer.id());
        prepared.bound_asset_identity = identity;
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "a queue system's render-world inputs"
)]
fn queue_clouds(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<CloudPipeline>,
    gpu: Res<CloudGpu>,
    (atmosphere, visibility): (Res<AtmosphereFrame>, Res<CloudVisibility>),
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
    pack: render_scene::shaderpack::ShaderPackDrawnEffects,
) {
    if !visibility.0 || !atmosphere.sky_kind().has_clouds() {
        return;
    }
    let draw_function = draw_functions.read().id::<DrawCloudCommands>();
    for (view_entity, main_entity, view, extracted_camera, msaa) in &views {
        if pack.replaces(view_entity, |drawn| drawn.clouds) {
            continue;
        }
        let Some(prepared) = gpu.views.get(&view_entity) else {
            continue;
        };
        if prepared.record_count == 0 {
            continue;
        }
        if let Some(diagnostic) = &prepared.geometry_diagnostic {
            debug_assert_eq!(diagnostic.quad_count(), prepared.record_count);
        }
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            &pipeline_cache,
            CloudPipelineKey {
                msaa: *msaa,
                hdr: extracted_camera.hdr,
            },
        ) else {
            continue;
        };
        crate::transparent_phase::add(
            phase,
            Transparent3d {
                sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                entity: (view_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function,
                distance: cloud_phase_distance(view, prepared.viewport, &atmosphere),
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            },
        );
    }
}

fn cloud_phase_distance(
    view: &ExtractedView,
    viewport: CloudViewport,
    atmosphere: &AtmosphereFrame,
) -> f32 {
    let cloud_center = Vec3::from_array(cloud_bounds_center(
        viewport,
        atmosphere.cloud_scroll_blocks(),
    ));
    view.rangefinder3d().distance(&cloud_center)
}

fn cloud_bounds_center(viewport: CloudViewport, scroll_blocks: f32) -> [f32; 3] {
    let center = viewport.centre();
    [
        center[0] as f32 - scroll_blocks,
        (CLOUD_UNDERSIDE_Y + CLOUD_TOP_Y) * 0.5,
        center[1] as f32,
    ]
}

type DrawCloudCommands = crate::gpu_timing::GpuDrawSpan<
    { render_scene::RuntimeStage::GpuSky as usize },
    (SetItemPipeline, SetCloudBindGroup<0>, DrawClouds),
>;

struct SetCloudBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetCloudBindGroup<I> {
    type Param = SRes<CloudGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(bind_group) = gpu
            .into_inner()
            .views
            .get(&item.entity())
            .and_then(|prepared| prepared.bind_group.as_ref())
        else {
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
        item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(prepared) = gpu.into_inner().views.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };
        let vertex_count = prepared
            .record_count
            .checked_mul(6)
            .expect("bounded cloud draw");
        pass.draw(0..vertex_count, 0..1);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
#[path = "cloud_render/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "cloud_pipeline_tests.rs"]
mod pipeline_tests;

impl crate::pipeline_warmup::PrewarmPipelines for CloudPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        ids.push(self.variants.specialize(
            cache,
            CloudPipelineKey {
                msaa: view.msaa,
                hdr: view.hdr,
            },
        )?);
        Ok(())
    }
}
