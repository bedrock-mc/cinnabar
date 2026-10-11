//! Native fixture resources shared by the UI damage parity scenarios.

use super::super::*;
use crate::gpu_snapshot::Gpu;
use crate::ui_render::{
    self as ui,
    pipeline::{
        UiPipelineKey, UiPipelineSpecializer, ui_bind_group_layout, ui_pipeline_descriptor,
    },
};
use bevy::render::render_resource::Specializer;
use wgpu::util::DeviceExt;

pub(super) const SIDE: u32 = 64;
const FORMAT: wgpu::TextureFormat = super::super::super::composite::UI_LAYER_FORMAT;

pub(super) struct Raster {
    pub(super) gpu: Gpu,
    pub(super) binding: wgpu::BindGroup,
    pub(super) materials: [wgpu::RenderPipeline; 4],
    pub(super) clear: wgpu::RenderPipeline,
}

/// Builds the real UI vertex layout, shader and gamma-space premultiplied blend.
fn pipeline(
    gpu: &Gpu,
    shader: &wgpu::ShaderModule,
    layout: &wgpu::PipelineLayout,
    depth: (bool, bool),
    clear: bool,
) -> wgpu::RenderPipeline {
    let base = ui_pipeline_descriptor(ui_bind_group_layout());
    let descriptor = if clear {
        ui::composite::clear_pipeline_descriptor()
    } else {
        let mut descriptor = base.clone();
        UiPipelineSpecializer
            .specialize(
                UiPipelineKey {
                    msaa: ui::Msaa::Off,
                    hdr: false,
                    invert_blend: false,
                    layer: true,
                    depth_test: depth.0,
                    depth_write: depth.1,
                    isolated_depth: true,
                },
                &mut descriptor,
            )
            .unwrap();
        descriptor
    };
    let buffer = &base.vertex.buffers[0];
    let buffers = [wgpu::VertexBufferLayout {
        array_stride: buffer.array_stride,
        step_mode: buffer.step_mode,
        attributes: &buffer.attributes,
    }];
    gpu.device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("UI damage parity"),
            layout: (!clear).then_some(layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: descriptor.vertex.entry_point.as_deref(),
                compilation_options: Default::default(),
                buffers: if clear { &[] } else { &buffers },
            },
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: descriptor.fragment.as_ref().unwrap().entry_point.as_deref(),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: FORMAT,
                    blend: descriptor.fragment.as_ref().unwrap().targets[0]
                        .as_ref()
                        .unwrap()
                        .blend,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            depth_stencil: descriptor.depth_stencil.clone(),
            primitive: Default::default(),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        })
}

/// Uploads fixture bytes without changing the production representation.
pub(super) fn buffer(gpu: &Gpu, bytes: &[u8], usage: wgpu::BufferUsages) -> wgpu::Buffer {
    gpu.device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytes,
            usage,
        })
}

/// Creates color or depth storage for a fixed-size native offscreen capture.
pub(super) fn target(gpu: &Gpu, depth: bool) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("UI damage parity target"),
        size: wgpu::Extent3d {
            width: SIDE,
            height: SIDE,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: if depth {
            bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT
        } else {
            FORMAT
        },
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | if depth {
                wgpu::TextureUsages::empty()
            } else {
                wgpu::TextureUsages::COPY_SRC
            },
        view_formats: &[],
    })
}

impl Raster {
    /// Skips only a missing native adapter; all shader, device and rendering errors remain failures.
    pub(super) fn new(input: &UiRenderInput) -> Option<Self> {
        let gpu = Gpu::for_fixture("retained UI damage exact pixel parity")?;
        let entries = ui_bind_group_layout();
        let bindings = gpu
            .device
            .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: None,
                entries: &entries.entries,
            });
        let layout = gpu
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[Some(&bindings)],
                immediate_size: 0,
            });
        let source = super::super::super::shader::source(include_str!("../../ui.wgsl"));
        let clear_shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("production UI damage clear"),
                source: wgpu::ShaderSource::Wgsl(include_str!("../../ui_composite.wgsl").into()),
            });
        let shader = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pages: Vec<_> = input
            .textures
            .pages()
            .iter()
            .flat_map(|page| page.pixels().iter().copied())
            .collect();
        let texture = gpu.device.create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: 2,
                    height: 2,
                    depth_or_array_layers: 2,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &pages,
        );
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let viewport = buffer(
            &gpu,
            bytemuck::cast_slice(&[SIDE as f32, SIDE as f32, 0.0, 0.0]),
            wgpu::BufferUsages::UNIFORM,
        );
        let format = buffer(
            &gpu,
            bytemuck::cast_slice(&[0_u32; 4]),
            wgpu::BufferUsages::UNIFORM,
        );
        let sampler = gpu
            .device
            .create_sampler(&wgpu::SamplerDescriptor::default());
        let linear = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let binding = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &bindings,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: viewport.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&linear),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: format.as_entire_binding(),
                },
            ],
        });
        Some(Self {
            materials: [(false, false), (false, true), (true, false), (true, true)]
                .map(|depth| pipeline(&gpu, &shader, &layout, depth, false)),
            clear: pipeline(&gpu, &clear_shader, &layout, (false, false), true),
            gpu,
            binding,
        })
    }
}
