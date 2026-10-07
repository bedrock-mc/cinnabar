//! Draws dropped-item and block models plus dynamic line geometry in the opaque 3D phase.
use crate::dropped_item::{
    DroppedItemModel, DroppedItemScene, ITEM_MESH_VERTEX_BYTES, ItemMeshVertex,
    MAX_DROPPED_ITEM_INSTANCES, MAX_DYNAMIC_ITEM_VERTICES, MAX_ITEM_LAYERS, MAX_ITEM_SPRITE_SIDE,
    block_mesh, cube_mesh, extruded_sprite_mesh, native_dropped_sprite_mesh,
};
use bevy::{
    asset::{AssetId, load_internal_asset, uuid_handle},
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Opaque3d, Opaque3dBatchSetKey, Opaque3dBinKey},
    ecs::{
        change_detection::Tick,
        query::ROQueryItem,
        system::{SystemParam, SystemParamItem, lifetimeless::Read, lifetimeless::SRes},
    },
    mesh::VertexBufferLayout,
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
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Extent3d,
            FilterMode, FragmentState, PipelineCache, RenderPipeline, RenderPipelineDescriptor,
            Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, ShaderType, Specializer,
            SpecializerKey, Texture, TextureDataOrder, TextureDescriptor, TextureDimension,
            TextureFormat, TextureSampleType, TextureUsages, TextureView, TextureViewDescriptor,
            TextureViewDimension, Variants, VertexAttribute, VertexFormat, VertexState,
            VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};
use std::ops::Range;

const ITEM_SHADER_HANDLE: Handle<Shader> = uuid_handle!("6b7c1b0e-2f3d-4a61-9d1e-7a8f2c4e5b13");
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
        "dropped_item.wgsl",
        crate::shader_safety::from_wgsl
    );
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
                    .run_if(crate::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

/// Per-copy vertex-rate data: three affine rows, then model index, light levels and overlay.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuItemInstance {
    rows: [[f32; 4]; 3],
    meta: [u32; 4],
}

const _: () = assert!(size_of::<GpuItemInstance>() == 64);

#[derive(Resource)]
struct ItemGpu {
    sampler: Sampler,
    environment: Buffer,
    instance_buffer: Buffer,
    mesh_buffer: Option<Buffer>,
    dynamic_buffer: Buffer,
    /// Vertex range of each model's mesh; empty for rejected models.
    ranges: Vec<Range<u32>>,
    _atlas: Option<Texture>,
    atlas_view: Option<TextureView>,
    models_revision: u64,
    dynamic_count: u32,
    /// Instance slot holding the identity transform used for dynamic geometry.
    identity_instance: u32,
    /// This frame's `(vertex range, instance index)` draws.
    draws: Vec<(Range<u32>, u32)>,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
    #[cfg(test)]
    upload_calls: u64,
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
            mipmap_filter: FilterMode::Nearest,
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
fn push_layer(atlas: &mut Vec<u8>, width: usize, height: usize, rgba8: &[u8]) -> u32 {
    let side = MAX_ITEM_SPRITE_SIDE as usize;
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
fn build_model(atlas: &mut Vec<u8>, model: &DroppedItemModel) -> Option<Vec<ItemMeshVertex>> {
    let side = MAX_ITEM_SPRITE_SIDE;
    let layers_used = atlas.len() / (side * side * 4) as usize;
    match model {
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
            push_layer(atlas, width, height, &sprite.rgba8);
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
            let layers: [u32; 6] =
                std::array::from_fn(|face| push_layer(atlas, tile, tile, &cube.faces[face]));
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
            let vertices = block_mesh(block, layers_used as u32)?;
            for (sprite, _) in block.materials.iter() {
                push_layer(
                    atlas,
                    sprite.width as usize,
                    sprite.height as usize,
                    &sprite.rgba8,
                );
            }
            Some(vertices)
        }
    }
}

fn rebuild_models(
    scene: &DroppedItemScene,
    device: &RenderDevice,
    queue: &RenderQueue,
    gpu: &mut ItemGpu,
) {
    let side = MAX_ITEM_SPRITE_SIDE as usize;
    let layer_bytes = side * side * 4;
    // Layer 0 is opaque white for untextured geometry.
    let mut atlas = vec![255_u8; layer_bytes];
    let mut vertices: Vec<ItemMeshVertex> = Vec::new();
    let mut ranges = Vec::with_capacity(scene.models.len());
    for model in scene.models.iter() {
        let start = vertices.len() as u32;
        if let Some(mesh) = build_model(&mut atlas, model) {
            vertices.extend(mesh);
        }
        ranges.push(start..vertices.len() as u32);
    }
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
                width: MAX_ITEM_SPRITE_SIDE,
                height: MAX_ITEM_SPRITE_SIDE,
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
            .filter(|range| !range.is_empty())
        else {
            continue;
        };
        draws.push((range.clone(), instances.len() as u32));
        instances.push(GpuItemInstance {
            rows: instance.world_from_item,
            meta: [
                instance.model,
                instance.block_level,
                instance.sky_level,
                instance.overlay_rgba8,
            ],
        });
    }
    gpu.identity_instance = instances.len() as u32;
    instances.push(GpuItemInstance {
        rows: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
        meta: [0, 15, 15, 0],
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
        bytemuck::cast_slice::<f32, u8>(&[scene.daylight, 0.0, 0.0, 0.0]),
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
                    ],
                },
            ],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: ITEM_SHADER_HANDLE,
            entry_point: Some("item_fragment".into()),
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
struct ItemPipelineKey {
    msaa: Msaa,
    hdr: bool,
}

