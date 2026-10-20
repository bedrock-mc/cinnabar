//! Draws [`render_model::NametagScene`] in the transparent 3D phase, preserving each
//! record's through-geometry or depth-tested visibility and alpha blending.

use crate::queue_writes::ProfiledQueue as _;
use bevy::prelude::DetectChanges;
use bevy::prelude::IntoScheduleConfigs;
use bevy::render::render_phase::AddRenderCommand;
use bevy::render::render_resource::ShaderType;
#[cfg(test)]
use bevy::render::renderer::RenderQueue;
use render_scene::NametagSceneResource;
use std::{ops::Range, sync::Arc};

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
        App, BevyError, Commands, Entity, FromWorld, Handle, Msaa, Query, Res, ResMut, Resource,
        Result, Shader, World, default,
    },
    render::{
        Render, RenderApp, RenderStartup, RenderSystems,
        extract_resource::ExtractResourcePlugin,
        render_phase::{
            DrawFunctionId, DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand,
            RenderCommandResult, SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendComponent, BlendState, Buffer,
            BufferBindingType, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthStencilState, Extent3d,
            FilterMode, FragmentState, PipelineCache, RenderPipeline, RenderPipelineDescriptor,
            Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages, Specializer,
            SpecializerKey, TexelCopyBufferLayout, TextureAspect, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::RenderDevice,
        sync_world::MainEntity,
        view::{ExtractedView, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use render_model::{MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NametagAtlasRect, NametagRecord};

const NAMETAG_SHADER_HANDLE: Handle<Shader> = uuid_handle!("5d1f0c8e-2a47-4b93-9e6c-1f7a3b8d4c20");
const RECORD_BYTES: usize = std::mem::size_of::<NametagRecord>();
const NAMETAG_BLEND: BlendState = BlendState {
    alpha: BlendComponent::REPLACE,
    ..BlendState::ALPHA_BLENDING
};

#[path = "nametag_render/shader.rs"]
mod shader;

#[cfg(test)]
#[path = "nametag_render/cosmetics_tests.rs"]
mod cosmetics_tests;

#[derive(Debug, PartialEq)]
struct NametagBatch {
    records: Range<u32>,
    depth_tested: bool,
    text: bool,
}

/// Refills `batches` with adjacent material modes batched, without moving records across
/// label boundaries; the vector's allocation is reused.
fn record_batches(records: &[NametagRecord], batches: &mut Vec<NametagBatch>) {
    batches.clear();
    for (index, record) in records.iter().enumerate() {
        let depth_tested = record.depth_tested != 0;
        let text = record.text != 0;
        if let Some(last) = batches
            .last_mut()
            .filter(|last| last.depth_tested == depth_tested && last.text == text)
        {
            last.records.end += 1;
        } else {
            batches.push(NametagBatch {
                records: index as u32..index as u32 + 1,
                depth_tested,
                text,
            });
        }
    }
}

/// A material batch consumes one Bevy phase entry, irrespective of its glyph count.
fn phase_batch_range(index: usize) -> Range<u32> {
    index as u32..index as u32 + 1
}

pub(crate) fn install_nametag_render(app: &mut App) {
    crate::upload_staging::install(app);
    crate::pipeline_warmup::register::<NametagPipeline>(app);
    app.init_resource::<NametagSceneResource>()
        .add_plugins(ExtractResourcePlugin::<NametagSceneResource>::default());
    load_internal_asset!(
        app,
        NAMETAG_SHADER_HANDLE,
        "nametag.wesl",
        shader::from_wesl
    );
    crate::transparent_phase::install(app.sub_app_mut(RenderApp));
    app.sub_app_mut(RenderApp)
        .init_resource::<NametagPipeline>()
        .add_render_command::<Transparent3d, DrawNametags>()
        .add_systems(RenderStartup, init_nametag_gpu)
        .add_systems(
            Render,
            (
                prepare_nametags.in_set(RenderSystems::PrepareResources),
                prepare_nametag_bind_group.in_set(RenderSystems::PrepareBindGroups),
                queue_nametags
                    .run_if(render_scene::panorama::world_passes_enabled)
                    .in_set(RenderSystems::Queue),
            ),
        );
}

#[derive(Resource)]
struct NametagGpu {
    record_buffer: Buffer,
    atlas_view: TextureView,
    atlas_texture: bevy::render::render_resource::Texture,
    sampler: Sampler,
    atlas: Arc<[NametagAtlasRect]>,
    batches: Vec<NametagBatch>,
    total: u32,
    bind_group: Option<BindGroup>,
    view_buffer_id: Option<BufferId>,
}

fn init_nametag_gpu(mut commands: Commands, render_device: Res<RenderDevice>) {
    let record_buffer = render_device.create_buffer_with_data(&BufferInitDescriptor {
        label: Some("nametag records"),
        contents: &vec![0_u8; MAX_NAMETAG_RECORDS * RECORD_BYTES],
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
    });
    let atlas_texture = render_device.create_texture(&TextureDescriptor {
        label: Some("nametag text atlas"),
        size: Extent3d {
            width: NAMETAG_ATLAS_SIDE,
            height: NAMETAG_ATLAS_SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8UnormSrgb,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let atlas_view = atlas_texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2),
        ..default()
    });
    let sampler = render_device.create_sampler(&SamplerDescriptor {
        label: Some("nametag atlas sampler"),
        address_mode_u: AddressMode::ClampToEdge,
        address_mode_v: AddressMode::ClampToEdge,
        mag_filter: FilterMode::Nearest,
        min_filter: FilterMode::Nearest,
        ..default()
    });
    commands.insert_resource(NametagGpu {
        record_buffer,
        atlas_view,
        atlas_texture,
        sampler,
        atlas: Arc::from([]),
        batches: Vec::new(),
        total: 0,
        bind_group: None,
        view_buffer_id: None,
    });
}

/// Uploads records and batches only when the extracted scene was reassigned, the GPU
/// resources were (re)created, or the queue changed; the atlas uploads its changed cells.
fn prepare_nametags(
    scene: Res<NametagSceneResource>,
    uploads: crate::upload_staging::FrameUploads,
    mut gpu: ResMut<NametagGpu>,
) {
    if scene.is_changed() || gpu.is_added() || uploads.queue.is_changed() {
        let total = scene.records.len().min(MAX_NAMETAG_RECORDS);
        gpu.total = total as u32;
        record_batches(&scene.records[..total], &mut gpu.batches);
        if total > 0 {
            uploads.write(
                &gpu.record_buffer,
                0,
                bytemuck::cast_slice::<NametagRecord, u8>(&scene.records[..total]),
            );
        }
    }
    if Arc::ptr_eq(&scene.atlas, &gpu.atlas) {
        return;
    }
    for rectangle in NametagAtlasRect::updates(&scene.atlas, &gpu.atlas) {
        let [x, y, width, height] = rectangle.cell;
        uploads.queue.profiled_write_texture(
            bevy::render::render_resource::TexelCopyTextureInfo {
                texture: &gpu.atlas_texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: TextureAspect::All,
            },
            &rectangle.rgba8,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
    }
    gpu.atlas = Arc::clone(&scene.atlas);
}

struct NametagPipelineSpecializer;

#[derive(Resource)]
struct NametagPipeline {
    variants: Variants<RenderPipeline, NametagPipelineSpecializer>,
    bind_group_layout: BindGroupLayoutDescriptor,
}

impl FromWorld for NametagPipeline {
    fn from_world(_world: &mut World) -> Self {
        let bind_group_layout = BindGroupLayoutDescriptor::new(
            "nametag bind group layout",
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
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let descriptor = RenderPipelineDescriptor {
            label: Some("nametag pipeline".into()),
            layout: vec![bind_group_layout.clone()],
            vertex: VertexState {
                shader: NAMETAG_SHADER_HANDLE,
                entry_point: Some("nametag_vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: NAMETAG_SHADER_HANDLE,
                entry_point: Some("nametag_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: Some(NAMETAG_BLEND),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(CompareFunction::Always),
                stencil: default(),
                bias: default(),
            }),
            ..default()
        };
        Self {
            variants: Variants::new(NametagPipelineSpecializer, descriptor),
            bind_group_layout,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
struct NametagPipelineKey {
    msaa: Msaa,
    hdr: bool,
    gamma_blend: bool,
    depth_tested: bool,
    text: bool,
}

impl Specializer<RenderPipeline> for NametagPipelineSpecializer {
    type Key = NametagPipelineKey;

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
        if key.gamma_blend {
            let fragment = descriptor.fragment.as_mut().unwrap();
            let target = fragment.targets[0].as_mut().unwrap();
            target.format = target.format.remove_srgb_suffix();
            fragment.shader_defs.push("NAMETAG_GAMMA_BLEND".into());
        }
        // Reverse-Z: nearer fragments carry larger depth.
        descriptor.depth_stencil.as_mut().unwrap().depth_compare = Some(if key.depth_tested {
            CompareFunction::GreaterEqual
        } else {
            CompareFunction::Always
        });
        let depth = descriptor.depth_stencil.as_mut().unwrap();
        depth.depth_write_enabled = Some(key.text && key.depth_tested);
        depth.bias = default();
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .blend = Some(NAMETAG_BLEND);
        Ok(key)
    }
}

fn prepare_nametag_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<NametagPipeline>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<NametagGpu>,
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
    let bind_group = render_device.create_bind_group(
        "nametag bind group",
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
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(&gpu.atlas_view),
            },
            BindGroupEntry {
                binding: 3,
                resource: BindingResource::Sampler(&gpu.sampler),
            },
        ],
    );
    gpu.bind_group = Some(bind_group);
    gpu.view_buffer_id = Some(view_buffer.id());
}

/// Camera identity and settings used to queue ordered world text.
type NametagQueueView = (
    Entity,
    &'static MainEntity,
    &'static ExtractedView,
    &'static bevy::render::camera::ExtractedCamera,
    &'static Msaa,
    Option<&'static render_scene::EnhancedRendering>,
);

fn queue_nametags(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<NametagPipeline>,
    gpu: Res<NametagGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<NametagQueueView>,
) {
    if gpu.total == 0 {
        return;
    }
    let functions = draw_functions.read();
    // Preserve complete back-to-front labels and their individual passes after world alpha.
    // At this magnitude one float ULP is 64; 128 avoids losing the batch ordering.
    for (view_entity, main_entity, view, extracted_camera, msaa, enhanced) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for (index, batch) in gpu.batches.iter().enumerate() {
            let Ok(pipeline_id) = pipeline.variants.specialize(
                &pipeline_cache,
                NametagPipelineKey {
                    msaa: *msaa,
                    hdr: extracted_camera.hdr,
                    gamma_blend: crate::chunk::transparent::gamma_pass::admitted(
                        extracted_camera.hdr,
                        *msaa,
                        enhanced.is_some(),
                    ),
                    depth_tested: batch.depth_tested,
                    text: batch.text,
                },
            ) else {
                continue;
            };
            crate::transparent_phase::add(
                phase,
                Transparent3d {
                    sorting_info:
                        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                    entity: (view_entity, *main_entity),
                    pipeline: pipeline_id,
                    draw_function: functions.id::<DrawNametags>(),
                    distance: 1.0e9 + index as f32 * 128.0,
                    // One phase entry per material batch; its records live in NametagGpu.
                    batch_range: phase_batch_range(index),
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                },
            );
        }
    }
}

type DrawNametags = (SetItemPipeline, SetNametagBindGroup<0>, DrawNametagRange);

pub(crate) fn draw_function(world: &World) -> Option<DrawFunctionId> {
    world
        .get_resource::<DrawFunctions<Transparent3d>>()?
        .read()
        .get_id::<DrawNametags>()
}

/// Enabled world filters postpone name text until the filtered scene is ready.
pub(crate) fn deferred_by_world_filter(
    enabled: bool,
    nametag: Option<DrawFunctionId>,
    draw: DrawFunctionId,
) -> bool {
    enabled && nametag == Some(draw)
}

struct SetNametagBindGroup<const I: usize>;

impl<P: PhaseItem, const I: usize> RenderCommand<P> for SetNametagBindGroup<I> {
    type Param = SRes<NametagGpu>;
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

struct DrawNametagRange;

impl<P: PhaseItem> RenderCommand<P> for DrawNametagRange {
    type Param = SRes<NametagGpu>;
    type ViewQuery = ();
    type ItemQuery = ();

    fn render<'w>(
        item: &P,
        _view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _item_query: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let Some(batch) = gpu
            .into_inner()
            .batches
            .get(item.batch_range().start as usize)
        else {
            return RenderCommandResult::Skip;
        };
        let records = &batch.records;
        pass.draw(records.start * 6..records.end * 6, 0..1);
        RenderCommandResult::Success
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for NametagPipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let gamma_blend =
            crate::chunk::transparent::gamma_pass::admitted(view.hdr, view.msaa, view.enhanced);
        for depth_tested in [false, true] {
            for text in [false, true] {
                ids.push(self.variants.specialize(
                    cache,
                    NametagPipelineKey {
                        msaa: view.msaa,
                        hdr: view.hdr,
                        gamma_blend,
                        depth_tested,
                        text,
                    },
                )?);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use render_model::NAMETAG_BLOCKS_PER_FONT_PIXEL;

    fn batches_of(records: &[NametagRecord]) -> Vec<NametagBatch> {
        let mut batches = Vec::new();
        record_batches(records, &mut batches);
        batches
    }

    #[test]
    fn records_reupload_on_the_first_frame_and_each_scene_change_only() {
        use bevy::ecs::system::RunSystemOnce;
        use bevy::prelude::{IntoSystem, System};
        let (device, queue) = wgpu::Device::noop(&Default::default());
        let mut world = World::new();
        world.insert_resource(RenderDevice::from(device));
        world.insert_resource(RenderQueue::new(queue));
        let plate = NametagRecord::default();
        let glyph = NametagRecord { text: 1, ..plate };
        world.insert_resource(NametagSceneResource(render_model::NametagScene {
            records: vec![plate, glyph],
            ..default()
        }));
        world.run_system_once(init_nametag_gpu).unwrap();
        let mut system = IntoSystem::into_system(prepare_nametags);
        system.initialize(&mut world);
        system.run((), &mut world).unwrap();
        assert_eq!(world.resource::<NametagGpu>().total, 2);
        assert_eq!(world.resource::<NametagGpu>().batches.len(), 2);

        // An unchanged scene leaves the uploaded state alone.
        world.resource_mut::<NametagGpu>().total = 0;
        system.run((), &mut world).unwrap();
        assert_eq!(world.resource::<NametagGpu>().total, 0);

        world.resource_mut::<NametagSceneResource>().records = vec![glyph];
        system.run((), &mut world).unwrap();
        let gpu = world.resource::<NametagGpu>();
        assert_eq!(gpu.total, 1);
        assert_eq!(
            gpu.batches,
            [NametagBatch {
                records: 0..1,
                depth_tested: false,
                text: true
            }]
        );
    }

    // Storage layout and shared geometry, with no glyph-to-plate separation.
    #[test]
    fn shader_mirrors_the_record_layout_scale_and_lift() {
        let source = include_str!("nametag.wesl");
        assert_eq!(std::mem::size_of::<NametagRecord>(), 80);
        assert!(source.contains("BLOCKS_PER_FONT_PIXEL: f32 = NAMETAG_SCALE_VALUE;"));
        assert!(source.contains("record.line_lift"));
        assert!(!source.contains("TEXT_LIFT_BLOCKS"));
    }

    #[test]
    fn records_batch_only_adjacent_identical_native_material_modes() {
        let plate = NametagRecord::default();
        let glyph = NametagRecord { text: 1, ..plate };
        let tested_plate = NametagRecord {
            depth_tested: 1,
            ..plate
        };
        let tested_glyph = NametagRecord {
            depth_tested: 1,
            ..glyph
        };
        let batches = batches_of(&[plate, glyph, glyph, tested_plate, tested_glyph]);
        assert_eq!(
            batches,
            [
                NametagBatch {
                    records: 0..1,
                    depth_tested: false,
                    text: false
                },
                NametagBatch {
                    records: 1..3,
                    depth_tested: false,
                    text: true
                },
                NametagBatch {
                    records: 3..4,
                    depth_tested: true,
                    text: false
                },
                NametagBatch {
                    records: 4..5,
                    depth_tested: true,
                    text: true
                },
            ]
        );
    }

    #[test]
    fn complete_labels_keep_their_interleaved_depth_passes_in_draw_order() {
        let plate = NametagRecord::default();
        let dim = NametagRecord { text: 1, ..plate };
        let full = NametagRecord {
            depth_tested: 1,
            ..dim
        };
        let batches = batches_of(&[plate, dim, full, plate, dim, full]);
        assert_eq!(
            batches
                .iter()
                .map(|batch| (batch.depth_tested, batch.text))
                .collect::<Vec<_>>(),
            [
                (false, false),
                (false, true),
                (true, true),
                (false, false),
                (false, true),
                (true, true)
            ]
        );
        assert_eq!(
            batches
                .into_iter()
                .flat_map(|batch| batch.records)
                .collect::<Vec<_>>(),
            [0, 1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn multiline_material_batches_visit_every_record_once_in_the_sorted_phase() {
        let plate = NametagRecord::default();
        let glyph = NametagRecord { text: 1, ..plate };
        let mut records = [
            plate, glyph, glyph, plate, glyph, plate, glyph, glyph, glyph,
        ];
        for record in &mut records[5..] {
            record.depth_tested = 1;
        }
        let batches = batches_of(&records);
        let mut phase_index = 0;
        let mut visited = Vec::new();
        while phase_index < batches.len() {
            let phase = phase_batch_range(phase_index);
            visited.extend(batches[phase.start as usize].records.clone());
            // SortedRenderPhase::render_range advances by the phase batch length.
            phase_index += phase.len();
        }
        assert_eq!(visited, (0..records.len() as u32).collect::<Vec<_>>());
    }

    #[test]
    fn encoded_blending_selects_the_matching_attachment_and_shader() {
        for hdr in [false, true] {
            for msaa in [Msaa::Off, Msaa::Sample4] {
                for enhanced in [false, true] {
                    let gamma_blend =
                        crate::chunk::transparent::gamma_pass::admitted(hdr, msaa, enhanced);
                    for text in [false, true] {
                        let mut descriptor = RenderPipelineDescriptor {
                            fragment: Some(FragmentState {
                                targets: vec![Some(ColorTargetState {
                                    format: render_scene::SCENE_COLOR_FORMAT,
                                    blend: Some(BlendState::ALPHA_BLENDING),
                                    write_mask: ColorWrites::ALL,
                                })],
                                ..default()
                            }),
                            depth_stencil: Some(DepthStencilState {
                                format: CORE_3D_DEPTH_FORMAT,
                                depth_write_enabled: Some(false),
                                depth_compare: Some(CompareFunction::Always),
                                stencil: default(),
                                bias: default(),
                            }),
                            ..default()
                        };
                        NametagPipelineSpecializer
                            .specialize(
                                NametagPipelineKey {
                                    msaa,
                                    hdr,
                                    gamma_blend,
                                    depth_tested: false,
                                    text,
                                },
                                &mut descriptor,
                            )
                            .unwrap();
                        let fragment = descriptor.fragment.unwrap();
                        let target = fragment.targets[0].as_ref().unwrap();
                        let base = if hdr {
                            render_scene::SCENE_HDR_FORMAT
                        } else {
                            render_scene::SCENE_COLOR_FORMAT
                        };
                        assert_eq!(
                            target.format,
                            if gamma_blend {
                                base.remove_srgb_suffix()
                            } else {
                                base
                            }
                        );
                        assert_eq!(target.blend, Some(NAMETAG_BLEND));
                        assert_eq!(
                            fragment.shader_defs.contains(&"NAMETAG_GAMMA_BLEND".into()),
                            gamma_blend
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn java_label_passes_write_depth_only_for_tested_text_without_bias() {
        for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
            for depth_tested in [false, true] {
                for text in [false, true] {
                    let mut descriptor = RenderPipelineDescriptor {
                        fragment: Some(FragmentState {
                            targets: vec![Some(ColorTargetState {
                                format: render_scene::SCENE_COLOR_FORMAT,
                                blend: Some(BlendState::ALPHA_BLENDING),
                                write_mask: ColorWrites::ALL,
                            })],
                            ..default()
                        }),
                        depth_stencil: Some(DepthStencilState {
                            format: CORE_3D_DEPTH_FORMAT,
                            depth_write_enabled: Some(false),
                            depth_compare: Some(CompareFunction::Always),
                            stencil: default(),
                            bias: default(),
                        }),
                        ..default()
                    };
                    NametagPipelineSpecializer
                        .specialize(
                            NametagPipelineKey {
                                msaa,
                                hdr: false,
                                gamma_blend: false,
                                depth_tested,
                                text,
                            },
                            &mut descriptor,
                        )
                        .unwrap();
                    let depth = descriptor.depth_stencil.unwrap();
                    assert_eq!(depth.depth_write_enabled, Some(text && depth_tested));
                    assert_eq!(
                        depth.depth_compare,
                        Some(if depth_tested {
                            CompareFunction::GreaterEqual
                        } else {
                            CompareFunction::Always
                        })
                    );
                    assert_eq!(depth.bias.constant, 0);
                    assert_eq!(depth.bias.slope_scale, 0.0);
                    assert_eq!(depth.bias.clamp, 0.0);
                }
            }
        }
    }

    #[test]
    fn screen_alignment_keeps_world_scale_anchor_and_multiline_lift() {
        use bevy::math::{EulerRot, Quat, Vec3};
        let record = NametagRecord {
            anchor: [1.0, 2.5, -5.0],
            rect: [0.0, 0.0, 1.0, 1.0],
            ..default()
        };
        for rotation in [
            Quat::IDENTITY,
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 0.3),
        ] {
            let corners = record
                .world_corners(rotation)
                .unwrap()
                .map(Vec3::from_array);
            assert_eq!(corners[0].to_array(), record.anchor);
            assert!(
                ((corners[1] - corners[0]).length() - NAMETAG_BLOCKS_PER_FONT_PIXEL).abs() < 1e-6
            );
            assert!(
                ((corners[3] - corners[0]).length() - NAMETAG_BLOCKS_PER_FONT_PIXEL).abs() < 1e-6
            );
            let lifted = NametagRecord {
                line_lift: 0.25,
                ..record
            }
            .world_corners(rotation)
            .unwrap()
            .map(Vec3::from_array);
            for (single, multi) in corners.into_iter().zip(lifted) {
                assert!((multi - single - Vec3::Y * 0.25).length() < 1e-6);
            }
        }
    }

    /// Mirrors the shader's view-uniform columns without the CPU's quaternion rotation.
    fn shader_world_corners(
        record: &NametagRecord,
        world_from_view: bevy::math::Mat4,
    ) -> [bevy::math::Vec3; 4] {
        use bevy::math::Vec3;
        let anchor = Vec3::from_array(record.anchor);
        let right = world_from_view.x_axis.truncate().normalize();
        let up = world_from_view.y_axis.truncate().normalize();
        let [x0, y0, x1, y1] = record.rect;
        [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|[x, y]| {
            anchor
                + Vec3::Y * record.line_lift
                + (right * x - up * y) * NAMETAG_BLOCKS_PER_FONT_PIXEL
        })
    }

    #[test]
    fn cpu_screen_alignment_matches_the_shader_for_camera_rotations_and_tag_positions() {
        use bevy::math::{EulerRot, Mat4, Quat, Vec3};
        let record = NametagRecord {
            anchor: [1.0, 2.5, -5.0],
            rect: [-21.0, -1.0, 21.0, 9.0],
            line_lift: 0.25,
            ..default()
        };
        for rotation in [
            Quat::IDENTITY,
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Quat::from_rotation_y(std::f32::consts::PI),
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
            Quat::from_euler(EulerRot::YXZ, 0.7, -0.4, 0.3),
            Quat::from_euler(EulerRot::YXZ, -2.1, 1.2, -0.6),
        ] {
            for eye in [Vec3::ZERO, Vec3::new(3.0, 2.0, -1.0)] {
                for scale in [Vec3::ONE, Vec3::new(2.0, 3.0, 4.0)] {
                    let world_from_view =
                        Mat4::from_scale_rotation_translation(scale, rotation, eye);
                    for offset in [
                        Vec3::ZERO,
                        Vec3::new(0.0, 0.0, -5.0),
                        Vec3::new(4.75, 0.0, -5.0),
                        Vec3::new(-4.75, 4.75, -5.0),
                        Vec3::new(4.75, -4.75, -5.0),
                    ] {
                        let tag = NametagRecord {
                            anchor: (eye + rotation * offset).to_array(),
                            ..record
                        };
                        let cpu = tag.world_corners(rotation).unwrap().map(Vec3::from_array);
                        for (cpu, shader) in cpu
                            .into_iter()
                            .zip(shader_world_corners(&tag, world_from_view))
                        {
                            assert!(
                                (cpu - shader).length() < 1e-6,
                                "rotation {rotation:?}, eye {eye:?}, scale {scale:?}, offset {offset:?}: CPU {cpu:?}, shader {shader:?}"
                            );
                        }
                    }
                }
            }
        }
    }
}
