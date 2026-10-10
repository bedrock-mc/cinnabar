//! Draws the setup window: the built-in panorama through the launcher's own shader, then the
//! CPU-drawn overlay composited on top.

use std::sync::Arc;

use anyhow::{Context, Result};
use render_model::panorama::PANORAMA_WGSL;
use render_model::{PanoramaFaces, PanoramaView};
use winit::window::Window;

const OVERLAY_WGSL: &str = r"
@group(0) @binding(0) var overlay: texture_2d<f32>;
@group(0) @binding(1) var overlay_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn overlay_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.position = vec4<f32>(corner * 2.0 - 1.0, 0.0, 1.0);
    out.uv = vec2<f32>(corner.x, 1.0 - corner.y);
    return out;
}

@fragment
fn overlay_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(overlay, overlay_sampler, in.uv);
}
";
const UNIFORM_BYTES: u64 = 32;

pub(super) struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    uniform: wgpu::Buffer,
    panorama_pipeline: wgpu::RenderPipeline,
    panorama_group: wgpu::BindGroup,
    overlay_pipeline: wgpu::RenderPipeline,
    overlay_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    overlay: Option<(wgpu::Texture, wgpu::BindGroup)>,
}

impl Gpu {
    pub(super) fn new(window: Arc<Window>, faces: &PanoramaFaces) -> Result<Self> {
        let size = window.inner_size();
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::from_env_or_default());
        let surface = instance
            .create_surface(window)
            .context("create window surface")?;
        let runtime = tokio::runtime::Builder::new_current_thread().build()?;
        let adapter = runtime
            .block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            }))
            .context("find a graphics adapter")?;
        let (device, queue) = runtime
            .block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("first-run setup"),
                required_limits:
                    wgpu::Limits::downlevel_webgl2_defaults().using_resolution(adapter.limits()),
                ..Default::default()
            }))
            .context("open the graphics device")?;
        let mut config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .context("surface is incompatible with the adapter")?;
        let formats = surface.get_capabilities(&adapter).formats;
        if let Some(srgb) = formats.iter().find(|format| format.is_srgb()) {
            config.format = *srgb;
        }
        config.present_mode = wgpu::PresentMode::AutoVsync;
        surface.configure(&device, &config);

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("panorama uniform"),
            size: UNIFORM_BYTES,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let faces_view = upload_faces(&device, &queue, faces);
        let panorama_layout = bind_group_layout(&device, true);
        let panorama_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("panorama"),
            layout: &panorama_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&faces_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let panorama_pipeline = pipeline(
            &device,
            &panorama_layout,
            PANORAMA_WGSL,
            ("panorama_vertex", "panorama_fragment"),
            config.format,
            None,
        );
        let overlay_layout = bind_group_layout(&device, false);
        let overlay_pipeline = pipeline(
            &device,
            &overlay_layout,
            OVERLAY_WGSL,
            ("overlay_vertex", "overlay_fragment"),
            config.format,
            Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
        );
        Ok(Self {
            surface,
            device,
            queue,
            config,
            uniform,
            panorama_pipeline,
            panorama_group,
            overlay_pipeline,
            overlay_layout,
            sampler,
            overlay: None,
        })
    }

    pub(super) fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    /// Replaces the overlay with premultiplied sRGB RGBA8 `pixels` of the given size.
    pub(super) fn set_overlay(&mut self, width: u32, height: u32, pixels: &[u8]) {
        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let reuse = self
            .overlay
            .as_ref()
            .is_some_and(|(texture, _)| texture.size() == size);
        if !reuse {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("setup overlay"),
                size,
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("setup overlay"),
                layout: &self.overlay_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.overlay = Some((texture, group));
        }
        if let Some((texture, _)) = &self.overlay {
            write_rgba(&self.queue, texture, size, 0, pixels);
        }
    }

    /// Presents one frame; a lost or outdated surface is reconfigured and skipped.
    pub(super) fn draw(&mut self, view: &PanoramaView) -> Result<()> {
        let Some(frame) = acquire_frame(self.surface.get_current_texture(), || {
            self.surface.configure(&self.device, &self.config);
        })?
        else {
            return Ok(());
        };
        let bytes: Vec<u8> = view
            .shader_uniform()
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        self.queue.write_buffer(&self.uniform, 0, &bytes);
        let target = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("setup"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.panorama_pipeline);
            pass.set_bind_group(0, &self.panorama_group, &[]);
            pass.draw(0..3, 0..1);
            if let Some((_, group)) = &self.overlay {
                pass.set_pipeline(&self.overlay_pipeline);
                pass.set_bind_group(0, group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        frame.present();
        Ok(())
    }
}

fn upload_faces(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    faces: &PanoramaFaces,
) -> wgpu::TextureView {
    let side = faces.side();
    let size = wgpu::Extent3d {
        width: side,
        height: side,
        depth_or_array_layers: 6,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("setup panorama"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let face_bytes = side as usize * side as usize * 4;
    for (layer, pixels) in faces.layer_major().chunks_exact(face_bytes).enumerate() {
        let one = wgpu::Extent3d {
            depth_or_array_layers: 1,
            ..size
        };
        write_rgba(queue, &texture, one, layer as u32, pixels);
    }
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

fn write_rgba(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    size: wgpu::Extent3d,
    layer: u32,
    pixels: &[u8],
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: 0,
                y: 0,
                z: layer,
            },
            aspect: wgpu::TextureAspect::All,
        },
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.width * 4),
            rows_per_image: Some(size.height),
        },
        size,
    );
}

/// Uniform + face array + sampler for the panorama, or texture + sampler for the overlay.
fn bind_group_layout(device: &wgpu::Device, panorama: bool) -> wgpu::BindGroupLayout {
    let texture = |binding, view_dimension| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
            view_dimension,
            multisampled: false,
        },
        count: None,
    };
    let sampler = |binding| wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    };
    let entries = if panorama {
        vec![
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(UNIFORM_BYTES),
                },
                count: None,
            },
            texture(1, wgpu::TextureViewDimension::D2Array),
            sampler(2),
        ]
    } else {
        vec![texture(0, wgpu::TextureViewDimension::D2), sampler(1)]
    };
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &entries,
    })
}

fn pipeline(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    source: &str,
    (vertex, fragment): (&str, &str),
    format: wgpu::TextureFormat,
    blend: Option<wgpu::BlendState>,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: None,
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some(vertex),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some(fragment),
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
        cache: None,
    })
}

/// Reconfigures recoverable surface losses and classifies acquisition failures.
fn acquire_frame<T>(
    result: Result<T, wgpu::SurfaceError>,
    mut reconfigure: impl FnMut(),
) -> Result<Option<T>> {
    match result {
        Ok(frame) => Ok(Some(frame)),
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            reconfigure();
            Ok(None)
        }
        Err(wgpu::SurfaceError::Timeout) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_fatal_surface_failure_must_not_be_retried() {
        let mut reconfigured = false;
        let result =
            acquire_frame::<()>(Err(wgpu::SurfaceError::OutOfMemory), || reconfigured = true);
        // Fatal errors must reach the setup host instead of silently skipping a frame.
        assert!(result.is_err(), "fatal surface failure was swallowed");
        assert!(!reconfigured);
    }
}
