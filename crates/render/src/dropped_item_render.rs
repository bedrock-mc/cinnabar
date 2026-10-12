//! Draws dropped-item and block models plus dynamic line geometry in the opaque 3D phase.
use bevy::prelude::IntoScheduleConfigs;
#[cfg(test)]
use bevy::prelude::IntoSystem;
#[cfg(test)]
use bevy::prelude::System;
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_resource::ShaderType;
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParam, SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    mesh::VertexBufferLayout,
    prelude::{
        App, BevyError, Commands, Component, Entity, FromWorld, Handle, Msaa, Plugin, Query, Res,
        ResMut, Resource, Result, Shader, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            BinnedRenderPhaseType, DrawFunctions, InputUniformIndex, PhaseItem, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewBinnedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, Buffer, BufferBindingType,
            BufferDescriptor, BufferId, BufferInitDescriptor, BufferSize, BufferUsages,
            CachedRenderPipelineId, Canonical, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, Face, FilterMode, FragmentState, FrontFace, PipelineCache,
            RenderPipeline, RenderPipelineDescriptor, Sampler, SamplerBindingType,
            SamplerDescriptor, ShaderStages, Specializer, SpecializerKey, Texture,
            TextureDataOrder, TextureDescriptor, TextureDimension, TextureFormat,
            TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexAttribute, VertexFormat, VertexState,
            VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use std::{ops::Range, sync::Arc};
use {
    render_model::{
        DroppedItemModel, ItemMeshVertex, MAX_DROPPED_ITEM_INSTANCES, MAX_DYNAMIC_ITEM_VERTICES,
        MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE,
        dropped_item::{
            DroppedItemBounds, DroppedItemCullView, ITEM_MESH_VERTEX_BYTES, block_mesh, cube_mesh,
            extruded_sprite_mesh, native_dropped_sprite_mesh,
        },
    },
    render_scene::DroppedItemScene,
};

const ITEM_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b7c1b0e-2f3d-4a61-9d1e-7a8f2c4e5b13");
mod glint;
pub(crate) mod terrain_items;
use terrain_items::{TerrainItemMeshGenerations, TerrainItemSessionSet};

#[cfg(all(test, feature = "publication-test-support"))]
mod terrain_tests;

#[derive(Debug, Clone, Copy, Default)]
pub struct DroppedItemRenderPlugin;

impl Plugin for DroppedItemRenderPlugin {
    fn build(&self, app: &mut App) {
        install(app);
    }

    fn finish(&self, app: &mut App) {
        install(app);
    }
}

#[derive(Resource)]
struct Installed;

fn install(app: &mut App) {
    app.init_resource::<DroppedItemScene>();
    crate::lighting::install(app);
    let Some(render_app) = app.get_sub_app(RenderApp) else {
        return;
    };
    if render_app.world().contains_resource::<Installed>() {
        return;
    }
    app.add_plugins(ExtractResourcePlugin::<DroppedItemScene>::default());
    load_internal_asset!(
        app,
        ITEM_SHADER_HANDLE,
        "dropped_item.wesl",
        crate::shader_safety::from_wesl
    );
    crate::pipeline_warmup::register::<ItemPipeline>(app);
    crate::install_opaque_phase_reset(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .insert_resource(Installed)
        .init_resource::<ItemPipeline>()
        .init_resource::<TerrainItemMeshGenerations>()
        .add_render_command::<Opaque3d, DrawItemCommands>()
        .add_systems(RenderStartup, init_gpu)
        .add_systems(
            Render,
            (
                terrain_items::begin_frame
                    .in_set(TerrainItemSessionSet)
                    .after(RenderSystems::ExtractCommands)
                    .before(RenderSystems::Queue),
                prepare_items.in_set(RenderSystems::PrepareResources),
                prepare_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_items
                    .run_if(render_scene::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

/// Per-copy vertex-rate data: three affine rows, then model index, light levels and overlay,
/// then the item glint UV scale (zero without glint).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuItemInstance {
    rows: [[f32; 4]; 3],
    meta: [u32; 4],
    glint: [f32; 4],
}

const _: () = assert!(size_of::<GpuItemInstance>() == 80);

#[derive(Resource)]
struct ItemGpu {
    sampler: Sampler,
    environment: Buffer,
    instance_buffer: Buffer,
    mesh_buffer: Option<Buffer>,
    dynamic_buffer: Buffer,
    /// Vertex range of each model's mesh; empty for rejected models.
    ranges: Vec<ItemMeshRange>,
    /// Item glint UV scale of each model, across its sprite.
    glint_scales: Vec<[f32; 2]>,
    glint_sampler: Sampler,
    /// The uploaded item glint and the pixels it holds (`None` for the transparent fallback).
    glint: Option<(Texture, TextureView, Option<Arc<[u8]>>)>,
    _atlas: Option<Texture>,
    atlas_view: Option<TextureView>,
    models_revision: u64,
    dynamic_count: u32,
    /// Instance slot holding the identity transform used for dynamic geometry.
    identity_instance: u32,
    /// This frame's `(vertex range, instance index)` draws.
    draws: Vec<ItemDraw>,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
    #[cfg(test)]
    upload_calls: u64,
}

/// A model's uploaded vertices and the cull mode its surface supports.
struct ItemMeshRange {
    vertices: Range<u32>,
    cull_back: bool,
    bounds: Option<DroppedItemBounds>,
}

/// One ordered draw, including the cull mode allowed by its model and transform.
struct ItemDraw {
    vertices: Range<u32>,
    instance: u32,
    cull_back: bool,
    bounds: Option<DroppedItemBounds>,
}

impl ItemDraw {
    /// Removes rear faces only when this view cannot clip or enter the item's closed surface.
    fn culls_in_view(&self, view: Option<&DroppedItemCullView>) -> bool {
        self.cull_back
            && self
                .bounds
                .zip(view)
                .is_some_and(|(bounds, view)| bounds.allows_back_face_culling(view))
    }
}

/// The prepared pipelines for a view; draw commands switch modes without regrouping instances.
#[derive(Component)]
struct ItemViewPipelines {
    two_sided: CachedRenderPipelineId,
    closed: CachedRenderPipelineId,
    cull_view: Option<DroppedItemCullView>,
}

fn init_gpu(mut commands: Commands, device: Res<RenderDevice>) {
    commands.insert_resource(ItemGpu {
        sampler: device.create_sampler(&SamplerDescriptor {
            label: Some("nearest dropped item sampler"),
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Nearest,
            min_filter: FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..default()
        }),
        environment: device.create_buffer(&BufferDescriptor {
            label: Some("dropped item environment"),
            size: 16,
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        instance_buffer: device.create_buffer(&BufferDescriptor {
            label: Some("bounded dropped item instances"),
            size: ((MAX_DROPPED_ITEM_INSTANCES + 1) * size_of::<GpuItemInstance>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        mesh_buffer: None,
        dynamic_buffer: device.create_buffer(&BufferDescriptor {
            label: Some("bounded dropped item dynamic geometry"),
            size: (MAX_DYNAMIC_ITEM_VERTICES * ITEM_MESH_VERTEX_BYTES) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }),
        ranges: Vec::new(),
        glint_scales: Vec::new(),
        glint_sampler: glint::sampler(&device),
        glint: None,
        _atlas: None,
        atlas_view: None,
        models_revision: u64::MAX,
        dynamic_count: 0,
        identity_instance: 0,
        draws: Vec::new(),
        bind_group: None,
        view_buffer_id: None,
        #[cfg(test)]
        upload_calls: 0,
    });
}

/// Appends one square-or-smaller RGBA tile as a new atlas layer and returns its index.
fn push_layer(atlas: &mut Vec<u8>, side: usize, width: usize, height: usize, rgba8: &[u8]) -> u32 {
    let layer_bytes = side * side * 4;
    let layer = atlas.len() / layer_bytes;
    atlas.resize(atlas.len() + layer_bytes, 0);
    let row_bytes = width * 4;
    for row in 0..height {
        let target = layer * layer_bytes + row * side * 4;
        atlas[target..target + row_bytes]
            .copy_from_slice(&rgba8[row * row_bytes..(row + 1) * row_bytes]);
    }
    layer as u32
}

/// Builds the mesh vertices for one model, appending its tiles to `atlas`; `None` if rejected.
fn build_model(
    atlas: &mut Vec<u8>,
    model: &DroppedItemModel,
    side: u32,
) -> Option<Vec<ItemMeshVertex>> {
    let layers_used = atlas.len() / (side * side * 4) as usize;
    match model {
        DroppedItemModel::Vacant => None,
        DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
            let (width, height) = (sprite.width as usize, sprite.height as usize);
            if layers_used >= MAX_ITEM_LAYERS
                || sprite.width > side
                || sprite.height > side
                || sprite.rgba8.len() != width * height * 4
            {
                return None;
            }
            let build = if matches!(model, DroppedItemModel::NativeSprite(_)) {
                native_dropped_sprite_mesh
            } else {
                extruded_sprite_mesh
            };
            let mesh = build(
                sprite.width,
                sprite.height,
                &sprite.rgba8,
                side,
                layers_used as u32,
            )?;
            push_layer(atlas, side as usize, width, height, &sprite.rgba8);
            Some(mesh)
        }
        DroppedItemModel::Cube(cube) => {
            let tile = cube.tile as usize;
            if cube.tile == 0
                || cube.tile > side
                || layers_used + 6 > MAX_ITEM_LAYERS
                || cube.faces.iter().any(|face| face.len() != tile * tile * 4)
            {
                return None;
            }
            let layers: [u32; 6] = std::array::from_fn(|face| {
                push_layer(atlas, side as usize, tile, tile, &cube.faces[face])
            });
            cube_mesh(layers, cube.tints, cube.tile, side)
        }
        DroppedItemModel::Block(block) => {
            for (sprite, _) in block.materials.iter() {
                if sprite.width == 0
                    || sprite.height == 0
                    || sprite.width > side
                    || sprite.height > side
                    || sprite.rgba8.len() != (sprite.width * sprite.height * 4) as usize
                {
                    return None;
                }
            }
            let vertices = block_mesh(block, layers_used as u32, side)?;
            for (sprite, _) in block.materials.iter() {
                push_layer(
                    atlas,
                    side as usize,
                    sprite.width as usize,
                    sprite.height as usize,
                    &sprite.rgba8,
                );
            }
            Some(vertices)
        }
    }
}

/// Sizes the atlas to admitted block artwork while ordinary item sprites retain their size.
fn atlas_side(models: &[DroppedItemModel]) -> u32 {
    models
        .iter()
        .filter_map(|model| match model {
            DroppedItemModel::Block(block) => block
                .materials
                .iter()
                .map(|(sprite, _)| sprite.width.max(sprite.height))
                .max(),
            DroppedItemModel::Cube(cube) => Some(cube.tile),
            _ => None,
        })
        .filter(|&side| side <= assets::MAX_TILE_SIZE)
        .fold(MAX_ITEM_SPRITE_SIDE, u32::max)
        .next_power_of_two()
}

fn rebuild_models(
    scene: &DroppedItemScene,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu: &mut ItemGpu,
) {
    let side = atlas_side(&scene.models) as usize;
    let layer_bytes = side * side * 4;
    // Layer 0 is opaque white for untextured geometry.
    let mut atlas = vec![255_u8; layer_bytes];
    let mut vertices: Vec<ItemMeshVertex> = Vec::new();
    let mut ranges = Vec::with_capacity(scene.models.len());
    for model in scene.models.iter() {
        let start = vertices.len() as u32;
        if let Some(mesh) = build_model(&mut atlas, model, side as u32) {
            vertices.extend(mesh);
        }
        ranges.push(ItemMeshRange {
            vertices: start..vertices.len() as u32,
            cull_back: model.can_cull_back_faces(),
            bounds: DroppedItemBounds::from_mesh(&vertices[start as usize..]),
        });
    }
    gpu.glint_scales = scene
        .models
        .iter()
        .map(|model| {
            glint::span(model, side as u32).map(|span| render_model::ITEM_GLINT_UV_SCALE / span)
        })
        .collect();
    let layers = atlas.len() / layer_bytes;
    gpu.mesh_buffer = (!vertices.is_empty()).then(|| {
        device.create_buffer_with_data(&BufferInitDescriptor {
            label: Some("dropped item model meshes"),
            contents: bytemuck::cast_slice::<ItemMeshVertex, u8>(&vertices),
            usage: BufferUsages::VERTEX,
        })
    });
    gpu.ranges = ranges;
    let texture = device.create_texture_with_data(
        queue,
        &TextureDescriptor {
            label: Some("dropped item texture layers"),
            size: Extent3d {
                width: side as u32,
                height: side as u32,
                depth_or_array_layers: layers as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        TextureDataOrder::LayerMajor,
        &atlas,
    );
    gpu.atlas_view = Some(texture.create_view(&TextureViewDescriptor {
        label: Some("dropped item texture layer array"),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    }));
    gpu._atlas = Some(texture);
    gpu.models_revision = scene.models_revision;
    gpu.bind_group = None;
}

fn prepare_items(
    scene: Res<DroppedItemScene>,
    terrain: Res<TerrainItemMeshGenerations>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<ItemGpu>,
) {
    if gpu.models_revision != scene.models_revision || gpu.atlas_view.is_none() {
        rebuild_models(&scene, &device, &queue, &mut gpu);
    }
    glint::upload(&scene, &device, &queue, &mut gpu);
    if scene.instances.is_empty()
        && scene.dynamic.is_empty()
        && !scene
            .terrain_instances
            .iter()
            .any(|candidate| terrain.visible(candidate))
    {
        gpu.draws.clear();
        gpu.dynamic_count = 0;
        gpu.identity_instance = 0;
        return;
    }
    let mut instances = Vec::with_capacity(scene.instances.len() + 1);
    let mut draws = Vec::with_capacity(scene.instances.len());
    for instance in scene
        .instances
        .iter()
        .chain(
            scene
                .terrain_instances
                .iter()
                .filter(|candidate| terrain.visible(candidate))
                .map(|candidate| &candidate.instance),
        )
        .take(MAX_DROPPED_ITEM_INSTANCES)
    {
        let Some(range) = gpu
            .ranges
            .get(instance.model as usize)
            .filter(|range| !range.vertices.is_empty())
        else {
            continue;
        };
        let rows = instance
            .world_from_item
            .map(|row| glam::Vec3::new(row[0], row[1], row[2]));
        draws.push(ItemDraw {
            vertices: range.vertices.clone(),
            instance: instances.len() as u32,
            cull_back: range.cull_back && rows[0].dot(rows[1].cross(rows[2])) > 0.0,
            bounds: range
                .bounds
                .and_then(|bounds| bounds.transformed(instance.world_from_item)),
        });
        instances.push(glint::pack_instance(instance, &gpu.glint_scales));
    }
    gpu.identity_instance = instances.len() as u32;
    instances.push(GpuItemInstance {
        rows: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        meta: [0, 15, 15, 0],
        glint: [0.0; 4],
    });
    queue.write_buffer(
        &gpu.instance_buffer,
        0,
        bytemuck::cast_slice::<GpuItemInstance, u8>(&instances),
    );
    #[cfg(test)]
    {
        gpu.upload_calls += 1;
    }
    gpu.dynamic_count = scene.dynamic.len() as u32;
    if !scene.dynamic.is_empty() {
        queue.write_buffer(
            &gpu.dynamic_buffer,
            0,
            bytemuck::cast_slice::<ItemMeshVertex, u8>(&scene.dynamic),
        );
        #[cfg(test)]
        {
            gpu.upload_calls += 1;
        }
    }
    gpu.draws = draws;
    queue.write_buffer(
        &gpu.environment,
        0,
        bytemuck::cast_slice::<f32, u8>(&[
            scene.daylight,
            scene.glint_scroll[0],
            scene.glint_scroll[1],
            scene.glint_scroll[2],
        ]),
    );
    #[cfg(test)]
    {
        gpu.upload_calls += 1;
    }
}

struct ItemPipelineSpecializer;

#[derive(Resource)]
struct ItemPipeline {
    variants: Variants<RenderPipeline, ItemPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for ItemPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = item_bind_group_layout();
        let descriptor = item_pipeline_descriptor(bind_group_layout.clone());
        Self {
            variants: Variants::new(ItemPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

fn item_bind_group_layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "dropped item bind group layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                // The fragment stage reads the camera position for distance fog.
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
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2Array,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 3,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 4,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Texture {
                    sample_type: TextureSampleType::Float { filterable: true },
                    view_dimension: TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 5,
                visibility: ShaderStages::FRAGMENT,
                ty: BindingType::Sampler(SamplerBindingType::Filtering),
                count: None,
            },
        ],
    )
}

fn item_pipeline_descriptor(layout: BindGroupLayoutDescriptor) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("dropped item pipeline".into()),
        layout: vec![layout, crate::lighting::layout()],
        vertex: VertexState {
            shader: ITEM_SHADER_HANDLE,
            entry_point: Some("item_vertex".into()),
            buffers: vec![
                VertexBufferLayout {
                    array_stride: ITEM_MESH_VERTEX_BYTES as u64,
                    step_mode: VertexStepMode::Vertex,
                    attributes: vec![
                        VertexAttribute {
                            format: VertexFormat::Float32x3,
                            offset: 0,
                            shader_location: 0,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x2,
                            offset: 12,
                            shader_location: 1,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x3,
                            offset: 20,
                            shader_location: 2,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32,
                            offset: 32,
                            shader_location: 7,
                        },
                        VertexAttribute {
                            format: VertexFormat::Unorm8x4,
                            offset: 36,
                            shader_location: 8,
                        },
                    ],
                },
                VertexBufferLayout {
                    array_stride: size_of::<GpuItemInstance>() as u64,
                    step_mode: VertexStepMode::Instance,
                    attributes: vec![
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 0,
                            shader_location: 3,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 16,
                            shader_location: 4,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 32,
                            shader_location: 5,
                        },
                        VertexAttribute {
                            format: VertexFormat::Uint32x4,
                            offset: 48,
                            shader_location: 6,
                        },
                        VertexAttribute {
                            format: VertexFormat::Float32x4,
                            offset: 64,
                            shader_location: 9,
                        },
                    ],
                },
            ],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: ITEM_SHADER_HANDLE,
            entry_point: Some("item_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: render_scene::SCENE_COLOR_FORMAT,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        depth_stencil: Some(DepthStencilState {
            format: CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::GreaterEqual),
            stencil: default(),
            bias: default(),
        }),
        ..default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct ItemPipelineKey {
    msaa: Msaa,
    hdr: bool,
    cull_back: bool,
    invert_culling: bool,
}

impl Specializer<RenderPipeline> for ItemPipelineSpecializer {
    type Key = ItemPipelineKey;

    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        descriptor.primitive.cull_mode = key.cull_back.then_some(Face::Back);
        descriptor.primitive.front_face = if key.invert_culling {
            FrontFace::Cw
        } else {
            FrontFace::Ccw
        };
        descriptor.multisample.count = key.msaa.samples();
        crate::alpha_coverage::apply(descriptor, true);
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

fn prepare_bind_group(
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    pipeline: Res<ItemPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<ItemGpu>,
) {
    let Some(view_binding) = view_uniforms.uniforms.binding() else {
        gpu.bind_group = None;
        return;
    };
    let (Some(atlas_view), Some((_, glint_view, _))) =
        (gpu.atlas_view.as_ref(), gpu.glint.as_ref())
    else {
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
    let bind_group = device.create_bind_group(
        "dropped item bind group",
        &cache.get_bind_group_layout(&pipeline.bind_group_layout),
        &[
            BindGroupEntry {
                binding: 0,
                resource: view_binding,
            },
            BindGroupEntry {
                binding: 1,
                resource: BindingResource::TextureView(atlas_view),
            },
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
            BindGroupEntry {
                binding: 3,
                resource: gpu.environment.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::TextureView(glint_view),
            },
            BindGroupEntry {
                binding: 5,
                resource: BindingResource::Sampler(&gpu.glint_sampler),
            },
        ],
    );
    gpu.bind_group = Some(bind_group);
    gpu.view_buffer_id = Some(view_buffer.id());
}

/// The per-view state item queuing reads from each extracted camera.
type ItemView = (
    Entity,
    &'static MainEntity,
    &'static ExtractedView,
    &'static bevy::render::camera::ExtractedCamera,
    &'static Msaa,
    Option<&'static bevy::render::camera::TemporalJitter>,
);

#[derive(SystemParam)]
struct QueueItemParams<'w, 's> {
    pipeline_cache: Res<'w, PipelineCache>,
    pipeline: ResMut<'w, ItemPipeline>,
    scene: Res<'w, DroppedItemScene>,
    terrain: Res<'w, TerrainItemMeshGenerations>,
    phases: ResMut<'w, ViewBinnedRenderPhases<Opaque3d>>,
    draw_functions: Res<'w, DrawFunctions<Opaque3d>>,
    views: Query<'w, 's, ItemView>,
}

fn queue_items(mut commands: Commands, mut params: QueueItemParams<'_, '_>) {
    if params.scene.instances.is_empty()
        && params.scene.dynamic.is_empty()
        && !params
            .scene
            .terrain_instances
            .iter()
            .any(|candidate| params.terrain.visible(candidate))
    {
        return;
    }
    let draw_function = params.draw_functions.read().id::<DrawItemCommands>();
    for (view_entity, main_entity, view, extracted_camera, msaa, jitter) in &params.views {
        let Some(phase) = params.phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = params.pipeline.variants.specialize(
            &params.pipeline_cache,
            ItemPipelineKey {
                msaa: *msaa,
                hdr: extracted_camera.hdr,
                cull_back: false,
                invert_culling: view.invert_culling,
            },
        ) else {
            continue;
        };

        let Ok(closed) = params.pipeline.variants.specialize(
            &params.pipeline_cache,
            ItemPipelineKey {
                msaa: *msaa,
                hdr: extracted_camera.hdr,
                cull_back: closed_item_culling(*msaa),
                invert_culling: view.invert_culling,
            },
        ) else {
            continue;
        };
        let projection = || view.clip_from_view * view.world_from_view.to_matrix().inverse();
        let clip_from_world = if jitter.is_some() {
            projection()
        } else {
            view.clip_from_world.unwrap_or_else(projection)
        };
        commands.entity(view_entity).insert(ItemViewPipelines {
            two_sided: pipeline_id,
            closed,
            cull_view: DroppedItemCullView::new(
                clip_from_world,
                view.world_from_view.translation(),
            ),
        });

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

type DrawItemCommands = crate::gpu_timing::GpuDrawSpan<
    { render_scene::RuntimeStage::GpuActors as usize },
    (
        SetItemPipeline,
        crate::lighting::SetWorldLightmap,
        DrawItems,
    ),
>;

struct DrawItems;

impl<P: PhaseItem> RenderCommand<P> for DrawItems {
    type Param = (SRes<ItemGpu>, SRes<PipelineCache>);
    type ViewQuery = (Read<ViewUniformOffset>, Read<ItemViewPipelines>);
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (gpu, cache) = gpu;
        let (gpu, cache) = (gpu.into_inner(), cache.into_inner());
        let (view, pipelines) = view;
        let Some(two_sided) = cache.get_render_pipeline(pipelines.two_sided) else {
            return RenderCommandResult::Skip;
        };
        let closed = cache
            .get_render_pipeline(pipelines.closed)
            .unwrap_or(two_sided);
        let _zone = bevy::log::info_span!("dropped_item.draw").entered();
        let Some(bind_group) = gpu.bind_group.as_ref() else {
            return RenderCommandResult::Success;
        };
        pass.set_bind_group(0, bind_group, &[view.offset]);
        pass.set_vertex_buffer(1, gpu.instance_buffer.slice(..));
        if let Some(mesh) = gpu.mesh_buffer.as_ref() {
            pass.set_vertex_buffer(0, mesh.slice(..));
            for draw in &gpu.draws {
                pass.set_render_pipeline(if draw.culls_in_view(pipelines.cull_view.as_ref()) {
                    closed
                } else {
                    two_sided
                });
                pass.draw(draw.vertices.clone(), draw.instance..draw.instance + 1);
            }
        }
        if gpu.dynamic_count != 0 {
            pass.set_render_pipeline(two_sided);
            pass.set_vertex_buffer(0, gpu.dynamic_buffer.slice(..));
            pass.draw(
                0..gpu.dynamic_count,
                gpu.identity_instance..gpu.identity_instance + 1,
            );
        }
        RenderCommandResult::Success
    }
}

/// Alpha-to-coverage can leave samples open to rear faces at texture edges.
/// Retain two-sided drawing for those passes so their coverage stays unchanged.
fn closed_item_culling(msaa: Msaa) -> bool {
    msaa == Msaa::Off
}

impl crate::pipeline_warmup::PrewarmPipelines for ItemPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        for cull_back in [false, true]
            .into_iter()
            .filter(|&cull| !cull || closed_item_culling(view.msaa))
        {
            for invert_culling in [false, true] {
                ids.push(self.variants.specialize(
                    cache,
                    ItemPipelineKey {
                        msaa: view.msaa,
                        hdr: view.hdr,
                        cull_back,
                        invert_culling,
                    },
                )?);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "dropped_item_render/upload_tests.rs"]
mod upload_tests;

#[cfg(test)]
#[path = "dropped_item_render/pipeline_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "dropped_item_render/culling_raster_tests.rs"]
mod culling_raster_tests;
