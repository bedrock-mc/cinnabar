//! Cull and depth-pyramid compute kernels on raw wgpu, so fixture tests drive the production code.

use std::{borrow::Cow, num::NonZeroU64};

use wgpu::util::DeviceExt;

use super::model::{
    CULL_WORKGROUP, CullPhase, CullViewUniform, FRUSTUM_ABSOLUTE_SLACK, FRUSTUM_RELATIVE_SLACK,
    HIZ_PADDING, PHASE_COUNT, STREAM_COUNT, args_words, draw_offset_bytes, group_count,
};

const RECORD_BYTES: u64 = std::mem::size_of::<super::model::CullRecord>() as u64;
const PYRAMID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
/// Threads per side of a pyramid workgroup; each reduces a tile of twice as many texels per
/// side of its dispatch's first level.
const PYRAMID_SIDE: u32 = 16;
/// Levels one pyramid dispatch builds, one storage texture each, within the four storage
/// textures per stage every compute-capable device offers.
const PYRAMID_DISPATCH_LEVELS: usize = 4;

// `hiz.wgsl` binds four levels and keeps the tile's later ones, down to a quarter of the
// workgroup side, in workgroup memory.
const _: () =
    assert!(PYRAMID_DISPATCH_LEVELS == 4 && PYRAMID_SIDE.is_power_of_two() && PYRAMID_SIDE >= 4);

/// The shared Hi-Z occlusion test; see `hiz_test.wgsl` for what the including shader declares.
pub fn hiz_test_source() -> &'static str {
    include_str!("hiz_test.wgsl")
}

/// The cull kernel source with its Rust-owned constants substituted.
pub fn cull_shader_source() -> String {
    let float = |value: f32| format!("{value:?}f");
    [include_str!("cull.wgsl"), hiz_test_source()]
        .concat()
        .replace("CULL_WORKGROUP", &format!("{CULL_WORKGROUP}u"))
        .replace("CULL_SIDE", &format!("{}i", world::SUB_CHUNK_SIDE))
        .replace("CULL_HIZ_PADDING", &float(HIZ_PADDING))
        .replace("CULL_RELATIVE_SLACK", &float(FRUSTUM_RELATIVE_SLACK))
        .replace("CULL_ABSOLUTE_SLACK", &float(FRUSTUM_ABSOLUTE_SLACK))
}

/// The pyramid kernels' source with their Rust-owned constants substituted.
pub fn pyramid_shader_source() -> String {
    include_str!("hiz.wgsl")
        .replace("HIZ_SIDE", &format!("{PYRAMID_SIDE}u"))
        .replace("HIZ_LEVELS", &format!("{PYRAMID_DISPATCH_LEVELS}u"))
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
    /// Per-command vertex entries; see [`super::model::resolve_culled_args`].
    pub draw_offsets: wgpu::Buffer,
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
                U::STORAGE | U::INDIRECT | U::COPY_DST | copy_out,
            ),
            draw_counts: buffer(
                "terrain cull draw counts",
                (PHASE_COUNT * STREAM_COUNT * 4) as u64,
                U::STORAGE | U::INDIRECT | copy_out,
            ),
            draw_offsets: buffer(
                "terrain cull draw offsets",
                draw_offset_bytes(capacity),
                U::STORAGE | U::VERTEX | copy_out,
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
    /// Per dispatch, the `HizBounds` uniform of `hiz.wgsl`: the source's covered extent and
    /// that of each level it builds.
    bounds: Vec<wgpu::Buffer>,
    /// Bound in place of the levels the last dispatch does not build, which it never writes.
    spare: Vec<wgpu::TextureView>,
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
        let bounds = (0..extents.len())
            .step_by(PYRAMID_DISPATCH_LEVELS)
            .map(|base| {
                let source = if base == 0 {
                    depth_size.map(|side| side.max(1))
                } else {
                    extents[base - 1]
                };
                let mut words = vec![source[0], source[1], 0, 0];
                for level in base..base + PYRAMID_DISPATCH_LEVELS {
                    let extent = extents.get(level).copied().unwrap_or([0, 0]);
                    words.extend([extent[0], extent[1], 0, 0]);
                }
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("terrain hi-z dispatch bounds"),
                    contents: bytemuck::cast_slice(&words),
                    usage: wgpu::BufferUsages::UNIFORM,
                })
            })
            .collect();
        let unbuilt = extents.len().next_multiple_of(PYRAMID_DISPATCH_LEVELS) - extents.len();
        // Separate textures, so no dispatch binds one subresource twice.
        let spare = (0..unbuilt)
            .map(|_| {
                device
                    .create_texture(&wgpu::TextureDescriptor {
                        label: Some("terrain hi-z spare level"),
                        size: wgpu::Extent3d {
                            width: 1,
                            height: 1,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: PYRAMID_FORMAT,
                        usage: wgpu::TextureUsages::STORAGE_BINDING,
                        view_formats: &[],
                    })
                    .create_view(&Default::default())
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
            spare,
        }
    }

    /// The views a dispatch building levels from `base` writes, spares past the last level.
    fn dispatch_levels(&self, base: usize) -> impl Iterator<Item = &wgpu::TextureView> {
        self.mips[base..]
            .iter()
            .chain(&self.spare)
            .take(PYRAMID_DISPATCH_LEVELS)
    }

    pub fn mip_count(&self) -> u32 {
        self.sizes.len() as u32
    }

    #[allow(dead_code, reason = "fixture tests read the pyramid back")]
    pub fn size(&self, level: u32) -> [u32; 2] {
        self.sizes[level as usize]
    }
}

