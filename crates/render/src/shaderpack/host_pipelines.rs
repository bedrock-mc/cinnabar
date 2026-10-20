//! The host's own full-screen passes around a pack (`blit.wgsl`): scene copies, presenting
//! and mip downsampling.

use super::pipelines::GpuContext;
use super::plan::MAX_COLOR_BUFFERS;
use wgpu::TextureFormat;

/// Host passes around the pack's programs.
pub(crate) struct HostPipelines {
    pub scene_colour: wgpu::RenderPipeline,
    pub scene_depth: wgpu::RenderPipeline,
    /// Writes the scene depth into the comparison `depthtex*`.
    pub scene_depth_attachment: Option<wgpu::RenderPipeline>,
    /// `present` per view target format.
    pub present: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    /// Copies pixels outside a camera viewport through the post-process target swap.
    pub preserve: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    /// `downsample` per mipmapped buffer format.
    pub downsample: Vec<(TextureFormat, wgpu::RenderPipeline)>,
    pub linear: wgpu::Sampler,
}

/// Builds the host passes for a pack's buffer and depth formats; `depth_targets` when the
/// pack has comparison `depthtex*` for the scene depth to fill, and
/// `float_depths` the float `depthtex*` copies it allocates (`targets::float_depths`).
pub(super) fn host_pipelines(
    gpu: &GpuContext,
    buffer_formats: &[Option<TextureFormat>; MAX_COLOR_BUFFERS as usize],
    depth_format: TextureFormat,
    depth_targets: bool,
    float_depths: [bool; 3],
    mipmap_formats: &[TextureFormat],
) -> HostPipelines {
    let device = &gpu.device;
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shader pack host passes"),
        source: wgpu::ShaderSource::Wgsl(include_str!("blit.wgsl").into()),
    });
    let pipeline = |entry: &str, targets: &[Option<wgpu::ColorTargetState>], depth: bool| {
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(entry),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("fullscreen"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: depth.then(|| wgpu::DepthStencilState {
                format: TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                targets,
            }),
            multiview_mask: None,
            cache: None,
        })
    };
    let target = |format: TextureFormat| {
        Some(wgpu::ColorTargetState {
            format,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })
    };
    let colour0 = buffer_formats[0].unwrap_or(TextureFormat::Rgba8Unorm);
    let mut formats: Vec<TextureFormat> = mipmap_formats.to_vec();
    formats.dedup();
    HostPipelines {
        scene_colour: pipeline("scene_colour", &[target(colour0)], false),
        scene_depth: pipeline(
            "scene_depth",
            &float_depths.map(|allocated| target(depth_format).filter(|_| allocated)),
            false,
        ),
        scene_depth_attachment: depth_targets
            .then(|| pipeline("scene_depth_attachment", &[], true)),
        present: [
            render_scene::SCENE_COLOR_FORMAT,
            render_scene::SCENE_HDR_FORMAT,
        ]
        .into_iter()
        .map(|f| (f, pipeline("present", &[target(f)], false)))
        .collect(),
        preserve: [
            render_scene::SCENE_COLOR_FORMAT,
            render_scene::SCENE_HDR_FORMAT,
        ]
        .into_iter()
        .map(|f| (f, pipeline("preserve", &[target(f)], false)))
        .collect(),
        downsample: formats
            .into_iter()
            .map(|f| (f, pipeline("downsample", &[target(f)], false)))
            .collect(),
        linear: device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("shader pack host linear"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        }),
    }
}
