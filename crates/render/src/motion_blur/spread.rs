//! A compacted pixel sweep supplies depth-tested object velocities outside current silhouettes.

use super::history::ExposureUniform;
use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderContext, RenderDevice},
    },
};
use std::mem::size_of;

const FORMAT: TextureFormat = TextureFormat::Rgba32Float;
const DEPTH_FORMAT: TextureFormat = bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT;
const COMPACT_SIDE: u32 = 8;

/// Shares the exposure constants and original per-sample motion classification with the sweep.
pub(super) fn source(multisampled: bool, compute: bool) -> String {
    let mut source = super::shader_source(include_str!("../motion_blur.wgsl"), multisampled);
    source.push_str(include_str!("spread.wgsl"));
    source = source.replace("ACTOR_COMPACTION_SIDE", &COMPACT_SIDE.to_string());
    if multisampled {
        source = source.replace(
            "return textureLoad(depth, pixel, 0);",
            r#"var z = 0.0;
    for (var sample = 0u; sample < textureNumSamples(depth); sample++) {
        z = max(z, textureLoad(depth, pixel, i32(sample)));
    }
    return z;"#,
        );
        // Only actor-covered samples establish the source actor's depth.
        let covered = source.find("fn covered_depth").unwrap();
        let compact = source.find("// Compact only").unwrap();
        source.replace_range(
            covered..compact,
            r#"fn covered_depth(pixel: vec2<i32>) -> f32 {
    var z = 0.0;
    for (var sample = 0u; sample < textureNumSamples(depth); sample++) {
        if textureLoad(actor_velocity, pixel, i32(sample)).z >= 0.5 {
            z = max(z, textureLoad(depth, pixel, i32(sample)));
        }
    }
    return z;
}

"#,
        );
    }
    if compute {
        source.truncate(source.find("struct SweepOutput").unwrap());
    } else {
        source = source.replace(
            "var<storage, read_write> positions",
            "var<storage, read> positions",
        );
        // The render module does not expose a writable-storage compute entry point.
        let start = source.find("@compute").unwrap();
        let end = source.find("struct SweepOutput").unwrap();
        source.replace_range(start..end, "");
    }
    source
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_sweep_shader_modes_validate_with_default_device_limits() {
        for multi in [false, true] {
            for compute in [false, true] {
                let source = source(multi, compute);
                let module = naga::front::wgsl::parse_str(&source)
                    .unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
                naga::valid::Validator::new(
                    naga::valid::ValidationFlags::all(),
                    naga::valid::Capabilities::all(),
                )
                .validate(&module)
                .unwrap();
            }
        }
        let (device, _) = wgpu::Device::noop(&Default::default());
        let _pipelines = Pipelines::new(&RenderDevice::from(device));
    }
}

/// Compaction compiles at renderer startup; render variants participate in loading readiness.
pub(super) struct Pipelines {
    compute: [wgpu::ComputePipeline; 2],
    layouts: [BindGroupLayoutDescriptor; 2],
    variants: [Option<[CachedRenderPipelineId; 2]>; 2],
}