/// Bind groups that seed a pyramid from one depth view and reduce its mips, one per dispatch.
pub struct PyramidBindings {
    multisampled: bool,
    dispatches: Vec<wgpu::BindGroup>,
}

/// The kernels that build a [`HizPyramid`] from a depth view, [`PYRAMID_DISPATCH_LEVELS`]
/// levels per dispatch.
pub struct PyramidKernels {
    seed: wgpu::ComputePipeline,
    seed_multisampled: wgpu::ComputePipeline,
    reduce: wgpu::ComputePipeline,
}

/// Compiles `module`'s `entry` without a fixed layout and without zero-filling workgroup
/// memory: every kernel writes its own workgroup slots before any barrier reads them, and
/// zero-filling the scratch anyway kept FXC optimising for seconds per kernel at every DX12
/// launch.
fn kernel(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: Option<&wgpu::PipelineLayout>,
    entry: &str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout,
        module,
        entry_point: Some(entry),
        compilation_options: wgpu::PipelineCompilationOptions {
            zero_initialize_workgroup_memory: false,
            ..Default::default()
        },
        cache: None,
    })
}

impl PyramidKernels {
    /// Compiles the seed and reduce kernels on `device`; they bind each pyramid through
    /// [`Self::bindings`].
    pub fn new(device: &wgpu::Device) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("terrain hi-z"),
            source: wgpu::ShaderSource::Wgsl(Cow::Owned(pyramid_shader_source())),
        });
        Self {
            seed: kernel(device, &module, None, "hiz_seed"),
            seed_multisampled: kernel(device, &module, None, "hiz_seed_multisampled"),
            reduce: kernel(device, &module, None, "hiz_reduce"),
        }
    }

    /// The pipeline of the dispatch building levels from `base`.
    fn pipeline(&self, base: usize, multisampled: bool) -> &wgpu::ComputePipeline {
        match (base, multisampled) {
            (0, false) => &self.seed,
            (0, true) => &self.seed_multisampled,
            _ => &self.reduce,
        }
    }

    /// Bind groups that seed `pyramid` from `depth` and reduce its mips.
    pub fn bindings(
        &self,
        device: &wgpu::Device,
        depth: &wgpu::TextureView,
        multisampled: bool,
        pyramid: &HizPyramid,
    ) -> PyramidBindings {
        let dispatches =
            pyramid
                .bounds
                .iter()
                .enumerate()
                .map(|(dispatch, bounds)| {
                    let base = dispatch * PYRAMID_DISPATCH_LEVELS;
                    let source = match (base, multisampled) {
                        (0, false) => 0,
                        (0, true) => 1,
                        _ => 2,
                    };
                    let source_view = if base == 0 {
                        depth
                    } else {
                        &pyramid.mips[base - 1]
                    };
                    let mut entries = vec![
                        wgpu::BindGroupEntry {
                            binding: source,
                            resource: wgpu::BindingResource::TextureView(source_view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3 + PYRAMID_DISPATCH_LEVELS as u32,
                            resource: bounds.as_entire_binding(),
                        },
                    ];
                    entries.extend(pyramid.dispatch_levels(base).zip(3..).map(
                        |(view, binding)| wgpu::BindGroupEntry {
                            binding,
                            resource: wgpu::BindingResource::TextureView(view),
                        },
                    ));
                    device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("terrain hi-z bindings"),
                        layout: &self.pipeline(base, multisampled).get_bind_group_layout(0),
                        entries: &entries,
                    })
                })
                .collect();
        PyramidBindings {
            multisampled,
            dispatches,
        }
    }

    /// Seeds and reduces every level of `pyramid` in one compute pass.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pyramid: &HizPyramid,
        bindings: &PyramidBindings,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("terrain hi-z"),
            timestamp_writes: None,
        });
        for (dispatch, group) in bindings.dispatches.iter().enumerate() {
            let base = dispatch * PYRAMID_DISPATCH_LEVELS;
            let extent = pyramid.extents[base];
            pass.set_pipeline(self.pipeline(base, bindings.multisampled));
            pass.set_bind_group(0, group, &[]);
            pass.dispatch_workgroups(
                extent[0].div_ceil(PYRAMID_SIDE * 2),
                extent[1].div_ceil(PYRAMID_SIDE * 2),
                1,
            );
        }
    }
}

pub struct CullKernels {
    layout: wgpu::BindGroupLayout,
    count: wgpu::ComputePipeline,
    scan: wgpu::ComputePipeline,
    emit: wgpu::ComputePipeline,
    pyramid: PyramidKernels,
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
                storage(10, false),
            ],
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
        let cull = module("terrain cull", cull_shader_source());
        // The occlusion bits are cleared explicitly, so no kernel needs zero-filled scratch.
        let pipeline = |module, layout: Option<&wgpu::PipelineLayout>, entry: &str| {
            kernel(device, module, layout, entry)
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
            pyramid: PyramidKernels::new(device),
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
        self.pyramid.bindings(device, depth, multisampled, pyramid)
    }

    pub fn encode_pyramid(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pyramid: &HizPyramid,
        bindings: &PyramidBindings,
    ) {
        self.pyramid.encode(encoder, pyramid, bindings);
    }
}
