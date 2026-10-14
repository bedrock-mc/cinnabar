//! The renderer's GPU objects: the fixed buffers sized once from a [`Capacity`], the compute
//! kernels and the two draw pipelines. [`LodGpu::new`] builds them all and may run on any
//! thread, so enabling distant terrain compiles and allocates off the render thread.

use std::{borrow::Cow, num::NonZeroU64};

use wgpu::util::DeviceExt;

use super::layout::{
    ARGS_WORDS, Capacity, DRAW_PASS_STRIDE, DRAW_PASSES, DrawPass, INDEX_COUNT, LodView, prelude,
};
use crate::chunk::{PyramidKernels, hiz_test_source};

/// Encoder-level timestamps one frame writes, one between each pair of passes.
pub(crate) const TIMESTAMP_MARKS: u32 =
    render_scene::distant_terrain::DistantTerrainPass::COUNT as u32 + 1;
/// Frames whose counters or timestamps may await readback at once.
pub(crate) const READBACK_SLOTS: usize = crate::gpu_timing::readback::SLOTS;

/// The colour and depth targets the draw pipelines render into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct TargetKey {
    pub(crate) colour: wgpu::TextureFormat,
    pub(crate) depth: wgpu::TextureFormat,
    pub(crate) samples: u32,
}

/// The scatter shader source.
pub(crate) fn scatter_source() -> String {
    prelude() + include_str!("scatter.wgsl")
}

/// The culling shader source, with the shared Hi-Z test.
pub(crate) fn cull_source() -> String {
    prelude() + include_str!("cull.wgsl") + hiz_test_source()
}

/// The draw shader source.
pub(crate) fn draw_source() -> String {
    prelude() + include_str!("draw.wgsl")
}

/// The culling kernels in dispatch order.
pub(crate) struct CullKernels {
    pub(crate) sections: wgpu::ComputePipeline,
    pub(crate) count_opaque: wgpu::ComputePipeline,
    pub(crate) scan_opaque: wgpu::ComputePipeline,
    pub(crate) emit_opaque: wgpu::ComputePipeline,
    pub(crate) count_translucent: wgpu::ComputePipeline,
    pub(crate) scan_translucent: wgpu::ComputePipeline,
    pub(crate) emit_translucent: wgpu::ComputePipeline,
}

/// Readback buffers of one frame's counters and timestamps.
pub(crate) struct ReadbackSlot {
    pub(crate) stats: wgpu::Buffer,
    pub(crate) timestamps: Option<wgpu::Buffer>,
}

/// Every GPU object of one renderer; see the module documentation.
pub(crate) struct LodGpu {
    pub(crate) capacity: Capacity,
    pub(crate) pool: wgpu::Buffer,
    pub(crate) clusters: wgpu::Buffer,
    pub(crate) sections: wgpu::Buffer,
    pub(crate) flags: wgpu::Buffer,
    pub(crate) group_sums: wgpu::Buffer,
    pub(crate) lists: wgpu::Buffer,
    pub(crate) args: wgpu::Buffer,
    pub(crate) order: wgpu::Buffer,
    pub(crate) staging: wgpu::Buffer,
    pub(crate) view: wgpu::Buffer,
    pub(crate) passes: wgpu::Buffer,
    pub(crate) indices: wgpu::Buffer,
    pub(crate) scatter: wgpu::ComputePipeline,
    pub(crate) scatter_group: wgpu::BindGroup,
    pub(crate) cull_layout: wgpu::BindGroupLayout,
    pub(crate) cull: CullKernels,
    pub(crate) pyramid: PyramidKernels,
    /// Bound where no pyramid exists; Hi-Z is then off.
    pub(crate) blank_hiz: wgpu::TextureView,
    pub(crate) draw_layout: wgpu::BindGroupLayout,
    pub(crate) opaque: wgpu::RenderPipeline,
    pub(crate) translucent: wgpu::RenderPipeline,
    pub(crate) readbacks: Vec<ReadbackSlot>,
    pub(crate) timestamps: Option<(wgpu::QuerySet, wgpu::Buffer)>,
}

/// A storage buffer binding visible to `visibility`, read-only when `read_only`.
fn storage_entry(
    binding: u32,
    read_only: bool,
    visibility: wgpu::ShaderStages,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}