impl Pipelines {
    /// Builds the two texture-reading compute modes before any world can render.
    pub fn new(device: &RenderDevice) -> Self {
        let compute = [false, true].map(|multi| {
            let mut entries = layout(multi).entries.to_vec();
            for entry in &mut entries {
                entry.visibility = ShaderStages::COMPUTE;
                if entry.binding == 6 {
                    entry.ty = BindingType::Buffer {
                        ty: BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: BufferSize::new(4),
                    };
                }
            }
            entries.push(BindGroupLayoutEntry {
                binding: 7,
                visibility: ShaderStages::COMPUTE,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(
                        size_of::<wgpu::util::DrawIndirectArgs>() as u64
                    ),
                },
                count: None,
            });
            let layout =
                device
                    .wgpu_device()
                    .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                        label: Some("actor compaction"),
                        entries: &entries,
                    });
            let pipeline_layout =
                device
                    .wgpu_device()
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("actor compaction"),
                        bind_group_layouts: &[Some(&layout)],
                        immediate_size: 0,
                    });
            let shader = device
                .wgpu_device()
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("actor exposure compaction"),
                    source: wgpu::ShaderSource::Wgsl(source(multi, true).into()),
                });
            device
                .wgpu_device()
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("actor exposure compaction"),
                    layout: Some(&pipeline_layout),
                    module: &shader,
                    entry_point: Some("compact"),
                    compilation_options: Default::default(),
                    cache: None,
                })
        });
        let layouts = [false, true].map(|multi| layout(multi));
        Self {
            compute,
            layouts,
            variants: [None; 2],
        }
    }

    /// Requests the same seed and sweep descriptors used by prepared views.
    pub fn prewarm(&mut self, cache: &PipelineCache, multi: bool) -> [CachedRenderPipelineId; 2] {
        let index = usize::from(multi);
        *self.variants[index].get_or_insert_with(|| {
            let shader = if multi {
                super::SPREAD_MULTI
            } else {
                super::SPREAD_SINGLE
            };
            [false, true].map(|sweep| {
                cache.queue_render_pipeline(RenderPipelineDescriptor {
                    label: Some(
                        if sweep {
                            "swept actor motion"
                        } else {
                            "actor exposure scene depth"
                        }
                        .into(),
                    ),
                    layout: vec![self.layouts[index].clone()],
                    vertex: VertexState {
                        shader: shader.clone(),
                        entry_point: Some(if sweep { "sweep_vertex" } else { "vertex" }.into()),
                        ..default()
                    },
                    fragment: Some(FragmentState {
                        shader: shader.clone(),
                        entry_point: Some(
                            if sweep {
                                "sweep_fragment"
                            } else {
                                "seed_depth"
                            }
                            .into(),
                        ),
                        targets: if sweep {
                            vec![Some(ColorTargetState {
                                format: FORMAT,
                                blend: None,
                                write_mask: ColorWrites::ALL,
                            })]
                        } else {
                            Vec::new()
                        },
                        ..default()
                    }),
                    depth_stencil: Some(DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(true),
                        depth_compare: Some(if sweep {
                            CompareFunction::GreaterEqual
                        } else {
                            CompareFunction::Always
                        }),
                        stencil: default(),
                        bias: default(),
                    }),
                    ..default()
                })
            })
        })
    }
}

/// Read-only bindings for seed and sweep share the prepared view's original depth and motion.
fn layout(multi: bool) -> BindGroupLayoutDescriptor {
    let texture = |binding, sample_type| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX_FRAGMENT,
        ty: BindingType::Texture {
            sample_type,
            view_dimension: TextureViewDimension::D2,
            multisampled: multi,
        },
        count: None,
    };
    let buffer = |binding, ty, bytes| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX_FRAGMENT,
        ty: BindingType::Buffer {
            ty,
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(bytes),
        },
        count: None,
    };
    BindGroupLayoutDescriptor::new(
        "actor exposure sweep",
        &[
            texture(2, TextureSampleType::Depth),
            buffer(
                3,
                BufferBindingType::Uniform,
                size_of::<ExposureUniform>() as u64,
            ),
            texture(4, TextureSampleType::Float { filterable: false }),
            buffer(
                6,
                BufferBindingType::Storage { read_only: true },
                size_of::<u32>() as u64,
            ),
        ],
    )
}

/// An enabled actor view retains its allocations and bindings across unchanged frames.
pub(super) struct View {
    pub view: TextureView,
    _texture: Texture,
    depth: Texture,
    depth_view: TextureView,
    _positions: Buffer,
    indirect: Buffer,
    compute_binding: wgpu::BindGroup,
    render_binding: BindGroup,
    source_motion: TextureViewId,
    source_depth: TextureViewId,
    multi: bool,
}

impl View {
    /// Reuses resources until the source actor mask or depth attachment changes.
    pub fn matches(&self, motion: &TextureView, depth: &TextureView) -> bool {
        (self.source_motion, self.source_depth) == (motion.id(), depth.id())
    }

