use super::{
    SHADER,
    gpu::{CameraFrame, Gpu},
    materials::Materials,
};
use bevy::{
    core_pipeline::core_3d::{CORE_3D_DEPTH_FORMAT, Transparent3d},
    ecs::{
        query::ROQueryItem,
        system::{SystemParamItem, lifetimeless::SRes},
    },
    prelude::*,
    render::{
        render_phase::{
            PhaseItem, RenderCommand, RenderCommandResult, SetItemPipeline, TrackedRenderPass,
        },
        render_resource::*,
        renderer::RenderDevice,
    },
};
use mod_render::retained::mesh::GpuVertex;
use render_scene::mod_scene::retained::GpuInstance;

pub(super) struct Specialization;
#[derive(Clone, Copy, PartialEq, Eq, Hash, SpecializerKey)]
pub(super) struct Key {
    pub msaa: Msaa,
    pub hdr: bool,
    pub always: bool,
    pub additive: bool,
}
#[derive(Resource)]
pub(super) struct Pipeline {
    pub variants: Variants<RenderPipeline, Specialization>,
    pub layout: BindGroupLayoutDescriptor,
}
impl FromWorld for Pipeline {
    /// Starts with reverse-Z depth testing, no depth writes and premultiplied alpha.
    fn from_world(_world: &mut World) -> Self {
        let layout = layout();
        let descriptor = RenderPipelineDescriptor {
            label: Some("mod retained unlit".into()),
            layout: vec![layout.clone()],
            vertex: VertexState {
                shader: SHADER,
                entry_point: Some("vertex".into()),
                buffers: Vec::new(),
                ..default()
            },
            fragment: Some(FragmentState {
                shader: SHADER,
                entry_point: Some("material_fragment".into()),
                targets: vec![Some(ColorTargetState {
                    format: render_scene::SCENE_COLOR_FORMAT,
                    blend: Some(BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
            variants: Variants::new(Specialization, descriptor),
            layout,
        }
    }
}
/// Keeps the native storage layouts consistent with the shader's vec4 arrays.
pub(super) fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "mod retained layout",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX_FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<CameraFrame>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<GpuVertex>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 2,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(std::mem::size_of::<GpuInstance>() as u64),
                },
                count: None,
            },
        ],
    )
}
impl Specializer<RenderPipeline> for Specialization {
    type Key = Key;
    /// Specializes only bounded host state; guests never supply pipeline descriptors.
    fn specialize(
        &self,
        key: Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Key>, BevyError> {
        descriptor.multisample.count = key.msaa.samples();
        descriptor.depth_stencil = Some(depth(key));
        descriptor.fragment.as_mut().unwrap().targets = vec![Some(target(key))];
        Ok(key)
    }
}

/// Applies the same host-owned depth state to shared and procedural pipelines.
pub(super) fn depth(key: Key) -> DepthStencilState {
    DepthStencilState {
        format: CORE_3D_DEPTH_FORMAT,
        depth_write_enabled: Some(false),
        depth_compare: Some(if key.always {
            CompareFunction::Always
        } else {
            CompareFunction::GreaterEqual
        }),
        stencil: default(),
        bias: default(),
    }
}

/// Shares HDR format and premultiplied/additive blend semantics across material tiers.
pub(super) fn target(key: Key) -> ColorTargetState {
    ColorTargetState {
        format: if key.hdr {
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
        },
        blend: Some(if key.additive {
            BlendState {
                color: BlendComponent {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::One,
                    operation: BlendOperation::Add,
                },
                alpha: BlendComponent::OVER,
            }
        } else {
            BlendState::PREMULTIPLIED_ALPHA_BLENDING
        }),
        write_mask: ColorWrites::ALL,
    }
}

/// Rebuilds bindings only when a view or storage allocation changes.
pub(super) fn bindings(
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    pipeline: Res<Pipeline>,
    mut gpu: ResMut<Gpu>,
) {
    let Gpu { meshes, views, .. } = &mut *gpu;
    for view in views.values_mut() {
        view.bindings
            .retain(|handle, _| meshes.contains_key(handle));
        for batch in &view.batches {
            let Some(mesh) = meshes.get(&batch.geometry) else {
                continue;
            };
            view.bindings.entry(batch.geometry).or_insert_with(|| {
                device.create_bind_group(
                    "mod retained bindings",
                    &cache.get_bind_group_layout(&pipeline.layout),
                    &[
                        BindGroupEntry {
                            binding: 0,
                            resource: view.frame.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 1,
                            resource: mesh.buffer.as_entire_binding(),
                        },
                        BindGroupEntry {
                            binding: 2,
                            resource: view.buffer.as_entire_binding(),
                        },
                    ],
                )
            });
        }
    }
}

pub(super) type Draw = crate::gpu_timing::GpuDrawSpan<
    { render_scene::RuntimeStage::GpuModPrimitives as usize },
    (SetItemPipeline, DrawBatch),
>;
pub(super) struct DrawBatch;
impl RenderCommand<Transparent3d> for DrawBatch {
    type Param = (SRes<Gpu>, SRes<Materials>);
    type ViewQuery = ();
    type ItemQuery = ();
    /// Draws one compatible consecutive group from shared immutable geometry.
    fn render<'w>(
        item: &Transparent3d,
        _: ROQueryItem<'w, '_, Self::ViewQuery>,
        _: Option<ROQueryItem<'w, '_, Self::ItemQuery>>,
        gpu: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (gpu, materials) = gpu;
        let gpu = gpu.into_inner();
        let materials = materials.into_inner();
        let Some(view) = gpu.views.get(&item.entity()) else {
            return RenderCommandResult::Skip;
        };
        let Some(batch) = view.batches.get(item.batch_range().start as usize) else {
            return RenderCommandResult::Skip;
        };
        let (Some(mesh), Some(binding)) = (
            gpu.meshes.get(&batch.geometry),
            view.bindings.get(&batch.geometry),
        ) else {
            return RenderCommandResult::Skip;
        };
        if let Some(handle) = batch.material {
            let Some(pipeline) = materials.ready(handle, batch.key) else {
                return RenderCommandResult::Skip;
            };
            pass.set_render_pipeline(pipeline);
        }
        pass.set_bind_group(0, binding, &[]);
        pass.draw(0..mesh.count, batch.instances.clone());
        RenderCommandResult::Success
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for Pipeline {
    /// Queues the finite depth/blend variants through the same cache used by drawing.
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), BevyError> {
        for always in [false, true] {
            for additive in [false, true] {
                ids.push(self.variants.specialize(
                    cache,
                    Key {
                        msaa: view.msaa,
                        hdr: view.hdr,
                        always,
                        additive,
                    },
                )?);
            }
        }
        Ok(())
    }
}