/// A uniform binding of at least `size` bytes, with a dynamic offset when `dynamic`.
fn uniform_entry(
    binding: u32,
    visibility: wgpu::ShaderStages,
    size: u64,
    dynamic: bool,
) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: dynamic,
            min_binding_size: NonZeroU64::new(size),
        },
        count: None,
    }
}

/// The scatter kernel's bindings: staging, then every [`super::layout::Target`] in order.
fn scatter_entries() -> [wgpu::BindGroupLayoutEntry; 6] {
    use wgpu::ShaderStages as S;
    std::array::from_fn(|binding| storage_entry(binding as u32, binding == 0, S::COMPUTE))
}

/// The culling kernels' bindings: view, sections, clusters, flags, group sums, lists, args,
/// order and the Hi-Z pyramid.
fn cull_entries() -> [wgpu::BindGroupLayoutEntry; 9] {
    use wgpu::ShaderStages as S;
    [
        uniform_entry(0, S::COMPUTE, size_of::<LodView>() as u64, false),
        storage_entry(1, true, S::COMPUTE),
        storage_entry(2, true, S::COMPUTE),
        storage_entry(3, false, S::COMPUTE),
        storage_entry(4, false, S::COMPUTE),
        storage_entry(5, false, S::COMPUTE),
        storage_entry(6, false, S::COMPUTE),
        storage_entry(7, true, S::COMPUTE),
        wgpu::BindGroupLayoutEntry {
            binding: 8,
            visibility: S::COMPUTE,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        },
    ]
}

/// The draw pipelines' bindings: view, sections, clusters, pool, lists, colour tables, the
/// world lightmap and the per-pass uniform.
fn draw_entries() -> [wgpu::BindGroupLayoutEntry; 8] {
    use wgpu::ShaderStages as S;
    [
        uniform_entry(0, S::VERTEX_FRAGMENT, size_of::<LodView>() as u64, false),
        storage_entry(1, true, S::VERTEX),
        storage_entry(2, true, S::VERTEX),
        storage_entry(3, true, S::VERTEX_FRAGMENT),
        storage_entry(4, true, S::VERTEX),
        storage_entry(5, true, S::FRAGMENT),
        uniform_entry(
            6,
            S::FRAGMENT,
            size_of::<render_model::LightmapTable>() as u64,
            false,
        ),
        uniform_entry(7, S::VERTEX_FRAGMENT, size_of::<DrawPass>() as u64, true),
    ]
}

/// The most storage buffers any one shader stage binds across the renderer's layouts, which
/// a device must support per stage.
pub(crate) fn storage_buffers_per_stage() -> u32 {
    use wgpu::ShaderStages as S;
    let layouts = [
        scatter_entries().to_vec(),
        cull_entries().to_vec(),
        draw_entries().to_vec(),
    ];
    layouts
        .iter()
        .flat_map(|entries| {
            [S::VERTEX, S::FRAGMENT, S::COMPUTE].map(|stage| {
                entries
                    .iter()
                    .filter(|entry| {
                        entry.visibility.contains(stage)
                            && matches!(
                                entry.ty,
                                wgpu::BindingType::Buffer {
                                    ty: wgpu::BufferBindingType::Storage { .. },
                                    ..
                                }
                            )
                    })
                    .count() as u32
            })
        })
        .max()
        .unwrap_or(0)
}

/// The compute pipeline of `module`'s `entry` with `layout`.
fn kernel(
    device: &wgpu::Device,
    module: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    entry: &str,
) -> wgpu::ComputePipeline {
    device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(entry),
        layout: Some(layout),
        module,
        entry_point: Some(entry),
        // Every kernel writes its workgroup scratch before a barrier reads it.
        compilation_options: wgpu::PipelineCompilationOptions {
            zero_initialize_workgroup_memory: false,
            ..Default::default()
        },
        cache: None,
    })
}

/// A WGSL shader module of `source`.
fn module(device: &wgpu::Device, label: &str, source: String) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(Cow::Owned(source)),
    })
}

/// The static index buffer: two triangles per quad slot, every cluster's slots alike.
pub(crate) fn cluster_indices() -> Vec<u16> {
    (0..INDEX_COUNT / 6)
        .flat_map(|quad| {
            let base = quad as u16 * 4;
            [base, base + 1, base + 2, base, base + 2, base + 3]
        })
        .collect()
}

