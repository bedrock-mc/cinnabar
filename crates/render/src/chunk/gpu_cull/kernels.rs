//! Cull and depth-pyramid compute kernels on raw wgpu, so fixture tests drive the production code.

use std::{borrow::Cow, num::NonZeroU64};

use super::storage::{CullStorage, OcclusionStorage};
use wgpu::util::DeviceExt;

use super::model::{
    CULL_STORAGE_BINDINGS, CullPhase, CullViewUniform, DECISION_CANDIDATE, DECISION_CUTOUT_SHIFT,
    DECISION_DRAWS, DECISION_HIDDEN, DECISION_LIQUID_SHIFT, DECISION_MODEL_SHIFT,
    DECISION_SOLID_MASK, FRUSTUM_ABSOLUTE_SLACK, FRUSTUM_RELATIVE_SLACK, HIZ_PADDING, PHASE_COUNT,
    PYRAMID_WORKGROUP, cull_workgroup, group_count,
};

const PYRAMID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;

/// The cull kernel source with its Rust-owned constants substituted.
pub fn cull_shader_source(workgroup: u32) -> String {
    let float = |value: f32| format!("{value:?}f");
    include_str!("cull.wgsl")
        .replace("CULL_WORKGROUP", &format!("{workgroup}u"))
        .replace("CULL_DECISION_DRAWS", &format!("{DECISION_DRAWS}u"))
        .replace("CULL_DECISION_CANDIDATE", &format!("{DECISION_CANDIDATE}u"))
        .replace("CULL_DECISION_HIDDEN", &format!("{DECISION_HIDDEN}u"))
        .replace(
            "CULL_DECISION_SOLID_MASK",
            &format!("{DECISION_SOLID_MASK}u"),
        )
        .replace(
            "CULL_DECISION_CUTOUT_SHIFT",
            &format!("{DECISION_CUTOUT_SHIFT}u"),
        )
        .replace(
            "CULL_DECISION_MODEL_SHIFT",
            &format!("{DECISION_MODEL_SHIFT}u"),
        )
        .replace(
            "CULL_DECISION_LIQUID_SHIFT",
            &format!("{DECISION_LIQUID_SHIFT}u"),
        )
        .replace("CULL_SIDE", &format!("{}i", world::SUB_CHUNK_SIDE))
        .replace("CULL_HIZ_PADDING", &float(HIZ_PADDING))
        .replace("CULL_RELATIVE_SLACK", &float(FRUSTUM_RELATIVE_SLACK))
        .replace("CULL_ABSOLUTE_SLACK", &float(FRUSTUM_ABSOLUTE_SLACK))
}

pub fn pyramid_shader_source() -> &'static str {
    include_str!("hiz.wgsl")
}

/// Mip sizes for a depth target: level 0 is half the next power of two on each axis, so
/// every level halves exactly, matches the hardware mip chain, and drops no edge pixel.
pub fn pyramid_sizes(depth_size: [u32; 2]) -> Vec<[u32; 2]> {
    let mut sizes = vec![depth_size.map(|side| side.max(2).next_power_of_two() / 2)];
    while let Some(&[width, height]) = sizes.last()
        && (width > 1 || height > 1)
    {
        sizes.push([(width / 2).max(1), (height / 2).max(1)]);
    }
    sizes
}

/// Texels of each level that cover at least one depth pixel, `ceil(depth / 2^(level + 1))` per
/// axis. The cull reads no other texel, so only these are built.
pub fn pyramid_extents(depth_size: [u32; 2]) -> Vec<[u32; 2]> {
    let mut extent = depth_size.map(|side| side.max(1));
    pyramid_sizes(depth_size)
        .iter()
        .map(|_| {
            extent = extent.map(|side| side.div_ceil(2));
            extent
        })
        .collect()
}

/// Reverse-Z farthest-depth pyramid over a depth target of `depth_size` pixels.
pub struct HizPyramid {
    pub depth_size: [u32; 2],
    #[allow(dead_code, reason = "fixture tests read the pyramid back")]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    mips: Vec<wgpu::TextureView>,
    sizes: Vec<[u32; 2]>,
    extents: Vec<[u32; 2]>,
    /// Per level, the `HizBounds` uniform of `hiz.wgsl`: source last texel, covered extent.
    bounds: Vec<wgpu::Buffer>,
}

