//! Atlas-backed model, portal and overlay passes with separate vertex lists.
//! Models and portal planes draw in the opaque phase; overlays draw afterward.

use std::mem::size_of;

#[cfg(test)]
use bevy::prelude::{IntoSystem, System};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{
        CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey, Transparent3d,
    },
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    prelude::{
        App, BevyError, Commands, Entity, FromWorld, Handle, IntoScheduleConfigs, Msaa, Plugin,
        Query, Res, ResMut, Resource, Result, Shader, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            AddRenderCommand, BinnedRenderPhaseType, DrawFunctionId, DrawFunctions,
            InputUniformIndex, PhaseItem, PhaseItemExtraIndex, RenderCommand, RenderCommandResult,
            SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferSize, BufferUsages, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, Origin3d, PipelineCache,
            PrimitiveTopology, RenderPipeline, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, Specializer,
            SpecializerKey, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
            TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
            TextureView, TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use super::{
    mesh::{BLOCK_ENTITY_VERTEX_WORDS, BlockEntityVertex},
    scene::{BlockEntityFrame, BlockEntityScene},
    selection::{BLOCK_SELECTION_VERTICES_PER_EDGE, BlockSelectionFrame},
};

#[path = "gpu/pipeline.rs"]
mod pipeline;
use pipeline::{BlockEntityPipeline, BlockEntityPipelineKey, PipelineMode};

const SHADER_HANDLE: Handle<Shader> = uuid_handle!("6f0c1c1e-3b6d-4a7e-9b1e-2f4f8a1d5c33");
const VERTEX_BYTES: u64 = (BLOCK_ENTITY_VERTEX_WORDS * size_of::<f32>()) as u64;
const MIN_BUFFER_VERTICES: u64 = 1024;
const PORTAL_PARAMETER_BYTES: u64 = size_of::<[[f32; 4]; 4]>() as u64;

#[derive(Debug, Clone, Copy, Default)]
pub struct BlockEntityRenderPlugin;

impl Plugin for BlockEntityRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }

    fn finish(&self, app: &mut App) {
        install(app);
    }
}

#[derive(Resource)]
struct BlockEntityRenderInstalled;