    /// Allocates no resources for actorless or disabled views.
    pub fn new(
        device: &RenderDevice,
        pipelines: &Pipelines,
        cache: &PipelineCache,
        motion: &TextureView,
        source_depth: &TextureView,
        uniform: &Buffer,
    ) -> Option<Self> {
        let size = motion.texture().size();
        let position_bytes =
            u64::from(size.width) * u64::from(size.height) * size_of::<u32>() as u64;
        if position_bytes > u64::from(device.limits().max_storage_buffer_binding_size) {
            return None;
        }
        let multi = motion.texture().sample_count() > 1;
        let index = usize::from(multi);
        let texture = |format, usage| {
            device.create_texture(&TextureDescriptor {
                label: Some("swept actor exposure"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(
            FORMAT,
            TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
        );
        let depth = texture(DEPTH_FORMAT, TextureUsages::RENDER_ATTACHMENT);
        let positions = device.create_buffer(&BufferDescriptor {
            label: Some("compacted actor pixels"),
            size: position_bytes,
            usage: BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let indirect = device.create_buffer(&BufferDescriptor {
            label: Some("swept actor indirect draw"),
            size: size_of::<wgpu::util::DrawIndirectArgs>() as u64,
            usage: BufferUsages::STORAGE | BufferUsages::INDIRECT | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let entries = [
            BindGroupEntry {
                binding: 2,
                resource: BindingResource::TextureView(source_depth),
            },
            BindGroupEntry {
                binding: 3,
                resource: uniform.as_entire_binding(),
            },
            BindGroupEntry {
                binding: 4,
                resource: BindingResource::TextureView(motion),
            },
            BindGroupEntry {
                binding: 6,
                resource: positions.as_entire_binding(),
            },
        ];
        let render_binding = device.create_bind_group(
            "swept actor exposure",
            &cache.get_bind_group_layout(&pipelines.layouts[index]),
            &entries,
        );
        let compute_entries = [
            entries[0].clone(),
            entries[1].clone(),
            entries[2].clone(),
            entries[3].clone(),
            BindGroupEntry {
                binding: 7,
                resource: indirect.as_entire_binding(),
            },
        ];
        let compute_binding = device
            .wgpu_device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("compacted actor exposure"),
                layout: &pipelines.compute[index].get_bind_group_layout(0),
                entries: &compute_entries,
            });
        Some(Self {
            view: color.create_view(&default()),
            _texture: color,
            depth_view: depth.create_view(&default()),
            depth,
            _positions: positions,
            indirect,
            compute_binding,
            render_binding,
            source_motion: motion.id(),
            source_depth: source_depth.id(),
            multi,
        })
    }

    /// Whether exposure can use both loading-prewarmed render pipelines.
    pub fn ready(&self, pipelines: &Pipelines, cache: &PipelineCache) -> bool {
        pipelines.variants[usize::from(self.multi)].is_some_and(|ids| {
            ids.into_iter()
                .all(|id| cache.get_render_pipeline(id).is_some())
        })
    }

    /// Compacts the actual mask, seeds scene depth, and fills only unobstructed swept pixels.
    pub fn draw(
        &self,
        pipelines: &Pipelines,
        cache: &PipelineCache,
        viewport: UVec4,
        context: &mut RenderContext,
        world: &World,
    ) -> bool {
        let Some(ids) = pipelines.variants[usize::from(self.multi)] else {
            return false;
        };
        let (Some(seed), Some(sweep)) = (
            cache.get_render_pipeline(ids[0]),
            cache.get_render_pipeline(ids[1]),
        ) else {
            return false;
        };
        let Some(rect) = crate::render_bounds::scissor(
            render_model::UiScissor::new(viewport.x, viewport.y, viewport.z, viewport.w),
            [self.depth.width(), self.depth.height()],
        ) else {
            return false;
        };
        context
            .command_encoder()
            .clear_buffer(&self.indirect, 0, None);
        crate::gpu_timing::timed(
            world,
            context,
            render_scene::RuntimeStage::GpuPost,
            |context| {
                let mut pass =
                    context
                        .command_encoder()
                        .begin_compute_pass(&wgpu::ComputePassDescriptor {
                            label: Some("compact moving actor coverage"),
                            timestamp_writes: None,
                        });
                pass.set_pipeline(&pipelines.compute[usize::from(self.multi)]);
                pass.set_bind_group(0, &self.compute_binding, &[]);
                pass.dispatch_workgroups(
                    viewport.z.div_ceil(COMPACT_SIDE),
                    viewport.w.div_ceil(COMPACT_SIDE),
                    1,
                );
            },
        );
        let timestamp = || {
            crate::gpu_timing::render_pass_timestamps(world, render_scene::RuntimeStage::GpuPost)
        };
        {
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("seed actor exposure depth"),
                color_attachments: &[],
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(0.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timestamp(),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.set_render_pipeline(seed);
            pass.set_bind_group(0, &self.render_binding, &[]);
            pass.draw(0..3, 0..1);
        }
        {
            let attachments = [Some(RenderPassColorAttachment {
                view: &self.view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })];
            let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
                label: Some("sweep depth-tested actor motion"),
                color_attachments: &attachments,
                depth_stencil_attachment: Some(RenderPassDepthStencilAttachment {
                    view: &self.depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: timestamp(),
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_scissor_rect(rect.x, rect.y, rect.width, rect.height);
            pass.set_render_pipeline(sweep);
            pass.set_bind_group(0, &self.render_binding, &[]);
            pass.draw_indirect(&self.indirect, 0);
        }
        true
    }
}
