//! Cull and depth-pyramid compute kernels on raw wgpu, so fixture tests drive the production code.

use std::{borrow::Cow, num::NonZeroU64};

use super::model::{
    CULL_WORKGROUP, CullPhase, CullViewUniform, FRUSTUM_ABSOLUTE_SLACK, FRUSTUM_RELATIVE_SLACK,
    HIZ_PADDING, PHASE_COUNT, STREAM_COUNT, args_words, group_count,
};

const RECORD_BYTES: u64 = std::mem::size_of::<super::model::CullRecord>() as u64;
const PYRAMID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
const PYRAMID_WORKGROUP: u32 = 8;

/// The cull kernel source with its Rust-owned constants substituted.
pub fn cull_shader_source() -> String {
    let float = |value: f32| format!("{value:?}f");
    include_str!("cull.wgsl")
        .replace("CULL_WORKGROUP", &format!("{CULL_WORKGROUP}u"))
        .replace("CULL_SIDE", &format!("{}i", world::SUB_CHUNK_SIDE))
        .replace("CULL_HIZ_PADDING", &float(HIZ_PADDING))
        .replace("CULL_RELATIVE_SLACK", &float(FRUSTUM_RELATIVE_SLACK))
        .replace("CULL_ABSOLUTE_SLACK", &float(FRUSTUM_ABSOLUTE_SLACK))
}

pub fn pyramid_shader_source() -> &'static str {
    include_str!("hiz.wgsl")
}

/// GPU buffers for `capacity` record slots; `readback` adds copy-out for tests.
pub struct CullStorage {
    pub capacity: u32,
    pub records: wgpu::Buffer,
    pub enabled: wgpu::Buffer,
    pub history: wgpu::Buffer,
    pub record_draws: wgpu::Buffer,
    pub group_sums: wgpu::Buffer,
    pub args: wgpu::Buffer,
    pub draw_counts: wgpu::Buffer,
    pub uniforms: [wgpu::Buffer; PHASE_COUNT],
}

impl CullStorage {
    pub fn new(device: &wgpu::Device, capacity: u32, readback: bool) -> Self {
        use wgpu::BufferUsages as U;
        let copy_out = if readback { U::COPY_SRC } else { U::empty() };
        let buffer = |label: &str, size: u64, usage: U| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        let slots = u64::from(capacity);
        let uniform = |label| {
            buffer(
                label,
                std::mem::size_of::<CullViewUniform>() as u64,
                U::UNIFORM | U::COPY_DST,
            )
        };
        Self {
            capacity,
            records: buffer(
                "terrain cull records",
                slots * RECORD_BYTES,
                U::STORAGE | U::COPY_DST,
            ),
            enabled: buffer(
                "terrain cull enabled bits",
                slots.div_ceil(32) * 4,
                U::STORAGE | U::COPY_DST,
            ),
            history: buffer(
                "terrain cull visibility history",
                slots * 4,
                U::STORAGE | U::COPY_DST | copy_out,
            ),
            record_draws: buffer("terrain cull record draws", slots * 4, U::STORAGE),
            group_sums: buffer(
                "terrain cull group sums",
                u64::from(group_count(capacity).max(1)) * 16,
                U::STORAGE,
            ),
            args: buffer(
                "terrain cull indirect args",
                args_words(capacity) * 4,
                U::STORAGE | U::INDIRECT | copy_out,
            ),
            draw_counts: buffer(
                "terrain cull draw counts",
                (PHASE_COUNT * STREAM_COUNT * 4) as u64,
                U::STORAGE | U::INDIRECT | copy_out,
            ),
            uniforms: [
                uniform("terrain cull early view"),
                uniform("terrain cull late view"),
            ],
        }
    }

    pub fn write_uniform(&self, queue: &wgpu::Queue, phase: CullPhase, uniform: &CullViewUniform) {
        queue.write_buffer(
            &self.uniforms[phase as usize],
            0,
            bytemuck::bytes_of(uniform),
        );
    }
}

/// Buffers for the occluded bit of `capacity` record slots.
pub struct OcclusionStorage {
    pub capacity: u32,
    pub records: wgpu::Buffer,
    /// One occluded bit per slot.
    pub occluded: wgpu::Buffer,
    pub uniform: wgpu::Buffer,
}

impl OcclusionStorage {
    pub fn new(device: &wgpu::Device, capacity: u32) -> Self {
        use wgpu::BufferUsages as U;
        let buffer = |label: &str, size: u64, usage: U| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16),
                usage,
                mapped_at_creation: false,
            })
        };
        Self {
            capacity,
            records: buffer(
                "terrain occlusion records",
                u64::from(capacity) * RECORD_BYTES,
                U::STORAGE | U::COPY_DST,
            ),
            occluded: buffer(
                "terrain occlusion bits",
                occlusion_bytes(capacity),
                U::STORAGE | U::COPY_SRC,
            ),
            uniform: buffer(
                "terrain occlusion view",
                std::mem::size_of::<CullViewUniform>() as u64,
                U::UNIFORM | U::COPY_DST,
            ),
        }
    }
}

/// Bytes of the occluded bitset for `capacity` slots.
pub fn occlusion_bytes(capacity: u32) -> u64 {
    u64::from(capacity.div_ceil(32)) * 4
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

/// Reverse-Z farthest-depth pyramid over a depth target of `depth_size` pixels.
pub struct HizPyramid {
    pub depth_size: [u32; 2],
    #[allow(dead_code, reason = "fixture tests read the pyramid back")]
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    mips: Vec<wgpu::TextureView>,
    sizes: Vec<[u32; 2]>,
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
        Self {
            depth_size,
            view: texture.create_view(&Default::default()),
            texture,
            mips,
            sizes,
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
    pub fn new(device: &wgpu::Device) -> Self {
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
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain cull layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: NonZeroU64::new(
                            std::mem::size_of::<CullViewUniform>() as u64
                        ),
                    },
                    count: None,
                },
                storage(1, true),
                storage(2, true),
                storage(3, false),
                storage(4, false),
                storage(5, false),
                storage(6, false),
                storage(7, false),
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
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("terrain cull pipeline layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let module = |label, source: String| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
            })
        };
        let cull = module("terrain cull", cull_shader_source());
        let pyramid = module("terrain hi-z", pyramid_shader_source().to_owned());
        let pipeline = |module, layout: Option<&wgpu::PipelineLayout>, entry: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout,
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
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
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("terrain cull"),
            timestamp_writes: None,
        });
        pass.set_bind_group(0, bind_group, &[]);
        let groups = group_count(slots);
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
        let groups = group_count(slots);
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
            ],
        );
        let reduce = pyramid
            .mips
            .windows(2)
            .map(|pair| {
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

    pub fn encode_pyramid(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pyramid: &HizPyramid,
        bindings: &PyramidBindings,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("terrain hi-z"),
            timestamp_writes: None,
        });
        let seed = if bindings.multisampled {
            &self.seed_multisampled
        } else {
            &self.seed
        };
        let levels = std::iter::once((seed, &bindings.seed))
            .chain(bindings.reduce.iter().map(|group| (&self.reduce, group)));
        for ((pipeline, group), size) in levels.zip(&pyramid.sizes) {
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(
                size[0].div_ceil(PYRAMID_WORKGROUP),
                size[1].div_ceil(PYRAMID_WORKGROUP),
                1,
            );
        }
    }
}