impl LodGpu {
    /// Builds every buffer, kernel and pipeline of a renderer of `capacity` drawing into
    /// `key`'s targets; `timestamps` adds the timing queries.
    pub(crate) fn new(
        device: &wgpu::Device,
        capacity: Capacity,
        key: TargetKey,
        timestamps: bool,
    ) -> Self {
        use wgpu::BufferUsages as U;
        let buffer = |label: &str, size: u64, usage: U| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: size.max(16).next_multiple_of(4),
                usage,
                mapped_at_creation: false,
            })
        };
        let pool = buffer("distant terrain pool", capacity.pool_bytes(), U::STORAGE);
        let clusters = buffer(
            "distant terrain clusters",
            capacity.clusters_bytes(),
            U::STORAGE | U::COPY_DST,
        );
        let sections = buffer(
            "distant terrain sections",
            capacity.sections_bytes(),
            U::STORAGE | U::COPY_DST,
        );
        let flags = buffer(
            "distant terrain flags",
            capacity.flags_bytes(),
            U::STORAGE | U::COPY_DST,
        );
        let group_sums = buffer(
            "distant terrain group sums",
            capacity.group_sums_bytes(),
            U::STORAGE,
        );
        let lists = buffer("distant terrain lists", capacity.lists_bytes(), U::STORAGE);
        let args = buffer(
            "distant terrain args",
            u64::from(ARGS_WORDS) * 4,
            U::STORAGE | U::INDIRECT | U::COPY_DST | U::COPY_SRC,
        );
        let order = buffer(
            "distant terrain order",
            capacity.order_bytes(),
            U::STORAGE | U::COPY_DST,
        );
        let staging = buffer(
            "distant terrain staging",
            capacity.staging_bytes(),
            U::STORAGE | U::COPY_DST,
        );
        let view = buffer(
            "distant terrain view",
            size_of::<LodView>() as u64,
            U::UNIFORM | U::COPY_DST,
        );
        let mut pass_bytes = vec![0_u8; DRAW_PASS_STRIDE as usize * DRAW_PASSES];
        let cap = capacity.clusters;
        for (index, pass) in [
            DrawPass::default(),
            DrawPass {
                base: cap - 1,
                backward: 1,
                ..DrawPass::default()
            },
            DrawPass {
                base: cap,
                translucent: 1,
                ..DrawPass::default()
            },
        ]
        .iter()
        .enumerate()
        {
            let at = index * DRAW_PASS_STRIDE as usize;
            pass_bytes[at..at + size_of::<DrawPass>()].copy_from_slice(bytemuck::bytes_of(pass));
        }
        let passes = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("distant terrain draw passes"),
            contents: &pass_bytes,
            usage: U::UNIFORM,
        });
        let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("distant terrain cluster indices"),
            contents: bytemuck::cast_slice(&cluster_indices()),
            usage: U::INDEX,
        });

        let scatter_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("distant terrain scatter layout"),
            entries: &scatter_entries(),
        });
        let scatter_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("distant terrain scatter pipeline layout"),
                bind_group_layouts: &[Some(&scatter_layout)],
                immediate_size: 0,
            });
        let scatter_module = module(device, "distant terrain scatter", scatter_source());
        let scatter = kernel(
            device,
            &scatter_module,
            &scatter_pipeline_layout,
            "lod_scatter",
        );
        let scatter_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("distant terrain scatter bindings"),
            layout: &scatter_layout,
            entries: &[&staging, &pool, &clusters, &sections, &flags, &order]
                .iter()
                .enumerate()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding: binding as u32,
                    resource: buffer.as_entire_binding(),
                })
                .collect::<Vec<_>>(),
        });

        let cull_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("distant terrain cull layout"),
            entries: &cull_entries(),
        });
        let cull_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("distant terrain cull pipeline layout"),
            bind_group_layouts: &[Some(&cull_layout)],
            immediate_size: 0,
        });
        let cull_module = module(device, "distant terrain cull", cull_source());
        let cull_kernel = |entry| kernel(device, &cull_module, &cull_pipeline_layout, entry);
        let cull = CullKernels {
            sections: cull_kernel("lod_sections"),
            count_opaque: cull_kernel("lod_count_opaque"),
            scan_opaque: cull_kernel("lod_scan_opaque"),
            emit_opaque: cull_kernel("lod_emit_opaque"),
            count_translucent: cull_kernel("lod_count_translucent"),
            scan_translucent: cull_kernel("lod_scan_translucent"),
            emit_translucent: cull_kernel("lod_emit_translucent"),
        };
        let blank = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("distant terrain hi-z placeholder"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let draw_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("distant terrain draw layout"),
            entries: &draw_entries(),
        });
        let draw_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("distant terrain draw pipeline layout"),
            bind_group_layouts: &[Some(&draw_layout)],
            immediate_size: 0,
        });
        let draw_module = module(device, "distant terrain draw", draw_source());
        let pipeline = |translucent: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if translucent {
                    "distant terrain translucent"
                } else {
                    "distant terrain opaque"
                }),
                layout: Some(&draw_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &draw_module,
                    entry_point: Some("lod_vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &draw_module,
                    entry_point: Some("lod_fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: key.colour,
                        blend: translucent.then_some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                // Clusters face one way and are culled by direction, so triangles need not be.
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: key.depth,
                    depth_write_enabled: Some(!translucent),
                    // Reverse-Z: strictly nearer, so coplanar resident terrain wins ties.
                    depth_compare: Some(wgpu::CompareFunction::Greater),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: wgpu::MultisampleState {
                    count: key.samples,
                    ..Default::default()
                },
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = pipeline(false);
        let translucent = pipeline(true);

        let readback = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: u64::from(ARGS_WORDS) * 4,
                usage: U::MAP_READ | U::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let timestamp_bytes = u64::from(TIMESTAMP_MARKS) * 8;
        let readbacks = (0..READBACK_SLOTS)
            .map(|_| ReadbackSlot {
                stats: readback("distant terrain counters readback"),
                timestamps: timestamps.then(|| {
                    device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("distant terrain timestamps readback"),
                        size: timestamp_bytes,
                        usage: U::MAP_READ | U::COPY_DST,
                        mapped_at_creation: false,
                    })
                }),
            })
            .collect();
        let timestamps = timestamps.then(|| {
            (
                device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("distant terrain timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: TIMESTAMP_MARKS,
                }),
                buffer(
                    "distant terrain timestamp resolve",
                    timestamp_bytes,
                    U::QUERY_RESOLVE | U::COPY_SRC,
                ),
            )
        });
        Self {
            capacity,
            pool,
            clusters,
            sections,
            flags,
            group_sums,
            lists,
            args,
            order,
            staging,
            view,
            passes,
            indices,
            scatter,
            scatter_group,
            cull_layout,
            cull,
            pyramid: PyramidKernels::new(device),
            blank_hiz: blank.create_view(&Default::default()),
            draw_layout,
            opaque,
            translucent,
            readbacks,
            timestamps,
        }
    }

    /// The culling bindings, testing `hiz` when given.
    pub(crate) fn cull_group(
        &self,
        device: &wgpu::Device,
        hiz: Option<&wgpu::TextureView>,
    ) -> wgpu::BindGroup {
        let buffers = [
            &self.view,
            &self.sections,
            &self.clusters,
            &self.flags,
            &self.group_sums,
            &self.lists,
            &self.args,
            &self.order,
        ];
        let mut entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 8,
            resource: wgpu::BindingResource::TextureView(hiz.unwrap_or(&self.blank_hiz)),
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("distant terrain cull bindings"),
            layout: &self.cull_layout,
            entries: &entries,
        })
    }

    /// The draw bindings with the colour `tables` and the world `lightmap`.
    pub(crate) fn draw_group(
        &self,
        device: &wgpu::Device,
        tables: &wgpu::Buffer,
        lightmap: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        let mut entries: Vec<_> = [
            &self.view,
            &self.sections,
            &self.clusters,
            &self.pool,
            &self.lists,
            tables,
            lightmap,
        ]
        .iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: buffer.as_entire_binding(),
        })
        .collect();
        entries.push(wgpu::BindGroupEntry {
            binding: 7,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &self.passes,
                offset: 0,
                size: NonZeroU64::new(size_of::<DrawPass>() as u64),
            }),
        });
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("distant terrain draw bindings"),
            layout: &self.draw_layout,
            entries: &entries,
        })
    }

    /// Clears every slot, record and list, so nothing is drawn until new updates commit.
    pub(crate) fn clear(&self, encoder: &mut wgpu::CommandEncoder) {
        for buffer in [
            &self.sections,
            &self.clusters,
            &self.flags,
            &self.args,
            &self.order,
        ] {
            encoder.clear_buffer(buffer, 0, None);
        }
    }
}
