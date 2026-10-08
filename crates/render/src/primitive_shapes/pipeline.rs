//! One instanced draw per shared mesh and four material passes for retained text.
use super::{
    PrimitiveShapesScene, SHADER,
    gpu::{ACTOR_BYTES, INSTANCE_BYTES, ShapeGpu, TEXT_BYTES, is_text},
};
use bevy::{
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{
            SystemParamItem,
            lifetimeless::{Read, SRes},
        },
    },
    mesh::VertexBufferLayout,
    prelude::*,
    render::{
        globals::{GlobalsBuffer, GlobalsUniform},
        render_phase::{
            DrawFunctions, PhaseItem, PhaseItemExtraIndex, RenderCommand, RenderCommandResult,
            SetItemPipeline, TrackedRenderPass, ViewSortedRenderPhases,
        },
        render_resource::*,
        renderer::RenderDevice,
        sync_world::MainEntity,
        view::{ExtractedView, ViewTarget, ViewUniform, ViewUniformOffset, ViewUniforms},
    },
};

pub(super) const TEXT_FRONT_FACE: FrontFace = FrontFace::Cw;

struct ShapeSpecializer;
#[derive(Resource)]
pub(super) struct ShapePipeline {
    variants: Variants<RenderPipeline, ShapeSpecializer>,
    layout: BindGroupLayoutDescriptor,
}

/// Declares storage or uniform buffers with their GPU-visible minimum size.
fn buffer_entry(binding: u32, bytes: u64, uniform: bool, dynamic: bool) -> BindGroupLayoutEntry {
    BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX_FRAGMENT,
        ty: BindingType::Buffer {
            ty: if uniform {
                BufferBindingType::Uniform
            } else {
                BufferBindingType::Storage { read_only: true }
            },
            has_dynamic_offset: dynamic,
            min_binding_size: BufferSize::new(bytes),
        },
        count: None,
    }
}

