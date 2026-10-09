//! Draws [`NametagScene`] in the transparent 3D phase: a see-through pass over everything and
//! a depth-tested pass, as vanilla's `name_tag` and `name_tag_depth_tested` materials do.
use std::{ops::Range, sync::Arc};

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
        extract_resource::{ExtractResource, ExtractResourcePlugin},
        render_phase::{
            AddRenderCommand, DrawFunctionId, DrawFunctions, PhaseItem, PhaseItemExtraIndex,
            RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass,
            ViewSortedRenderPhases,
        },
        render_resource::{
            AddressMode, BindGroup, BindGroupEntry, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, Buffer,
            BufferBindingType, BufferId, BufferInitDescriptor, BufferSize, BufferUsages, Canonical,
            ColorTargetState, ColorWrites, CompareFunction, DepthBiasState, DepthStencilState,
            Extent3d, FilterMode, FragmentState, PipelineCache, RenderPipeline,
            RenderPipelineDescriptor, Sampler, SamplerBindingType, SamplerDescriptor, ShaderStages,
            ShaderType, Specializer, SpecializerKey, TexelCopyBufferLayout, TextureAspect,
            TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType, TextureUsages,
            TextureView, TextureViewDescriptor, TextureViewDimension, Variants, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

use render_model::{
    MAX_NAMETAG_RECORDS, NAMETAG_ATLAS_SIDE, NAMETAG_TEXT_REVERSE_Z_BIAS, NametagAtlasRect,
    NametagRecord, NametagScene,
};

/// Main-world holder of this frame's [`NametagScene`], cloned into the render world.
#[derive(Resource, ExtractResource, Clone, Debug, Default, Deref, DerefMut)]
pub struct NametagSceneResource(pub NametagScene);

const NAMETAG_SHADER_HANDLE: Handle<Shader> = uuid_handle!("5d1f0c8e-2a47-4b93-9e6c-1f7a3b8d4c20");
const RECORD_BYTES: usize = std::mem::size_of::<NametagRecord>();

#[path = "nametag_render/shader.rs"]
mod shader;

#[derive(Debug, PartialEq)]
struct NametagBatch {
    records: Range<u32>,
    depth_tested: bool,
    text: bool,
}

fn record_batches(records: &[NametagRecord], see_through: usize) -> Vec<NametagBatch> {
    let mut batches: Vec<NametagBatch> = Vec::new();
    for (index, record) in records.iter().enumerate() {
        let depth_tested = index >= see_through;
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
    batches
}

/// A material batch consumes one Bevy phase entry, irrespective of its glyph count.
fn phase_batch_range(index: usize) -> Range<u32> {
    index as u32..index as u32 + 1
}

pub(crate) fn install_nametag_render(app: &mut App) {
    crate::pipeline_warmup::register::<NametagPipeline>(app);
    app.init_resource::<NametagSceneResource>()
        .add_plugins(ExtractResourcePlugin::<NametagSceneResource>::default());
    load_internal_asset!(
        app,
        NAMETAG_SHADER_HANDLE,
        "nametag.wgsl",
        shader::from_wgsl
    );
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
                    .run_if(crate::panorama::world_passes_enabled)
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

fn prepare_nametags(
    scene: Res<NametagSceneResource>,
    render_queue: Res<RenderQueue>,
    mut gpu: ResMut<NametagGpu>,
) {
    let total = scene.records.len().min(MAX_NAMETAG_RECORDS);
    gpu.total = total as u32;
    gpu.batches = record_batches(&scene.records[..total], scene.see_through.min(total));
    if total > 0 {
        render_queue.write_buffer(
            &gpu.record_buffer,
            0,
            bytemuck::cast_slice::<NametagRecord, u8>(&scene.records[..total]),
        );
    }
    if Arc::ptr_eq(&scene.atlas, &gpu.atlas) {
        return;
    }
    for rectangle in NametagAtlasRect::updates(&scene.atlas, &gpu.atlas) {
        let [x, y, width, height] = rectangle.cell;
        render_queue.write_texture(
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
                    format: TextureFormat::bevy_default(),
                    blend: Some(BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                ..default()
            }),
            depth_stencil: Some(DepthStencilState {
                format: CORE_3D_DEPTH_FORMAT,
                depth_write_enabled: false,
                depth_compare: CompareFunction::Always,
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
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        if key.gamma_blend {
            let fragment = descriptor.fragment.as_mut().unwrap();
            let target = fragment.targets[0].as_mut().unwrap();
            target.format = target.format.remove_srgb_suffix();
            fragment.shader_defs.push("NAMETAG_GAMMA_BLEND".into());
        }
        // Reverse-Z: nearer fragments carry larger depth.
        descriptor.depth_stencil.as_mut().unwrap().depth_compare = if key.depth_tested {
            CompareFunction::GreaterEqual
        } else {
            CompareFunction::Always
        };
        let depth = descriptor.depth_stencil.as_mut().unwrap();
        depth.depth_write_enabled = key.text;
        depth.bias = DepthBiasState {
            constant: if key.text && key.depth_tested {
                NAMETAG_TEXT_REVERSE_Z_BIAS
            } else {
                0
            },
            ..default()
        };
        let fragment = descriptor.fragment.as_mut().unwrap();
        if key.text && key.depth_tested {
            fragment.shader_defs.push("NAMETAG_ALPHA_TEST".into());
        }
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

fn queue_nametags(
    pipeline_cache: Res<PipelineCache>,
    mut pipeline: ResMut<NametagPipeline>,
    gpu: Res<NametagGpu>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &Msaa,
        Option<&crate::EnhancedRendering>,
    )>,
) {
    if gpu.total == 0 {
        return;
    }
    let functions = draw_functions.read();
    // Preserve record order (plate then glyphs, ordinary then sneaking) after world alpha.
    // At this magnitude one float ULP is 64; 128 avoids losing the batch ordering.
    for (view_entity, main_entity, view, msaa, enhanced) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for (index, batch) in gpu.batches.iter().enumerate() {
            let Ok(pipeline_id) = pipeline.variants.specialize(
                &pipeline_cache,
                NametagPipelineKey {
                    msaa: *msaa,
                    hdr: view.hdr,
                    gamma_blend: crate::chunk::transparent::gamma_pass::admitted(
                        view.hdr,
                        *msaa,
                        enhanced.is_some(),
                    ),
                    depth_tested: batch.depth_tested,
                    text: batch.text,
                },
            ) else {
                continue;
            };
            phase.add(Transparent3d {
                entity: (view_entity, *main_entity),
                pipeline: pipeline_id,
                draw_function: functions.id::<DrawNametags>(),
                distance: 1.0e9 + index as f32 * 128.0,
                // One phase entry per material batch; its records live in NametagGpu.
                batch_range: phase_batch_range(index),
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            });
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
    use render_model::{NAMETAG_ACOS_CUBIC, NAMETAG_ACOS_LINEAR, NAMETAG_BLOCKS_PER_FONT_PIXEL};

    // Storage layout and native geometry, with no invented glyph-to-plate separation.
    #[test]
    fn shader_mirrors_the_record_layout_scale_and_lift() {
        let source = include_str!("nametag.wgsl");
        assert_eq!(std::mem::size_of::<NametagRecord>(), 80);
        assert!(source.contains("BLOCKS_PER_FONT_PIXEL: f32 = NAMETAG_SCALE_VALUE;"));
        assert!(source.contains("native_acos(-z / horizontal)"));
        assert!(source.contains("record.line_lift"));
        assert!(!source.contains("TEXT_LIFT_BLOCKS"));
        assert!(
            source.find("texel.a < 0.5").unwrap() < source.find("color = color * texel").unwrap()
        );
    }

    #[test]
    fn records_batch_only_adjacent_identical_native_material_modes() {
        let plate = NametagRecord::default();
        let glyph = NametagRecord { text: 1, ..plate };
        let batches = record_batches(&[plate, glyph, glyph, plate, glyph], 3);
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
    fn multiline_material_batches_visit_every_record_once_in_the_sorted_phase() {
        let plate = NametagRecord::default();
        let glyph = NametagRecord { text: 1, ..plate };
        let records = [
            plate, glyph, glyph, plate, glyph, plate, glyph, glyph, glyph,
        ];
        let batches = record_batches(&records, 5);
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
                                    format: TextureFormat::bevy_default(),
                                    blend: Some(BlendState::ALPHA_BLENDING),
                                    write_mask: ColorWrites::ALL,
                                })],
                                ..default()
                            }),
                            depth_stencil: Some(DepthStencilState {
                                format: CORE_3D_DEPTH_FORMAT,
                                depth_write_enabled: false,
                                depth_compare: CompareFunction::Always,
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
                            ViewTarget::TEXTURE_FORMAT_HDR
                        } else {
                            TextureFormat::bevy_default()
                        };
                        assert_eq!(
                            target.format,
                            if gamma_blend {
                                base.remove_srgb_suffix()
                            } else {
                                base
                            }
                        );
                        assert_eq!(target.blend, Some(BlendState::ALPHA_BLENDING));
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
    fn four_native_material_modes_keep_depth_writes_alpha_test_and_bias() {
        for msaa in [Msaa::Off, Msaa::Sample2, Msaa::Sample4, Msaa::Sample8] {
            for depth_tested in [false, true] {
                for text in [false, true] {
                    let mut descriptor = RenderPipelineDescriptor {
                        fragment: Some(FragmentState {
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
                            depth_compare: CompareFunction::Always,
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
                    assert_eq!(depth.depth_write_enabled, text);
                    assert_eq!(
                        depth.depth_compare,
                        if depth_tested {
                            CompareFunction::GreaterEqual
                        } else {
                            CompareFunction::Always
                        }
                    );
                    assert_eq!(
                        depth.bias.constant,
                        if text && depth_tested {
                            NAMETAG_TEXT_REVERSE_Z_BIAS
                        } else {
                            0
                        }
                    );
                    assert_eq!(depth.bias.slope_scale, 0.0);
                    assert_eq!(depth.bias.clamp, 0.0);
                    assert_eq!(
                        descriptor
                            .fragment
                            .unwrap()
                            .shader_defs
                            .iter()
                            .any(|def| def == &"NAMETAG_ALPHA_TEST".into()),
                        text && depth_tested
                    );
                }
            }
        }
    }

    #[test]
    fn native_billboard_keeps_world_scale_and_uses_unlifted_cubic_eye_angles() {
        use bevy::math::Vec3;
        let record = NametagRecord {
            anchor: [1.0, 2.5, -5.0],
            rect: [0.0, 0.0, 1.0, 1.0],
            ..default()
        };
        for eye in [[0.0; 3], [0.0, 8.0, -4.0], [20.0, -5.0, 70.0]] {
            let corners = record.world_corners(eye).unwrap().map(Vec3::from_array);
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
            .world_corners(eye)
            .unwrap()
            .map(Vec3::from_array);
            for (single, multi) in corners.into_iter().zip(lifted) {
                assert!((multi - single - Vec3::Y * 0.25).length() < 1e-6);
            }
        }
        let diagonal = NametagRecord {
            anchor: [0.0; 3],
            ..record
        }
        .world_corners([1.0, 0.0, 1.0])
        .unwrap();
        let acos = |value: f32| {
            (NAMETAG_ACOS_CUBIC * value * value * value - NAMETAG_ACOS_LINEAR * value)
                + std::f32::consts::FRAC_PI_2
        };
        let expected_yaw = -acos(-std::f32::consts::FRAC_1_SQRT_2);
        assert!((diagonal[1][0] + expected_yaw.cos() * NAMETAG_BLOCKS_PER_FONT_PIXEL).abs() < 1e-7);
        assert!((diagonal[1][2] - expected_yaw.sin() * NAMETAG_BLOCKS_PER_FONT_PIXEL).abs() < 1e-7);
        assert!((expected_yaw + 3.0 * std::f32::consts::FRAC_PI_4).abs() > 0.02);
        assert!(record.world_corners(record.anchor).is_none());
        assert!(record.world_corners([f32::MAX; 3]).is_none());
    }
}
