#![allow(
    dead_code,
    reason = "shared GPU helpers serve different shader test targets"
)]

use std::{
    borrow::Cow,
    future::Future,
    ops::Range,
    task::{Context, Poll, Waker},
};
use wgpu::{naga, util::DeviceExt};

pub const SNAPSHOT_SIDE: u32 = 256;
/// Color retained by pixels where a fixture fragment is discarded.
pub const CLEAR_COLOR: wgpu::Color = wgpu::Color {
    r: 0.12,
    g: 0.18,
    b: 0.25,
    a: 1.0,
};

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub backend: wgpu::Backend,
}

/// Starts an isolated shader probe with every production atmosphere field zeroed.
pub fn atmosphere() -> Vec<f32> {
    atmosphere_words(&bytemuck::Zeroable::zeroed())
}

/// Copies a typed production frame without repeating its size or field count.
pub fn atmosphere_words(frame: &render_scene::AtmosphereFrame) -> Vec<f32> {
    bytemuck::cast_slice(std::slice::from_ref(frame)).to_vec()
}

/// Supplies the ordinary cloud colour and geometry from their owning constants.
pub fn cloud_uniform() -> [f32; 8] {
    [
        1.0,
        1.0,
        1.0,
        render_scene::CLOUD_ALPHA,
        meshing::CLOUD_CELL_BLOCKS,
        meshing::CLOUD_UNDERSIDE_Y,
        meshing::CLOUD_TOP_Y,
        meshing::CLOUD_WORLD_PERIOD,
    ]
}

/// Lists cloud resources once so every cloud raster fixture uses the same layout.
pub fn cloud_bindings<'a>(
    view: &'a wgpu::Buffer,
    atmosphere: &'a wgpu::Buffer,
    records: &'a wgpu::Buffer,
    cloud: &'a wgpu::Buffer,
) -> [wgpu::BindGroupEntry<'a>; 4] {
    let buffers = [view, atmosphere, records, cloud];
    std::array::from_fn(|binding| wgpu::BindGroupEntry {
        binding: binding as u32,
        resource: buffers[binding].as_entire_binding(),
    })
}

/// Reserves one range for all flattened production lighting resources.
pub const FIXTURE_LIGHTING_BASE: u32 = 64;

/// Owns shared fixture lighting so additional shader resources have one constructor.
pub struct WorldLighting {
    lightmap: wgpu::Buffer,
    atmosphere: wgpu::Buffer,
    water: wgpu::TextureView,
    sampler: wgpu::Sampler,
    water_info: wgpu::Buffer,
}

impl WorldLighting {
    /// Uses a probe's lightmap and atmosphere, supplying neutral optional resources.
    pub fn new(gpu: &Gpu, lightmap: &wgpu::Buffer, frame: Option<&wgpu::Buffer>) -> Self {
        Self {
            lightmap: lightmap.clone(),
            atmosphere: frame
                .cloned()
                .unwrap_or_else(|| gpu.buffer(&atmosphere(), wgpu::BufferUsages::UNIFORM)),
            water: gpu.colour_texture_array(&[[255; 4]; 2]),
            sampler: gpu.device.create_sampler(&Default::default()),
            water_info: gpu.water_info(),
        }
    }

    /// Selects authored water resources for probes that exercise texture selection.
    pub fn with_water(
        mut self,
        water: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        info: &wgpu::Buffer,
    ) -> Self {
        self.water = water.clone();
        self.sampler = sampler.clone();
        self.water_info = info.clone();
        self
    }

    /// Lists the complete shared lighting group in production binding order.
    pub fn bindings(&self) -> impl Iterator<Item = wgpu::BindGroupEntry<'_>> {
        [
            (0, self.lightmap.as_entire_binding()),
            (1, self.atmosphere.as_entire_binding()),
            (2, wgpu::BindingResource::TextureView(&self.water)),
            (3, wgpu::BindingResource::Sampler(&self.sampler)),
            (4, self.water_info.as_entire_binding()),
        ]
        .into_iter()
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource })
    }

    /// Creates the complete lighting layout for fixtures that combine several pipelines.
    pub fn layout(&self, gpu: &Gpu) -> wgpu::BindGroupLayout {
        let entries = self
            .bindings()
            .map(|entry| wgpu::BindGroupLayoutEntry {
                binding: entry.binding,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: match entry.resource {
                    wgpu::BindingResource::Buffer(_) => wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    wgpu::BindingResource::TextureView(_) => wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    wgpu::BindingResource::Sampler(_) => {
                        wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering)
                    }
                    _ => panic!("unsupported world fixture resource"),
                },
                count: None,
            })
            .collect::<Vec<_>>();
        gpu.device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("world fixture lighting"),
                entries: &entries,
            })
    }

    /// Relocates the same resource group for single-group raster probes.
    pub fn flat_bindings(&self) -> impl Iterator<Item = wgpu::BindGroupEntry<'_>> {
        self.bindings().map(|mut entry| {
            entry.binding += FIXTURE_LIGHTING_BASE;
            entry
        })
    }
}

