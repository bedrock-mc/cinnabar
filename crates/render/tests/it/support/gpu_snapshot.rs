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
use wgpu::util::DeviceExt;

pub const SNAPSHOT_SIDE: u32 = 256;

pub struct Gpu {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub backend: wgpu::Backend,
}

pub struct Draw<'a> {
    pub fragment: &'a str,
    pub vertices: Range<u32>,
    pub bindings: &'a [wgpu::BindGroupEntry<'a>],
    pub blend: Option<wgpu::BlendState>,
    pub write_depth: bool,
}

pub struct RasterState {
    pub primitive: wgpu::PrimitiveState,
    pub depth_compare: wgpu::CompareFunction,
    pub write_mask: wgpu::ColorWrites,
    pub multisample: wgpu::MultisampleState,
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
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            primitive: Default::default(),
            depth_compare: wgpu::CompareFunction::GreaterEqual,
            write_mask: wgpu::ColorWrites::ALL,
            multisample: Default::default(),
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
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
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
        self.render_to_format(
            source,
            vertex,
            draws,
            wgpu::TextureFormat::Rgba8Unorm,
            RasterConfiguration {
                state,
                pipelines: &[],
                sources: &[],
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
            },
        )
    }

    /// Exercises the same hardware transfer as Bevy's ordinary sRGB target.
    pub fn render_srgb(&self, source: &str, vertex: &str, draws: &[Draw<'_>]) -> Vec<u8> {
        self.render_to_format(
            source,
            vertex,
            draws,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            RasterConfiguration {
                state: RasterState::default(),
                pipelines: &[],
                sources: &[],
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
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
            state.multisample.count,
        );
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
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: shader,
                        entry_point: Some(entry.map_or(vertex, |entry| entry.vertex)),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive,
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: draw.write_depth,
                        depth_compare: state.depth_compare,
                        stencil: Default::default(),
                        bias: Default::default(),
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
                    multiview: None,
                    cache: None,
                });
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: draw.bindings,
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: multisampled_view.as_ref().unwrap_or(&view),
                    depth_slice: None,
                    resolve_target: multisampled_view.as_ref().map(|_| &view),
                    ops: wgpu::Operations {
                        load: if index == 0 {
                            wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.12,
                                g: 0.18,
                                b: 0.25,
                                a: 1.0,
                            })
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
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
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
        readback.slice(..).get_mapped_range().to_vec()
    }
}

/// Saves diagnostic pixels outside git when an offline snapshot run requests them.
pub fn save(name: &str, pixels: &[u8]) {
    if let Some(directory) = std::env::var_os("CINNABAR_REVIEW_SNAPSHOT_DIR") {
        image::save_buffer(
            std::path::PathBuf::from(directory).join(format!("{name}.png")),
            pixels,
            SNAPSHOT_SIDE,
            SNAPSHOT_SIDE,
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
