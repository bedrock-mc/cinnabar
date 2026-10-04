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
}

impl Default for RasterState {
    fn default() -> Self {
        Self {
            primitive: Default::default(),
            depth_compare: wgpu::CompareFunction::GreaterEqual,
            write_mask: wgpu::ColorWrites::ALL,
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

impl Gpu {
    /// Creates a physical device for explicitly requested snapshot fixtures.
    pub fn new() -> Option<Self> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = finish(instance.request_adapter(&wgpu::RequestAdapterOptions::default()));
        let adapter = adapter.expect("snapshot fixtures require a native GPU adapter");
        assert_ne!(
            adapter.get_info().backend,
            wgpu::Backend::Noop,
            "native GPU required"
        );
        let (device, queue) =
            finish(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        Some(Self { device, queue })
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

    /// Renders production entry points with reverse depth, then reads their actual pixels.
    pub fn render(&self, source: &str, vertex: &str, draws: &[Draw<'_>]) -> Vec<u8> {
        self.render_with_state(source, vertex, draws, RasterState::default())
    }

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
            state,
        )
    }

    /// Exercises the same hardware transfer as Bevy's ordinary sRGB target.
    pub fn render_srgb(&self, source: &str, vertex: &str, draws: &[Draw<'_>]) -> Vec<u8> {
        self.render_to_format(
            source,
            vertex,
            draws,
            wgpu::TextureFormat::Rgba8UnormSrgb,
            RasterState::default(),
        )
    }

    fn render_to_format(
        &self,
        source: &str,
        vertex: &str,
        draws: &[Draw<'_>],
        target_format: wgpu::TextureFormat,
        state: RasterState,
    ) -> Vec<u8> {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(source)),
            });
        let size = wgpu::Extent3d {
            width: SNAPSHOT_SIDE,
            height: SNAPSHOT_SIDE,
            depth_or_array_layers: 1,
        };
        let texture = |format, usage| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let target = texture(
            target_format,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::RENDER_ATTACHMENT,
        );
        let view = target.create_view(&Default::default());
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for (index, draw) in draws.iter().enumerate() {
            let pipeline = self
                .device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some(vertex),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: state.primitive,
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: draw.write_depth,
                        depth_compare: state.depth_compare,
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
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
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
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