/// Selects only resources actually read by the requested shader stages.
pub fn bindings_for<'a>(
    source: &str,
    stages: &[&str],
    group: u32,
    entries: &[wgpu::BindGroupEntry<'a>],
) -> Vec<wgpu::BindGroupEntry<'a>> {
    let module = naga::front::wgsl::parse_str(source).expect("fixture shader parses");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("fixture shader validates");
    let stages = stages
        .iter()
        .map(|name| {
            let index = module
                .entry_points
                .iter()
                .position(|entry| entry.name == *name)
                .expect("fixture entry point exists");
            info.get_entry_point(index)
        })
        .collect::<Vec<_>>();
    let used = module
        .global_variables
        .iter()
        .filter_map(|(handle, global)| {
            global
                .binding
                .as_ref()
                .filter(|binding| {
                    binding.group == group && stages.iter().any(|stage| !stage[handle].is_empty())
                })
                .map(|binding| binding.binding)
        })
        .collect::<Vec<_>>();
    for binding in &used {
        assert!(
            entries.iter().any(|entry| entry.binding == *binding),
            "missing fixture resource at group {group}, binding {binding}"
        );
    }
    entries
        .iter()
        .filter(|entry| used.contains(&entry.binding))
        .cloned()
        .collect()
}

pub struct Draw<'a> {
    pub fragment: &'a str,
    pub vertices: Range<u32>,
    pub bindings: &'a [wgpu::BindGroupEntry<'a>],
    pub blend: Option<wgpu::BlendState>,
    pub write_depth: bool,
}

pub struct RasterState {
    /// Initial background for mask and colour witnesses.
    pub clear_color: wgpu::Color,
    /// Optional production actor fragment selected by the actor raster fixture.
    pub actor_fragment: Option<&'static str>,
    /// Optional dissolve companion, rendered with its independent coverage parameter.
    pub actor_companion: Option<render_model::ActorMaterial>,
    /// Optional attachment retained for a later pass to sample or probe.
    pub depth_target: Option<wgpu::Texture>,
    pub layout: Option<wgpu::PipelineLayout>,
    pub primitive: wgpu::PrimitiveState,
    pub depth_compare: wgpu::CompareFunction,
    pub write_mask: wgpu::ColorWrites,
    pub multisample: wgpu::MultisampleState,
    /// Optional per-draw polygon offsets, applied after clipping and rasterization.
    pub depth_biases: Vec<wgpu::DepthBiasState>,
}

pub struct DrawPipeline<'a> {
    pub vertex: &'a str,
    pub topology: wgpu::PrimitiveTopology,
}

struct RasterConfiguration<'a> {
    state: RasterState,
    pipelines: &'a [DrawPipeline<'a>],
    /// Per-draw shader sources; empty when every draw shares the one source.
    sources: &'a [&'a str],
    /// Shared resources after each draw's group zero, in bind-group order.
    additional_bindings: &'a [&'a [wgpu::BindGroupEntry<'a>]],
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            clear_color: CLEAR_COLOR,
            actor_fragment: None,
            actor_companion: None,
            depth_target: None,
            layout: None,
            primitive: Default::default(),
            depth_compare: wgpu::CompareFunction::GreaterEqual,
            write_mask: wgpu::ColorWrites::ALL,
            multisample: Default::default(),
            depth_biases: Vec::new(),
        }
    }
}

/// Polls wgpu futures without an additional executor dependency.
fn finish<T>(future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        std::thread::yield_now();
    }
}

#[allow(
    clippy::print_stderr,
    reason = "names the missing GPU fixture when the test skips; test-exempt where render's unit tests include this file"
)]
fn fixture_adapter<T>(name: &str, result: Result<T, wgpu::RequestAdapterError>) -> Option<T> {
    match result {
        Ok(adapter) => Some(adapter),
        Err(error @ wgpu::RequestAdapterError::NotFound { .. }) => {
            eprintln!("skipping {name}: missing native GPU adapter fixture ({error})");
            None
        }
        Err(error) => panic!("{name}: GPU fixture adapter request failed: {error}"),
    }
}