fn install(app: &mut App) {
    app.init_resource::<BlockEntityFrame>()
        .init_resource::<BlockSelectionFrame>()
        .init_resource::<BlockEntityScene>()
        .init_resource::<super::items::StaticItemPlacements>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<BlockEntityRenderInstalled>()
    {
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<BlockEntityFrame>::default());
    app.add_plugins(ExtractResourcePlugin::<BlockSelectionFrame>::default());
    crate::lighting::install(app);
    load_internal_asset!(app, SHADER_HANDLE, "block_entity.wesl", |source, path| {
        crate::shader_safety::from_block_entity_wesl(
            source,
            path,
            BLOCK_ENTITY_VERTEX_WORDS,
            BLOCK_SELECTION_VERTICES_PER_EDGE,
        )
    });
    crate::pipeline_warmup::register::<BlockEntityPipeline>(app);
    crate::install_opaque_phase_reset(app.sub_app_mut(RenderApp));
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .insert_resource(BlockEntityRenderInstalled)
        .init_resource::<BlockEntityPipeline>()
        .add_render_command::<Opaque3d, DrawSolidCommands>()
        .add_render_command::<Opaque3d, DrawPortalCommands>()
        .add_render_command::<Transparent3d, DrawOverlayCommands>()
        .add_render_command::<Transparent3d, DrawOutlineCommands>()
        .add_render_command::<Transparent3d, DrawCrackCommands>()
        .add_render_command::<Transparent3d, DrawAdditiveCommands>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                prepare_resources.in_set(RenderSystems::PrepareResources),
                prepare_bind_groups.in_set(RenderSystems::PrepareBindGroups),
                (
                    queue_solid,
                    queue_overlay,
                    queue_outline,
                    queue_crack,
                    queue_additive,
                )
                    .distributive_run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

/// One storage buffer of vertices with its live count and bind group.
struct VertexList {
    buffer: Option<Buffer>,
    capacity_vertices: u64,
    count: u32,
    bind_group: Option<BindGroup>,
}

impl VertexList {
    const fn new() -> Self {
        Self {
            buffer: None,
            capacity_vertices: 0,
            count: 0,
            bind_group: None,
        }
    }

    fn upload(
        &mut self,
        vertices: &[BlockEntityVertex],
        render_device: &RenderDevice,
        render_queue: &RenderQueue,
        label: &'static str,
    ) {
        self.count = u32::try_from(vertices.len()).unwrap_or(0);
        if vertices.is_empty() {
            return;
        }
        let needed = vertices.len() as u64;
        if self.buffer.is_none() || self.capacity_vertices < needed {
            let capacity = needed.next_power_of_two().max(MIN_BUFFER_VERTICES);
            self.buffer = Some(render_device.create_buffer(&BufferDescriptor {
                label: Some(label),
                size: capacity * VERTEX_BYTES,
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
            self.capacity_vertices = capacity;
            self.bind_group = None;
        }
        if let Some(buffer) = &self.buffer {
            render_queue.write_buffer(buffer, 0, bytemuck::cast_slice(vertices));
        }
    }
}

#[derive(Resource)]
struct BlockEntityGpu {
    solid: VertexList,
    overlay: VertexList,
    outline: VertexList,
    crack: VertexList,
    portal: VertexList,
    additive: VertexList,
    portal_uniform: Buffer,
    portal_parameters: Option<[[f32; 4]; 4]>,
    #[cfg(test)]
    portal_uploads: u64,
    texture: Option<Texture>,
    view: Option<TextureView>,
    atlas_identity: [u8; 32],
    atlas_size: [u32; 2],
    dynamic_revision: u64,
    sampler: Sampler,
    view_buffer_id: Option<BufferId>,
    frame_revision: u64,
    selection_revision: u64,
}

fn init_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    commands.insert_resource(BlockEntityGpu {
        solid: VertexList::new(),
        overlay: VertexList::new(),
        outline: VertexList::new(),
        crack: VertexList::new(),
        portal: VertexList::new(),
        additive: VertexList::new(),
        portal_uniform: render_device.create_buffer(&BufferDescriptor {
            label: Some("portal projector parameters"),
            size: PORTAL_PARAMETER_BYTES,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        portal_parameters: None,
        #[cfg(test)]
        portal_uploads: 0,
        texture: None,
        view: None,
        atlas_identity: [0; 32],
        atlas_size: [0; 2],
        dynamic_revision: u64::MAX,
        sampler: render_device.create_sampler(&SamplerDescriptor {
            label: Some("block-entity nearest atlas sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..default()
        }),
        view_buffer_id: None,
        frame_revision: u64::MAX,
        selection_revision: u64::MAX,
    });
}

fn prepare_resources(
    frame: Res<BlockEntityFrame>,
    selection: Res<BlockSelectionFrame>,
    atmosphere: Option<Res<crate::AtmosphereFrame>>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<BlockEntityGpu>,
) {
    let atmosphere = atmosphere.as_deref().copied().unwrap_or_default();
    let [red, green, blue] = atmosphere.fog_color();
    let portal_parameters = [
        frame.portal_star_rect,
        [frame.portal_time_seconds, 0.0, 0.0, 0.0],
        [red, green, blue, atmosphere.fog_start()],
        [atmosphere.fog_end(), 0.0, 0.0, 0.0],
    ];
    if !frame.portal.is_empty() && gpu.portal_parameters != Some(portal_parameters) {
        render_queue.write_buffer(
            &gpu.portal_uniform,
            0,
            bytemuck::cast_slice(&portal_parameters),
        );
        gpu.portal_parameters = Some(portal_parameters);
        #[cfg(test)]
        {
            gpu.portal_uploads += 1;
        }
    }
    let atlas = frame
        .atlas
        .as_ref()
        .unwrap_or_else(|| super::selection::fallback_atlas());
    if gpu.atlas_identity != atlas.identity || gpu.atlas_size != atlas.size || gpu.texture.is_none()
    {
        let texture = render_device.create_texture(&TextureDescriptor {
            label: Some("block-entity atlas"),
            size: Extent3d {
                width: atlas.size[0],
                height: atlas.size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });
        write_rows(
            &render_queue,
            &texture,
            0,
            atlas.size[0],
            atlas.static_height,
            &atlas.static_rgba8,
        );
        gpu.view = Some(texture.create_view(&TextureViewDescriptor::default()));
        gpu.texture = Some(texture);
        gpu.atlas_identity = atlas.identity;
        gpu.atlas_size = atlas.size;
        gpu.dynamic_revision = u64::MAX;
        gpu.solid.bind_group = None;
        gpu.overlay.bind_group = None;
        gpu.outline.bind_group = None;
        gpu.crack.bind_group = None;
        gpu.portal.bind_group = None;
        gpu.additive.bind_group = None;
    }
    let dynamic_rows = atlas.size[1].saturating_sub(atlas.static_height);
    if gpu.dynamic_revision != frame.dynamic_revision
        && frame.dynamic_rgba8.len() == atlas.size[0] as usize * dynamic_rows as usize * 4
    {
        if let Some(texture) = &gpu.texture {
            write_rows(
                &render_queue,
                texture,
                atlas.static_height,
                atlas.size[0],
                dynamic_rows,
                &frame.dynamic_rgba8,
            );
        }
        gpu.dynamic_revision = frame.dynamic_revision;
    }
    if gpu.frame_revision != frame.revision {
        gpu.solid.upload(
            &frame.solid,
            &render_device,
            &render_queue,
            "block-entity solid vertices",
        );
        gpu.portal.upload(
            &frame.portal,
            &render_device,
            &render_queue,
            "portal encoded plane vertices",
        );
        gpu.additive.upload(
            &frame.additive,
            &render_device,
            &render_queue,
            "dragon death additive vertices",
        );
    }
    if gpu.frame_revision != frame.revision {
        gpu.overlay.upload(
            &frame.overlay,
            &render_device,
            &render_queue,
            "block-entity overlay vertices",
        );
    }
    if gpu.selection_revision != selection.revision {
        gpu.outline.upload(
            &selection.outline,
            &render_device,
            &render_queue,
            "block selection line vertices",
        );
    }
    if gpu.frame_revision != frame.revision || gpu.selection_revision != selection.revision {
        let crack = frame
            .crack
            .iter()
            .chain(selection.highlight.iter())
            .copied()
            .collect::<Vec<_>>();
        gpu.crack.upload(
            &crack,
            &render_device,
            &render_queue,
            "block-entity crack vertices",
        );
        gpu.frame_revision = frame.revision;
        gpu.selection_revision = selection.revision;
    }
}

fn write_rows(
    render_queue: &RenderQueue,
    texture: &Texture,
    first_row: u32,
    width: u32,
    rows: u32,
    rgba8: &[u8],
) {
    if rows == 0 {
        return;
    }
    render_queue.write_texture(
        TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: Origin3d {
                x: 0,
                y: first_row,
                z: 0,
            },
            aspect: default(),
        },
        rgba8,
        TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(rows),
        },
        Extent3d {
            width,
            height: rows,
            depth_or_array_layers: 1,
        },
    );
}

fn prepare_bind_groups(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<BlockEntityPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<BlockEntityGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.solid.bind_group = None;
        gpu.overlay.bind_group = None;
        gpu.outline.bind_group = None;
        gpu.crack.bind_group = None;
        gpu.portal.bind_group = None;
        gpu.additive.bind_group = None;
        return;
    };
    let view_buffer = view_uniforms
        .uniforms
        .buffer()
        .expect("a dynamic view binding always owns a GPU buffer");
    if gpu.view_buffer_id != Some(view_buffer.id()) {
        gpu.solid.bind_group = None;
        gpu.overlay.bind_group = None;
        gpu.outline.bind_group = None;
        gpu.crack.bind_group = None;
        gpu.portal.bind_group = None;
        gpu.additive.bind_group = None;
        gpu.view_buffer_id = Some(view_buffer.id());
    }
    let BlockEntityGpu {
        solid,
        overlay,
        outline,
        crack,
        portal,
        additive,
        portal_uniform,
        view,
        sampler,
        ..
    } = &mut *gpu;
    let Some(view) = view.as_ref() else {
        solid.bind_group = None;
        overlay.bind_group = None;
        outline.bind_group = None;
        crack.bind_group = None;
        portal.bind_group = None;
        additive.bind_group = None;
        return;
    };
    let layout = pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout);
    for (list, label) in [
        (solid, "block-entity solid bind group"),
        (overlay, "block-entity overlay bind group"),
        (outline, "block selection line bind group"),
        (crack, "block-entity crack bind group"),
        (portal, "portal bind group"),
        (additive, "dragon death additive bind group"),
    ] {
        let (Some(buffer), None) = (list.buffer.as_ref(), list.bind_group.as_ref()) else {
            continue;
        };
        list.bind_group = Some(render_device.create_bind_group(
            label,
            &layout,
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: view_binding.clone(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(sampler),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: portal_uniform.as_entire_binding(),
                },
            ],
        ));
    }
}

fn queue_solid(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    if gpu.solid.count == 0 && gpu.portal.count == 0 {
        return;
    }
    let functions = draw_functions.read();
    for (list, mode, draw_function) in [
        (
            &gpu.solid,
            PipelineMode::Solid,
            functions.id::<DrawSolidCommands>(),
        ),
        (
            &gpu.portal,
            PipelineMode::Portal,
            functions.id::<DrawPortalCommands>(),
        ),
    ] {
        if list.count == 0 || list.bind_group.is_none() {
            continue;
        }
        for (view_entity, main_entity, view, extracted_camera, msaa) in &views {
            let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
                continue;
            };
            let Ok(pipeline_id) = pipeline.variants.specialize(
                &pipeline_cache,
                BlockEntityPipelineKey {
                    mode,
                    msaa: *msaa,
                    hdr: extracted_camera.hdr,
                },
            ) else {
                continue;
            };

            phase.add(
                Opaque3dBatchSetKey {
                    draw_function,
                    pipeline: pipeline_id,
                    material_bind_group_index: None,
                    lightmap_slab: None,
                    slabs: default(),
                },
                Opaque3dBinKey {
                    asset_id: AssetId::<Shader>::default().untyped(),
                },
                (view_entity, *main_entity),
                InputUniformIndex::default(),
                BinnedRenderPhaseType::NonMesh,
            );
        }
    }
}

fn queue_overlay(
    pipeline_cache: Res<PipelineCache>,
    pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    let draw_function = draw_functions.read().id::<DrawOverlayCommands>();
    queue_blended(
        &gpu.overlay,
        PipelineMode::Overlay,
        draw_function,
        &pipeline_cache,
        pipeline,
        phases,
        &views,
    );
}

fn queue_outline(
    pipeline_cache: Res<PipelineCache>,
    pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    let draw_function = draw_functions.read().id::<DrawOutlineCommands>();
    queue_blended(
        &gpu.outline,
        PipelineMode::Outline,
        draw_function,
        &pipeline_cache,
        pipeline,
        phases,
        &views,
    );
}

fn queue_crack(
    pipeline_cache: Res<PipelineCache>,
    pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    let draw_function = draw_functions.read().id::<DrawCrackCommands>();
    queue_blended(
        &gpu.crack,
        PipelineMode::Crack,
        draw_function,
        &pipeline_cache,
        pipeline,
        phases,
        &views,
    );
}

fn queue_additive(
    pipeline_cache: Res<PipelineCache>,
    pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    let draw_function = draw_functions.read().id::<DrawAdditiveCommands>();
    queue_blended(
        &gpu.additive,
        PipelineMode::Additive,
        draw_function,
        &pipeline_cache,
        pipeline,
        phases,
        &views,
    );
}

fn queue_blended(
    list: &VertexList,
    mode: PipelineMode,
    draw_function: DrawFunctionId,
    pipeline_cache: &PipelineCache,
    mut pipeline: ResMut<BlockEntityPipeline>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    views: &Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &bevy::render::camera::ExtractedCamera,
        &Msaa,
    )>,
) {
    if list.count == 0 || list.bind_group.is_none() {
        return;
    }
    for (view_entity, main_entity, view, extracted_camera, msaa) in views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = pipeline.variants.specialize(
            pipeline_cache,
            BlockEntityPipelineKey {
                mode,
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
                // Blended layers hug opaque geometry; drawing them last is enough.
                distance: 0.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            },
        );
    }
}

type DrawSolidCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawList<SOLID>,
);
type DrawOverlayCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawList<OVERLAY>,
);
type DrawCrackCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawList<CRACK>,
);
type DrawOutlineCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawList<OUTLINE>,
);
type DrawPortalCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawList<PORTAL>,
);
type DrawAdditiveCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawList<ADDITIVE>,
);

const SOLID: u8 = 0;
const OVERLAY: u8 = 1;
const CRACK: u8 = 2;
const PORTAL: u8 = 3;
const ADDITIVE: u8 = 4;
const OUTLINE: u8 = 5;

struct DrawList<const LIST: u8>;

impl<P: PhaseItem, const LIST: u8> RenderCommand<P> for DrawList<LIST> {
    type Param = SRes<BlockEntityGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view_offset: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let list = match LIST {
            OVERLAY => &gpu.overlay,
            OUTLINE => &gpu.outline,
            CRACK => &gpu.crack,
            PORTAL => &gpu.portal,
            ADDITIVE => &gpu.additive,
            _ => &gpu.solid,
        };
        let Some(bind_group) = &list.bind_group else {
            return RenderCommandResult::Skip;
        };
        if list.count == 0 {
            return RenderCommandResult::Skip;
        }
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        let count = if LIST == OUTLINE {
            list.count / 2 * BLOCK_SELECTION_VERTICES_PER_EDGE
        } else {
            list.count
        };
        pass.draw(0..count, 0..1);
        RenderCommandResult::Success
    }
}

#[cfg(test)]
#[path = "gpu/upload_tests.rs"]
mod upload_tests;

#[cfg(test)]
#[path = "gpu/tests.rs"]
mod tests;

#[cfg(test)]
#[path = "gpu/prewarm_tests.rs"]
mod prewarm_tests;