impl FromWorld for ShapePipeline {
    /// Shares actor, instance, text and clock bindings between all mesh variants.
    fn from_world(_: &mut World) -> Self {
        let layout = BindGroupLayoutDescriptor::new(
            "primitive shape layout",
            &[
                buffer_entry(0, ViewUniform::min_size().get(), true, true),
                buffer_entry(1, INSTANCE_BYTES, false, false),
                buffer_entry(2, 16, true, false),
                buffer_entry(3, GlobalsUniform::min_size().get(), true, false),
                buffer_entry(4, ACTOR_BYTES, false, false),
                buffer_entry(5, TEXT_BYTES, false, false),
                BindGroupLayoutEntry {
                    binding: 6,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 7,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        );
        let mut descriptor = descriptor();
        descriptor.layout.push(layout.clone());
        Self {
            variants: Variants::new(ShapeSpecializer, descriptor),
            layout,
        }
    }
}

/// Builds the shared base descriptor before applying text material differences.
fn descriptor() -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("primitive shape pipeline".into()),
        vertex: VertexState {
            shader: SHADER,
            entry_point: Some("shape_vertex".into()),
            buffers: vec![VertexBufferLayout {
                array_stride: 16,
                step_mode: VertexStepMode::Vertex,
                attributes: vec![VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 0,
                    shader_location: 0,
                }],
            }],
            ..default()
        },
        fragment: Some(FragmentState {
            shader: SHADER,
            entry_point: Some("shape_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format: TextureFormat::bevy_default(),
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..default()
        }),
        primitive: PrimitiveState {
            topology: PrimitiveTopology::LineList,
            cull_mode: None,
            ..default()
        },
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
struct Key {
    msaa: Msaa,
    hdr: bool,
    gamma: bool,
    mode: u8,
}

impl Specializer<RenderPipeline> for ShapeSpecializer {
    type Key = Key;
    /// Text uses the same depth and alpha-test modes as the existing nametag renderer.
    fn specialize(
        &self,
        key: Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.targets[0].as_mut().unwrap().format = if key.hdr {
            ViewTarget::TEXTURE_FORMAT_HDR
        } else {
            TextureFormat::bevy_default()
        };
        if key.gamma {
            let target = fragment.targets[0].as_mut().unwrap();
            target.format = target.format.remove_srgb_suffix();
            fragment.shader_defs.push("SHAPE_GAMMA".into());
        }
        let depth = descriptor.depth_stencil.as_mut().unwrap();
        if key.mode != 0 {
            fragment.targets[0].as_mut().unwrap().blend = Some(BlendState::ALPHA_BLENDING);
            descriptor.primitive.topology = PrimitiveTopology::TriangleList;
            descriptor.primitive.front_face = TEXT_FRONT_FACE;
            descriptor.vertex.buffers.clear();
            descriptor.vertex.entry_point = Some("text_vertex".into());
            let depth_test = key.mode >= 3;
            let glyph = key.mode == 2 || key.mode == 4;
            depth.depth_compare = if depth_test {
                CompareFunction::GreaterEqual
            } else {
                CompareFunction::Always
            };
            depth.depth_write_enabled = glyph;
            if depth_test && glyph {
                depth.bias.constant = render_model::NAMETAG_TEXT_REVERSE_Z_BIAS;
                fragment.shader_defs.push("TEXT_ALPHA_TEST".into());
            }
            for (name, enabled) in [("TEXT_DEPTH", depth_test), ("TEXT_GLYPH", glyph)] {
                if enabled {
                    descriptor.vertex.shader_defs.push(name.into());
                    fragment.shader_defs.push(name.into());
                }
            }
        }
        Ok(key)
    }
}

/// Reuses bind groups until an arena or shared view/clock buffer changes identity.
pub(super) fn prepare_bind_groups(
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    pipeline: Res<ShapePipeline>,
    views: Res<ViewUniforms>,
    globals: Res<GlobalsBuffer>,
    mut gpu: ResMut<ShapeGpu>,
) {
    let (Some(view), Some(global)) = (views.uniforms.binding(), globals.buffer.binding()) else {
        return;
    };
    let view_id = views.uniforms.buffer().map(Buffer::id);
    let global_id = globals.buffer.buffer().map(Buffer::id);
    if gpu.view_id != view_id || gpu.global_id != global_id {
        for batch in &mut gpu.batches {
            batch.bind_groups.clear();
        }
        gpu.view_id = view_id;
        gpu.global_id = global_id;
    }
    let gpu = &mut *gpu;
    // The cached layout lookup clones its descriptor, so retained frames must not reach it.
    let mut layout = None;
    for batch in &mut gpu.batches {
        if !batch.bind_groups.is_empty() {
            continue;
        }
        let layout = layout.get_or_insert_with(|| cache.get_bind_group_layout(&pipeline.layout));
        for chunk in &batch.slots.chunks {
            batch.bind_groups.push(device.create_bind_group(
                "primitive shape bind group",
                layout,
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: view.clone(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: chunk.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: gpu.frame.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 3,
                        resource: global.clone(),
                    },
                    BindGroupEntry {
                        binding: 4,
                        resource: gpu.actors.chunks[0].as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 5,
                        resource: gpu.text.chunks[0].as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 6,
                        resource: BindingResource::TextureView(&gpu.atlas_view),
                    },
                    BindGroupEntry {
                        binding: 7,
                        resource: BindingResource::Sampler(&gpu.sampler),
                    },
                ],
            ));
        }
    }
}

/// Queues work by mesh variant, never by shape or text record.
pub(super) fn queue(
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<ShapePipeline>,
    scene: Res<PrimitiveShapesScene>,
    mut phases: ResMut<ViewSortedRenderPhases<Transparent3d>>,
    functions: Res<DrawFunctions<Transparent3d>>,
    views: Query<(
        Entity,
        &MainEntity,
        &ExtractedView,
        &Msaa,
        Option<&crate::EnhancedRendering>,
    )>,
) {
    let store = scene
        .store
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if store.batches.is_empty() {
        return;
    }
    let functions = functions.read();
    for (entity, main, view, msaa, enhanced) in &views {
        let Some(phase) = phases.get_mut(&view.retained_view_entity) else {
            continue;
        };
        for (index, batch) in store.batches.iter().enumerate() {
            if batch.instances.is_empty() {
                continue;
            }
            let modes = if is_text(batch.key) { 1..5 } else { 0..1 };
            for mode in modes {
                let Ok(id) = pipeline.variants.specialize(
                    &cache,
                    Key {
                        msaa: *msaa,
                        hdr: view.hdr,
                        gamma: crate::chunk::transparent::gamma_pass::admitted(
                            view.hdr,
                            *msaa,
                            enhanced.is_some(),
                        ),
                        mode,
                    },
                ) else {
                    continue;
                };
                let encoded = index as u32 * 5 + u32::from(mode);
                phase.add(Transparent3d {
                    entity: (entity, *main),
                    pipeline: id,
                    draw_function: functions.id::<DrawShapes>(),
                    distance: 1.1e9 + mode as f32 * 128.0,
                    batch_range: encoded..encoded + 1,
                    extra_index: PhaseItemExtraIndex::None,
                    indexed: false,
                });
            }
        }
    }
}

pub(super) type DrawShapes = (SetItemPipeline, DrawShapeBatch);
pub(super) struct DrawShapeBatch;
impl<P: PhaseItem> RenderCommand<P> for DrawShapeBatch {
    type Param = SRes<ShapeGpu>;
    type ViewQuery = Read<ViewUniformOffset>;
    type ItemQuery = ();
    /// Draws each arena chunk with its own binding; visibility is decided on the GPU.
    fn render<'w>(
        item: &P,
        view: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let gpu = gpu.into_inner();
        let Some(batch) = gpu.batches.get(item.batch_range().start as usize / 5) else {
            return RenderCommandResult::Skip;
        };
        if batch.bind_groups.is_empty() {
            return RenderCommandResult::Skip;
        }
        if is_text(batch.key) {
            pass.set_bind_group(0, &batch.bind_groups[0], &[view.offset]);
            pass.draw(0..6, 0..gpu.text_count);
            return RenderCommandResult::Success;
        }
        pass.set_vertex_buffer(0, batch.mesh.slice(..));
        for (index, group) in batch.bind_groups.iter().enumerate() {
            pass.set_bind_group(0, group, &[view.offset]);
            let instances = batch.slots.chunk_len(index, batch.instances as usize);
            pass.draw(0..batch.vertices, 0..instances);
        }
        RenderCommandResult::Success
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;

impl crate::pipeline_warmup::PrewarmPipelines for ShapePipeline {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        let gamma =
            crate::chunk::transparent::gamma_pass::admitted(view.hdr, view.msaa, view.enhanced);
        // Mesh batches draw mode 0; text batches draw modes 1 through 4.
        for mode in 0..5 {
            ids.push(self.variants.specialize(
                cache,
                Key {
                    msaa: view.msaa,
                    hdr: view.hdr,
                    gamma,
                    mode,
                },
            )?);
        }
        Ok(())
    }
}