impl HizPyramid {
    pub fn new(device: &wgpu::Device, depth_size: [u32; 2]) -> Self {
        let sizes = pyramid_sizes(depth_size);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain hi-z pyramid"),
            size: wgpu::Extent3d {
                width: sizes[0][0],
                height: sizes[0][1],
                depth_or_array_layers: 1,
            },
            mip_level_count: sizes.len() as u32,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PYRAMID_FORMAT,
            usage: wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let mips = (0..sizes.len() as u32)
            .map(|level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        let extents = pyramid_extents(depth_size);
        let bounds = extents
            .iter()
            .enumerate()
            .map(|(level, extent)| {
                let source = if level == 0 {
                    depth_size
                } else {
                    extents[level - 1]
                };
                let words = [
                    source[0].saturating_sub(1),
                    source[1].saturating_sub(1),
                    extent[0],
                    extent[1],
                ];
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain hi-z level bounds"),
                    contents: bytemuck::cast_slice(&words),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
            })
            .collect();
        Self {
            depth_size,
            view: texture.create_view(&Default::default()),
            texture,
            mips,
            sizes,
            extents,
            bounds,
        }
    }

    pub fn mip_count(&self) -> u32 {
        self.sizes.len() as u32
    }

    #[allow(dead_code, reason = "fixture tests read the pyramid back")]
    pub fn size(&self, level: u32) -> [u32; 2] {
        self.sizes[level as usize]
    }
}

/// Bind groups that seed a pyramid from one depth view and reduce its mips.
pub struct PyramidBindings {
    multisampled: bool,
    seed: wgpu::BindGroup,
    reduce: Vec<wgpu::BindGroup>,
}

pub struct CullKernels {
    pub workgroup: u32,
    layout: wgpu::BindGroupLayout,
    count: wgpu::ComputePipeline,
    scan: wgpu::ComputePipeline,
    emit: wgpu::ComputePipeline,
    seed: wgpu::ComputePipeline,
    seed_multisampled: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
    occlusion: wgpu::ComputePipeline,
    blank_pyramid: wgpu::TextureView,
}

impl CullKernels {
    /// Creates kernels only after admission has established compatible device limits.
    pub fn new(device: &wgpu::Device) -> Self {
        let workgroup =
            cull_workgroup(&device.limits()).expect("cull kernels require compatible limits");
        let storage = |binding, read_only| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let mut entries = vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: NonZeroU64::new(std::mem::size_of::<CullViewUniform>() as u64),
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 8,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ];
        entries.extend(CULL_STORAGE_BINDINGS.map(|binding| storage(binding, binding < 3)));
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain cull layout"),
            entries: &entries,
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain cull pipeline layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = |label, source: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
            })
        };
        let cull = module("terrain cull", cull_shader_source(workgroup));
        let pyramid = module("terrain hi-z", pyramid_shader_source().to_owned());
        let pipeline = |module, layout: Option<&wgpu::PipelineLayout>, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout,
                module,
                entry_point: Some(entry),
                // Every kernel writes its own workgroup slots before any barrier reads them, and
                // the occlusion bits are cleared explicitly. Zero-filling the workgroup scratch
                // anyway kept FXC optimising for seconds per kernel at every DX12 launch.
                compilation_options: wgpu::PipelineCompilationOptions {
                    zero_initialize_workgroup_memory: false,
                    ..Default::default()
                },
                cache: None,
            })
        };
        let blank = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("terrain hi-z placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: PYRAMID_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            workgroup,
            count: pipeline(&cull, Some(&pipeline_layout), "cull_count"),
            scan: pipeline(&cull, Some(&pipeline_layout), "cull_scan"),
            emit: pipeline(&cull, Some(&pipeline_layout), "cull_emit"),
            seed: pipeline(&pyramid, None, "hiz_seed"),
            seed_multisampled: pipeline(&pyramid, None, "hiz_seed_multisampled"),
            reduce: pipeline(&pyramid, None, "hiz_reduce"),
            occlusion: pipeline(&cull, None, "cull_occlusion"),
            blank_pyramid: blank.create_view(&Default::default()),
            layout,
        }
    }

    /// One bind group per phase; the late one tests `pyramid` when there is one.
    pub fn bind_groups(
        &self,
        device: &wgpu::Device,
        storage: &CullStorage,
        pyramid: Option<&HizPyramid>,
    ) -> [wgpu::BindGroup; PHASE_COUNT] {
        CullPhase::ALL.map(|phase| {
            let hiz = match (phase, pyramid) {
                (CullPhase::Late, Some(pyramid)) => &pyramid.view,
                _ => &self.blank_pyramid,
            };
            let buffers = [
                &storage.uniforms[phase as usize],
                &storage.records,
                &storage.enabled,
                &storage.history,
                &storage.record_draws,
                &storage.group_sums,
                &storage.args,
                &storage.draw_counts,
            ];
            let mut entries = buffers
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>();
            entries.push(wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(hiz),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 10,
                resource: storage.draw_offsets.as_entire_binding(),
            });
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("terrain cull bindings"),
                layout: &self.layout,
                entries: &entries,
            })
        })
    }

    /// Decides, offsets and writes one phase's draws for `slots` records.
    pub fn encode_cull(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        slots: u32,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("terrain cull"),
            timestamp_writes,
        });
        pass.set_bind_group(0, bind_group, &[]);
        let groups = group_count(slots, self.workgroup);
        for (pipeline, workgroups) in [(&self.count, groups), (&self.scan, 1), (&self.emit, groups)]
        {
            if workgroups != 0 {
                pass.set_pipeline(pipeline);
                pass.dispatch_workgroups(workgroups, 1, 1);
            }
        }
    }

    /// Binds the occlusion kernel to `storage` and the pyramid it tests.
    pub fn occlusion_bind_group(
        &self,
        device: &wgpu::Device,
        storage: &OcclusionStorage,
        pyramid: &HizPyramid,
    ) -> wgpu::BindGroup {
        let entries = [
            (0, storage.uniform.as_entire_binding()),
            (1, storage.records.as_entire_binding()),
            (8, wgpu::BindingResource::TextureView(&pyramid.view)),
            (9, storage.occluded.as_entire_binding()),
        ]
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain occlusion bindings"),
            layout: &self.occlusion.get_bind_group_layout(0),
            entries: &entries,
        })
    }

    /// Writes the occluded bit of every slot below `slots`.
    pub fn encode_occlusion(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        bind_group: &wgpu::BindGroup,
        slots: u32,
    ) {
        let groups = group_count(slots, self.workgroup);
        if groups == 0 {
            return;
        }
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("terrain occlusion"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.occlusion);
        pass.set_bind_group(0, bind_group, &[]);
        pass.dispatch_workgroups(groups, 1, 1);
    }

    pub fn pyramid_bindings(
        &self,
        device: &wgpu::Device,
        depth: &wgpu::TextureView,
        multisampled: bool,
        pyramid: &HizPyramid,
    ) -> PyramidBindings {
        let group = |pipeline: &wgpu::ComputePipeline, entries: &[wgpu::BindGroupEntry]| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("terrain hi-z bindings"),
                layout: &pipeline.get_bind_group_layout(0),
                entries,
            })
        };
        let seed_pipeline = if multisampled {
            &self.seed_multisampled
        } else {
            &self.seed
        };
        let seed = group(
            seed_pipeline,
            &[
                wgpu::BindGroupEntry {
                    binding: if multisampled { 1 } else { 0 },
                    resource: wgpu::BindingResource::TextureView(depth),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&pyramid.mips[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: pyramid.bounds[0].as_entire_binding(),
                },
            ],
        );
        let reduce = pyramid
            .mips
            .windows(2)
            .zip(&pyramid.bounds[1..])
            .map(|(pair, bounds)| {
                group(
                    &self.reduce,
                    &[
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(&pair[0]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureView(&pair[1]),
                        },
                        wgpu::BindGroupEntry {
                            binding: 4,
                            resource: bounds.as_entire_binding(),
                        },
                    ],
                )
            })
            .collect();
        PyramidBindings {
            multisampled,
            seed,
            reduce,
        }
    }

    /// Builds all active Hi-Z levels in one pass with an optional timestamp pair.
    pub fn encode_pyramid(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pyramid: &HizPyramid,
        bindings: &PyramidBindings,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("terrain hi-z"),
            timestamp_writes,
        });
        let seed = if bindings.multisampled {
            &self.seed_multisampled
        } else {
            &self.seed
        };
        let levels = std::iter::once((seed, &bindings.seed))
            .chain(bindings.reduce.iter().map(|group| (&self.reduce, group)));
        for ((pipeline, group), extent) in levels.zip(&pyramid.extents) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(
                extent[0].div_ceil(PYRAMID_WORKGROUP),
                extent[1].div_ceil(PYRAMID_WORKGROUP),
                1,
            );
        }
    }
}