impl Gpu {
    /// Binds one unanimated texel for each optional water kind in ordinary-path fixtures.
    pub fn water_info(&self) -> wgpu::Buffer {
        self.words(
            &[[0, 1, 1, 1]; assets::java_world::WATER_TEXTURE_PATHS.len()].concat(),
            wgpu::BufferUsages::UNIFORM,
        )
    }

    /// Uploads both the raw and byte-truncated tables using the production lightmap layout.
    pub fn lightmap(&self, values: &[f32]) -> wgpu::Buffer {
        let raw: &[[f32; 4]; 256] = bytemuck::cast_slice(values)
            .try_into()
            .expect("256 RGBA light entries");
        let table = render_model::LightmapUniform::new(*raw);
        self.buffer(
            bytemuck::cast_slice(bytemuck::bytes_of(&table)),
            wgpu::BufferUsages::UNIFORM,
        )
    }

    /// Creates a physical device for explicitly requested snapshot fixtures.
    pub fn new() -> Option<Self> {
        Self::for_fixture("native GPU snapshot")
    }

    /// Skips absent hardware while preserving adapter, device and rendering errors.
    pub fn for_fixture(name: &str) -> Option<Self> {
        Self::for_fixture_with(name, wgpu::Features::empty())
    }

    /// As [`Self::for_fixture`], enabling whichever of `features` the adapter offers.
    pub fn for_fixture_with(name: &str, features: wgpu::Features) -> Option<Self> {
        Self::for_fixture_with_limits(name, features, wgpu::Limits::default())
    }