impl Specializer<RenderPipeline> for ItemPipelineSpecializer {
    type Key = ItemPipelineKey;

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
    let Some(atlas_view) = gpu.atlas_view.as_ref() else {
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
        ],
    );
    gpu.bind_group = Some(bind_group);
    gpu.view_buffer_id = Some(view_buffer.id());
}

#[derive(SystemParam)]
struct QueueItemParams<'w, 's> {
    pipeline_cache: Res<'w, PipelineCache>,
    pipeline: ResMut<'w, ItemPipeline>,
    scene: Res<'w, DroppedItemScene>,
    terrain: Res<'w, TerrainItemMeshGenerations>,
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
}

fn queue_items(mut params: QueueItemParams<'_, '_>, mut next_tick: Local<Tick>) {
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
    for (view_entity, main_entity, view, msaa) in &params.views {
        let Some(phase) = params.phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        let Ok(pipeline_id) = params.pipeline.variants.specialize(
            &params.pipeline_cache,
            ItemPipelineKey {
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
    }
}

type DrawItemCommands = crate::gpu_timing::GpuDrawSpan<
    { crate::RuntimeStage::GpuActors as usize },
    (
        SetItemPipeline,
        crate::lighting::SetWorldLightmap,
        DrawItems,
    ),
>;

struct DrawItems;

impl<P: PhaseItem> RenderCommand<P> for DrawItems {
    type Param = SRes<ItemGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();

    fn render<'w>(
        _item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let Some(bind_group) = gpu.bind_group.as_ref() else {
            return RenderCommandResult::Success;
        };
        pass.set_bind_group(0, bind_group, &[view.offset]);
        pass.set_vertex_buffer(1, gpu.instance_buffer.slice(..));
        if let Some(mesh) = gpu.mesh_buffer.as_ref() {
            pass.set_vertex_buffer(0, mesh.slice(..));
            for (range, instance) in &gpu.draws {
                pass.draw(range.clone(), *instance..*instance + 1);
            }
        }
        if gpu.dynamic_count != 0 {
            pass.set_vertex_buffer(0, gpu.dynamic_buffer.slice(..));
            pass.draw(
                0..gpu.dynamic_count,
                gpu.identity_instance..gpu.identity_instance + 1,
            );
        }
        RenderCommandResult::Success
    }
}

#[cfg(test)]
#[path = "dropped_item_render/upload_tests.rs"]
mod upload_tests;

#[cfg(test)]
mod tests {
    use bevy::render::render_resource::ShaderStages;

    // The item fragment stage reads the view for distance fog; a vertex-only binding fails validation.
    #[test]
    fn fragment_view_reads_are_visible_to_the_fragment_stage() {
        let lighting = crate::material_shader::source(include_str!("lighting.wgsl")).replacen(
            "#define_import_path cinnabar::lighting",
            "",
            1,
        );
        let source = include_str!("dropped_item.wgsl")
            .replace(
                "#import bevy_render::view::View",
                "struct View { clip_from_world: mat4x4<f32>, world_position: vec3<f32>, }",
            )
            .replace(
                "#import cinnabar::lighting::{actor_light_colour, actor_distance_fog, tint_to_gamma, tint_to_linear}",
                &lighting,
            );
        assert!(crate::shader_test_support::fragment_reads_binding(
            &source, 0, 0
        ));
        assert!(
            super::item_bind_group_layout().entries[0]
                .visibility
                .contains(ShaderStages::FRAGMENT)
        );
    }
}
