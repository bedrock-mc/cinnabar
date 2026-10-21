//! Atlas-backed model, portal and overlay passes with separate vertex lists.
//! Models and portal planes draw in the opaque phase; overlays draw afterward.

use crate::queue_writes::ProfiledQueue as _;
use bevy::prelude::IntoScheduleConfigs;
#[cfg(test)]
use bevy::prelude::System;
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_resource::ShaderType;
use std::{mem::size_of, ops::Range, sync::Arc};

#[cfg(test)]
use bevy::prelude::IntoSystem;
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{
        CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey, Transparent3d,
    },
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    prelude::{
        App, BevyError, Commands, Entity, FromWorld, Handle, Msaa, Plugin, Query, Res, ResMut,
        Resource, Result, Shader, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            BinnedRenderPhaseType, DrawFunctionId, DrawFunctions, InputUniformIndex, PhaseItem,
            PhaseItemExtraIndex, RenderCommand, RenderCommandResult, SetItemPipeline,
            TrackedRenderPass, ViewBinnedRenderPhases, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendFactor,
            BlendOperation, BlendState, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferSize, BufferUsages, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, FilterMode, FragmentState, Origin3d, PipelineCache,
            PrimitiveTopology, RenderPipeline, RenderPipelineDescriptor, Sampler,
            SamplerBindingType, SamplerDescriptor, ShaderStages, Specializer, SpecializerKey,
            TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use render_model::{
    BLOCK_ENTITY_VERTEX_WORDS, BLOCK_SELECTION_VERTICES_PER_EDGE, BlockEntityVertex,
};

#[path = "gpu/pipeline.rs"]
mod pipeline;
use pipeline::{BlockEntityPipeline, BlockEntityPipelineKey, PipelineMode};

const SHADER_HANDLE: Handle<Shader> = uuid_handle!("6f0c1c1e-3b6d-4a7e-9b1e-2f4f8a1d5c33");
const VERTEX_BYTES: u64 = (BLOCK_ENTITY_VERTEX_WORDS * size_of::<f32>()) as u64;
const MIN_BUFFER_VERTICES: u64 = 1024;
/// Bytes compared per `memcmp` while locating the changed span of a vertex list.
const COMPARE_BLOCK_BYTES: usize = 4096;
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
    app.init_resource::<render_scene::BlockEntityFrameResource>()
        .init_resource::<render_scene::BlockSelectionFrameResource>()
        .init_resource::<render_scene::BlockEntitySceneResource>()
        .init_resource::<render_scene::StaticItemPlacementsResource>();
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app
        .world()
        .contains_resource::<BlockEntityRenderInstalled>()
    {
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<
        render_scene::BlockEntityFrameResource,
    >::default());
    app.add_plugins(ExtractResourcePlugin::<
        render_scene::BlockSelectionFrameResource,
    >::default());
    crate::lighting::install(app);
    crate::upload_staging::install(app);
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
        .add_render_command::<Transparent3d, DrawHighlightCommands>()
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
                    .distributive_run_if(render_scene::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[path = "gpu/vertices.rs"]
mod vertices;
use vertices::VertexList;
#[cfg(feature = "shaderpacks")]
#[path = "gpu/shaderpack_source.rs"]
mod shaderpack_source;
#[cfg(feature = "shaderpacks")]
pub(crate) use shaderpack_source::shaderpack_block_entities;

/// The vertex lists last passed to each [`VertexList::upload`], shared rather than copied.
#[derive(Default)]
struct UploadedLists {
    solid: Arc<[BlockEntityVertex]>,
    overlay: Arc<[BlockEntityVertex]>,
    portal: Arc<[BlockEntityVertex]>,
    additive: Arc<[BlockEntityVertex]>,
    outline: Arc<[BlockEntityVertex]>,
    /// Frame cracks followed by the selection highlight.
    crack: Vec<BlockEntityVertex>,
}

#[derive(Resource)]
pub(crate) struct BlockEntityGpu {
    solid: VertexList,
    overlay: VertexList,
    outline: VertexList,
    crack: VertexList,
    /// Prefix of the shared crack buffer containing damage faces, before selection highlights.
    frame_crack_count: u32,
    portal: VertexList,
    additive: VertexList,
    uploaded: UploadedLists,
    /// Reused to assemble the crack list before it replaces `uploaded.crack`.
    crack_scratch: Vec<BlockEntityVertex>,
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
        frame_crack_count: 0,
        portal: VertexList::new(),
        additive: VertexList::new(),
        uploaded: UploadedLists::default(),
        crack_scratch: Vec::new(),
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

pub(crate) fn prepare_resources(
    frame: Res<render_scene::BlockEntityFrameResource>,
    selection: Res<render_scene::BlockSelectionFrameResource>,
    atmosphere: Option<Res<render_scene::AtmosphereFrame>>,
    uploads: crate::upload_staging::FrameUploads,
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
        uploads.write(
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
        .unwrap_or_else(|| render_model::block_entity::selection::fallback_atlas());
    if gpu.atlas_identity != atlas.identity || gpu.atlas_size != atlas.size || gpu.texture.is_none()
    {
        let texture = uploads.device.create_texture(&TextureDescriptor {
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
            view_formats: crate::actor::gpu::ARTWORK_VIEW_FORMATS,
        });
        write_rows(
            &uploads.queue,
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
                &uploads.queue,
                texture,
                atlas.static_height,
                atlas.size[0],
                dynamic_rows,
                &frame.dynamic_rgba8,
            );
        }
        gpu.dynamic_revision = frame.dynamic_revision;
    }
    let BlockEntityGpu {
        solid,
        overlay,
        outline,
        crack,
        portal,
        additive,
        uploaded,
        crack_scratch,
        frame_crack_count,
        frame_revision,
        selection_revision,
        ..
    } = &mut *gpu;
    if *frame_revision != frame.revision {
        for (list, previous, vertices, label) in [
            (
                &mut *solid,
                &mut uploaded.solid,
                &frame.solid,
                "block-entity solid vertices",
            ),
            (
                &mut *portal,
                &mut uploaded.portal,
                &frame.portal,
                "portal encoded plane vertices",
            ),
            (
                &mut *additive,
                &mut uploaded.additive,
                &frame.additive,
                "dragon death additive vertices",
            ),
            (
                &mut *overlay,
                &mut uploaded.overlay,
                &frame.overlay,
                "block-entity overlay vertices",
            ),
        ] {
            list.upload(previous, vertices, &uploads, label);
            *previous = Arc::clone(vertices);
        }
    }
    if *selection_revision != selection.revision {
        outline.upload(
            &uploaded.outline,
            &selection.outline,
            &uploads,
            "block selection line vertices",
        );
        uploaded.outline = Arc::clone(&selection.outline);
    }
    if *frame_revision != frame.revision || *selection_revision != selection.revision {
        crack_scratch.clear();
        crack_scratch.extend_from_slice(&frame.crack);
        *frame_crack_count = frame.crack.len() as u32;
        crack_scratch.extend_from_slice(&selection.highlight);
        crack.upload(
            &uploaded.crack,
            crack_scratch,
            &uploads,
            "block-entity crack vertices",
        );
        std::mem::swap(crack_scratch, &mut uploaded.crack);
        *frame_revision = frame.revision;
        *selection_revision = selection.revision;
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
    render_queue.profiled_write_texture(
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

/// A camera block entities queue for, and whether it runs a shader pack.
type BlockEntityView = (
    Entity,
    &'static MainEntity,
    &'static ExtractedView,
    &'static bevy::render::camera::ExtractedCamera,
    &'static Msaa,
    bevy::ecs::query::Has<render_scene::shaderpack::ShaderPackRendering>,
);

fn queue_solid(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    mut phases: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<DrawFunctions<Opaque3d>>,
    views: Query<BlockEntityView>,
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
        for (view_entity, main_entity, view, extracted_camera, msaa, _pack_view) in &views {
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
    mut pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<BlockEntityView>,
) {
    let draw_function = draw_functions.read().id::<DrawOverlayCommands>();
    queue_blended(
        &gpu.overlay,
        PipelineMode::Overlay,
        draw_function,
        &pipeline_cache,
        &mut pipeline,
        &mut phases,
        &views,
        |_, _| false,
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "a queue system's render-world inputs"
)]
fn queue_outline(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<BlockEntityView>,
    pack: render_scene::shaderpack::ShaderPackDrawnEffects,
) {
    let draw_function = draw_functions.read().id::<DrawOutlineCommands>();
    queue_blended(
        &gpu.outline,
        PipelineMode::Outline,
        draw_function,
        &pipeline_cache,
        &mut pipeline,
        &mut phases,
        &views,
        |view, _| pack.replaces(view, |drawn| drawn.selection),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "a queue system's render-world inputs"
)]
fn queue_crack(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<BlockEntityView>,
    pack: render_scene::shaderpack::ShaderPackDrawnEffects,
) {
    // Check partition ranges when drawing, after this frame's vertex upload.
    for highlight in [false, true] {
        let functions = draw_functions.read();
        let draw_function = if highlight {
            functions.id::<DrawHighlightCommands>()
        } else {
            functions.id::<DrawCrackCommands>()
        };
        queue_blended(
            &gpu.crack,
            PipelineMode::Crack,
            draw_function,
            &pipeline_cache,
            &mut pipeline,
            &mut phases,
            &views,
            |view, _| {
                pack.replaces(view, |drawn| {
                    if highlight {
                        drawn.selection
                    } else {
                        drawn.cracks
                    }
                })
            },
        );
    }
}

fn queue_additive(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlockEntityPipeline>,
    gpu: Res<BlockEntityGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<BlockEntityView>,
) {
    let draw_function = draw_functions.read().id::<DrawAdditiveCommands>();
    queue_blended(
        &gpu.additive,
        PipelineMode::Additive,
        draw_function,
        &pipeline_cache,
        &mut pipeline,
        &mut phases,
        &views,
        |_, _| false,
    );
}

/// Queues `list` on every camera `skip` does not reject, given the view and whether it runs
/// a shader pack (views whose pack draws the list itself).
#[allow(
    clippy::too_many_arguments,
    reason = "one blended list's queue inputs and its view filter"
)]
fn queue_blended(
    list: &VertexList,
    mode: PipelineMode,
    draw_function: DrawFunctionId,
    pipeline_cache: &PipelineCache,
    pipeline: &mut BlockEntityPipeline,
    phases: &mut ViewSortedRenderPhases<Transparent3d>,
    views: &Query<BlockEntityView>,
    skip: impl Fn(Entity, bool) -> bool,
) {
    if list.count == 0 || list.bind_group.is_none() {
        return;
    }
    for (view_entity, main_entity, view, extracted_camera, msaa, pack_view) in views {
        if skip(view_entity, pack_view) {
            continue;
        }
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
type DrawHighlightCommands = (
    SetItemPipeline,
    crate::lighting::SetWorldLightmap,
    DrawList<HIGHLIGHT>,
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
const HIGHLIGHT: u8 = 6;

/// Selects damage faces or selection faces from their shared upload.
fn crack_vertices(count: u32, damage: u32, highlight: bool) -> Range<u32> {
    if highlight {
        damage.min(count)..count
    } else {
        0..damage.min(count)
    }
}

struct DrawList<const LIST: u8>;

impl<P: PhaseItem, const LIST: u8> RenderCommand<P> for DrawList<LIST> {
    type Param = (
        SRes<BlockEntityGpu>,
        crate::shaderpack::native_actors::NativeActorCoverage<'static>,
    );
    type ViewQuery = (Read<ViewUniformOffset>, Read<ExtractedView>, Entity);
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (gpu, coverage) = gpu;
        if matches!(LIST, SOLID | OVERLAY) && coverage.blocks(view.2) {
            return RenderCommandResult::Skip;
        }
        let gpu = gpu.into_inner();
        let list = match LIST {
            OVERLAY => &gpu.overlay,
            OUTLINE => &gpu.outline,
            CRACK | HIGHLIGHT => &gpu.crack,
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
        let (view_offset, view, _) = view;
        pass.set_bind_group(0, bind_group, &[view_offset.offset]);
        let count = if LIST == OUTLINE {
            list.count / 2 * BLOCK_SELECTION_VERTICES_PER_EDGE
        } else {
            list.count
        };
        if LIST == OUTLINE {
            pass.draw(0..count, 0..1);
        } else {
            let _zone = bevy::log::info_span!("block_entity.submit_cull").entered();
            let mut submitted = 0;
            let matrix = view.clip_from_world.unwrap_or_else(|| {
                view.clip_from_view * view.world_from_view.to_matrix().inverse()
            });
            let keep = match LIST {
                CRACK => crack_vertices(count, gpu.frame_crack_count, false),
                HIGHLIGHT => crack_vertices(count, gpu.frame_crack_count, true),
                _ => 0..count,
            };
            for range in render_model::block_entity::draw_ranges::visible_ranges(&list.runs, matrix)
            {
                let range = range.start.max(keep.start)..range.end.min(keep.end);
                if range.is_empty() {
                    continue;
                }
                submitted += range.end - range.start;
                pass.draw(range, 0..1);
            }
            bevy::log::trace!(
                candidate_vertices = count,
                submitted_vertices = submitted,
                hidden_by_view_cone = count - submitted,
                "block entity submission work"
            );
        }
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

#[cfg(test)]
#[path = "gpu/reinstall_tests.rs"]
mod reinstall_tests;

#[cfg(test)]
#[path = "gpu/live_reload_tests.rs"]
mod live_reload_tests;