    /// As [`Self::for_fixture_with`], with the production renderer's raised `limits`.
    #[allow(
        clippy::print_stderr,
        reason = "names the missing GPU fixture when the test skips; test-exempt where render's unit tests include this file"
    )]
    pub fn for_fixture_with_limits(
        name: &str,
        features: wgpu::Features,
        limits: wgpu::Limits,
    ) -> Option<Self> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
        let adapter = fixture_adapter(
            name,
            finish(instance.request_adapter(&wgpu::RequestAdapterOptions::default())),
        )?;
        if adapter.get_info().backend == wgpu::Backend::Noop {
            eprintln!("skipping {name}: missing native GPU adapter fixture (Noop adapter)");
            return None;
        }
        let descriptor = wgpu::DeviceDescriptor {
            required_features: adapter.features() & features,
            required_limits: limits,
            ..Default::default()
        };
        let (device, queue) = finish(adapter.request_device(&descriptor))
            .unwrap_or_else(|error| panic!("{name}: GPU fixture device creation failed: {error}"));
        let backend = adapter.get_info().backend;
        Some(Self {
            device,
            queue,
            backend,
        })
    }

    /// Uploads raw storage words, such as packed quads, for a fixture.
    pub fn words(&self, data: &[u32], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(data),
                usage,
            })
    }

    /// Uploads the packed production uniform or vertex words used by a fixture.
    pub fn buffer(&self, data: &[f32], usage: wgpu::BufferUsages) -> wgpu::Buffer {
        self.device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(data),
                usage,
            })
    }

    /// Uploads one solid RGBA texel per layer for distinct material witnesses.
    pub fn colour_texture_array(&self, colours: &[[u8; 4]]) -> wgpu::TextureView {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("fixture colour layers"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: colours.len() as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(colours),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            texture.size(),
        );
        texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
    }

    /// A zeroed 1x1 2D texture for bindings a draw never samples.
    pub fn blank_texture_view(&self) -> wgpu::TextureView {
        self.device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }

    /// Renders production entry points with reverse depth, then reads their actual pixels.
    pub fn render(&self, source: &str, vertex: &str, draws: &[Draw<'_>]) -> Vec<u8> {
        self.render_with_state(source, vertex, draws, RasterState::default())
    }

    /// Applies the requested raster state, including sample count and alpha-to-coverage.
    pub fn render_with_state(
        &self,
        source: &str,
        vertex: &str,
        draws: &[Draw<'_>],
        state: RasterState,
    ) -> Vec<u8> {
        self.render_with_bind_groups(source, vertex, draws, state, &[])
    }

    /// Binds shared lighting resources after each draw's own group-zero resources.
    pub fn render_with_bind_groups(
        &self,
        source: &str,
        vertex: &str,
        draws: &[Draw<'_>],
        state: RasterState,
        additional_bindings: &[&[wgpu::BindGroupEntry<'_>]],
    ) -> Vec<u8> {
        self.render_to_format(
            source,
            vertex,
            draws,
            wgpu::TextureFormat::Rgba8Unorm,
            RasterConfiguration {
                state,
                pipelines: &[],
                sources: &[],
                additional_bindings,
            },
        )
    }

    /// Renders mixed primitives through each draw's production vertex entry point.
    pub fn render_mixed(
        &self,
        source: &str,
        draws: &[Draw<'_>],
        pipelines: &[DrawPipeline<'_>],
    ) -> Vec<u8> {
        assert_eq!(draws.len(), pipelines.len());
        self.render_to_format(
            source,
            "",
            draws,
            wgpu::TextureFormat::Rgba8Unorm,
            RasterConfiguration {
                state: RasterState::default(),
                pipelines,
                sources: &[],
                additional_bindings: &[],
            },
        )
    }

    /// Renders each draw through its own shader source and vertex entry point
    /// into one shared colour and depth target, as separate world passes do.
    pub fn render_sources(
        &self,
        sources: &[&str],
        draws: &[Draw<'_>],
        pipelines: &[DrawPipeline<'_>],
        state: RasterState,
    ) -> Vec<u8> {
        assert_eq!(draws.len(), sources.len());
        assert_eq!(draws.len(), pipelines.len());
        self.render_to_format(
            "",
            "",
            draws,
            wgpu::TextureFormat::Rgba8Unorm,
            RasterConfiguration {
                state,
                pipelines,
                sources,
                additional_bindings: &[],
            },
        )
    }

    /// Renders separate shader modules into one ordinary sRGB colour and depth target.
    pub fn render_sources_srgb(
        &self,
        sources: &[&str],
        draws: &[Draw<'_>],
        pipelines: &[DrawPipeline<'_>],
    ) -> Vec<u8> {
        assert_eq!(draws.len(), sources.len());
        assert_eq!(draws.len(), pipelines.len());
        self.render_to_format(
            "",
            "",
            draws,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            RasterConfiguration {
                state: RasterState::default(),
                pipelines,
                sources,
                additional_bindings: &[],
            },
        )
    }

    /// Exercises the same hardware transfer as Bevy's ordinary sRGB target.
    pub fn render_srgb(&self, source: &str, vertex: &str, draws: &[Draw<'_>]) -> Vec<u8> {
        self.render_srgb_with_state(source, vertex, draws, RasterState::default())
    }

    /// Renders sRGB fixtures with the production colour mask and depth state.
    pub fn render_srgb_with_state(
        &self,
        source: &str,
        vertex: &str,
        draws: &[Draw<'_>],
        state: RasterState,
    ) -> Vec<u8> {
        self.render_to_format(
            source,
            vertex,
            draws,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            RasterConfiguration {
                state,
                pipelines: &[],
                sources: &[],
                additional_bindings: &[],
            },
        )
    }

    /// Resolves multisampled production draws before inspecting their final pixels.
    pub fn render_with_samples(
        &self,
        source: &str,
        vertex: &str,
        draws: &[Draw<'_>],
        samples: u32,
    ) -> Vec<u8> {
        self.render_to_format(
            source,
            vertex,
            draws,
            wgpu::TextureFormat::Rgba8Unorm,
            RasterConfiguration {
                state: RasterState {
                    multisample: wgpu::MultisampleState {
                        count: samples,
                        ..Default::default()
                    },
                    ..Default::default()
                },
                pipelines: &[],
                sources: &[],
                additional_bindings: &[],
            },
        )
    }

    fn render_to_format(
        &self,
        source: &str,
        vertex: &str,
        draws: &[Draw<'_>],
        target_format: wgpu::TextureFormat,
        configuration: RasterConfiguration<'_>,
    ) -> Vec<u8> {
        let RasterConfiguration {
            state,
            pipelines,
            sources,
            additional_bindings,
        } = configuration;
        let module = |source: &str| {
            self.device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: None,
                    source: wgpu::ShaderSource::Wgsl(Cow::Owned(source.to_owned())),
                })
        };
        let shared = sources.is_empty().then(|| module(source));
        let modules = sources
            .iter()
            .map(|source| module(source))
            .collect::<Vec<_>>();
        let size = wgpu::Extent3d {
            width: SNAPSHOT_SIDE,
            height: SNAPSHOT_SIDE,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage, sample_count| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let target = texture(
            target_format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            1,
        );
        let multisampled = (state.multisample.count > 1).then(|| {
            texture(
                target_format,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                state.multisample.count,
            )
        });
        let depth = state.depth_target.clone().unwrap_or_else(|| {
            texture(
                wgpu::TextureFormat::Depth32Float,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                state.multisample.count,
            )
        });
        let view = target.create_view(&Default::default());
        let multisampled_view = multisampled
            .as_ref()
            .map(|texture| texture.create_view(&Default::default()));
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (index, draw) in draws.iter().enumerate() {
            let entry = pipelines.get(index);
            let shader = shared.as_ref().unwrap_or_else(|| &modules[index]);
            let mut primitive = state.primitive;
            if let Some(entry) = entry {
                primitive.topology = entry.topology;
            }
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: state.layout.as_ref(),
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some(entry.map_or(vertex, |entry| entry.vertex)),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive,
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: Some(draw.write_depth),
                        depth_compare: Some(state.depth_compare),
                        stencil: Default::default(),
                        bias: state.depth_biases.get(index).copied().unwrap_or_default(),
                    }),
                    multisample: state.multisample,
                    fragment: Some(wgpu::FragmentState {
                        module: shader,
                        entry_point: Some(draw.fragment),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: target_format,
                            blend: draw.blend,
                            write_mask: state.write_mask,
                        })],
                    }),
                    multiview_mask: None,
                    cache: None,
                });
            let groups = std::iter::once(draw.bindings)
                .chain(additional_bindings.iter().copied())
                .enumerate()
                .map(|(group, entries)| {
                    let entries = if state.layout.is_some() {
                        entries.to_vec()
                    } else {
                        bindings_for(
                            sources.get(index).copied().unwrap_or(source),
                            &[entry.map_or(vertex, |entry| entry.vertex), draw.fragment],
                            group as u32,
                            entries,
                        )
                    };
                    self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: None,
                        layout: &pipeline.get_bind_group_layout(group as u32),
                        entries: &entries,
                    })
                })
                .collect::<Vec<_>>();
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: multisampled_view.as_ref().unwrap_or(&view),
                    depth_slice: None,
                    resolve_target: multisampled_view.as_ref().map(|_| &view),
                    ops: wgpu::Operations {
                        load: if index == 0 {
                            wgpu::LoadOp::Clear(state.clear_color)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: if index == 0 {
                            wgpu::LoadOp::Clear(0.0)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&pipeline);
            for (index, group) in groups.iter().enumerate() {
                pass.set_bind_group(index as u32, group, &[]);
            }
            pass.draw(draw.vertices.clone(), 0..1);
        }
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SNAPSHOT_SIDE) * u64::from(SNAPSHOT_SIDE) * 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SNAPSHOT_SIDE * 4),
                    rows_per_image: Some(SNAPSHOT_SIDE),
                },
            },
            size,
        );
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        readback
            .slice(..)
            .get_mapped_range()
            .expect("readback buffer is mapped")
            .to_vec()
    }
}

/// Saves square diagnostic pixels outside git when an offline snapshot run requests them.
#[cfg(test)]
pub fn save(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("CINNABAR_REVIEW_SNAPSHOT_DIR") {
        let side = (pixels.len() / 4).isqrt() as u32;
        image::save_buffer(
            std::path::PathBuf::from(directory).join(format!("{name}.png")),
            pixels,
            side,
            side,
            image::ColorType::Rgba8,
        )
        .unwrap();
    }
}

/// Packs the minimal imported View layout used by standalone shader validation.
pub fn view(matrix: bevy::math::Mat4, eye: bevy::math::Vec3) -> Vec<f32> {
    let mut words = matrix.to_cols_array().to_vec();
    for _ in 0..5 {
        words.extend(bevy::math::Mat4::IDENTITY.to_cols_array());
    }
    words.extend([
        eye.x,
        eye.y,
        eye.z,
        1.0,
        0.0,
        0.0,
        SNAPSHOT_SIDE as f32,
        SNAPSHOT_SIDE as f32,
    ]);
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_admission_skips_only_a_missing_adapter() {
        let missing = wgpu::RequestAdapterError::NotFound {
            active_backends: wgpu::Backends::empty(),
            requested_backends: wgpu::Backends::empty(),
            supported_backends: wgpu::Backends::empty(),
            no_fallback_backends: wgpu::Backends::empty(),
            no_adapter_backends: wgpu::Backends::empty(),
            incompatible_surface_backends: wgpu::Backends::empty(),
        };
        assert_eq!(
            fixture_adapter::<()>("missing-adapter policy", Err(missing)),
            None
        );
        assert_eq!(fixture_adapter("present-adapter policy", Ok(())), Some(()));
    }

    #[test]
    #[should_panic(expected = "GPU fixture adapter request failed")]
    fn fixture_admission_preserves_other_adapter_errors() {
        fixture_adapter::<()>(
            "adapter-error policy",
            Err(wgpu::RequestAdapterError::EnvNotSet),
        );
    }
}
